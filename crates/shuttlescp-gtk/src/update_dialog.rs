//! Telling the user about a newer release, downloading it and installing it (see
//! [`shuttlescp_core::update`] for the checks that make this safe).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};

use shuttlescp_core::about;
use shuttlescp_core::format::format_size;
use shuttlescp_core::update::{self, Manifest};

use crate::dialog::{self, ButtonKind, Dialog};

/// How often the dialog shows the download's progress.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// News from the download thread.
enum Download {
    /// Bytes so far, and in all.
    Progress(u64, u64),
    /// Finished: the downloaded file, or what went wrong.
    Done(Result<PathBuf, String>),
}

/// Say that the running version is the latest.
pub fn report_up_to_date(parent: &gtk::Window) {
    let dialog = Dialog::new(parent, "Check for Updates");
    dialog
        .content
        .append(&dialog::text("ShuttleSCP is up to date", &["heading"]));
    dialog.content.append(&dialog::text(
        &format!(
            "Version {} is the latest release.",
            env!("CARGO_PKG_VERSION")
        ),
        &[],
    ));
    let close = dialog.add_close_button("Close");
    dialog.window.present();
    close.grab_focus();
}

/// Say that looking for an update did not work.
pub fn report_error(parent: &gtk::Window, error: &str) {
    let dialog = Dialog::new(parent, "Check for Updates");
    dialog
        .content
        .append(&dialog::text("Cannot check for updates", &["heading"]));
    dialog
        .content
        .append(&dialog::text(&format!("{}.", capitalised(error)), &[]));
    let close = dialog.add_close_button("Close");
    dialog.window.present();
    close.grab_focus();
}

/// Offer a newer release. From an AppImage it can be downloaded and installed here;
/// otherwise the download page is offered instead.
///
/// # Arguments
///
/// * `parent` - Window the dialog stays above
/// * `manifest` - The newer release, from a verified manifest
/// * `on_installed` - Called with the AppImage's path once the new version replaced it,
///   to restart into it
pub fn offer(parent: &gtk::Window, manifest: Manifest, on_installed: impl Fn(PathBuf) + 'static) {
    let dialog = Dialog::new(parent, "Update Available");
    dialog.content.append(&dialog::text(
        &format!("ShuttleSCP {} is available", manifest.version),
        &["heading"],
    ));
    let published = manifest.published.get(..10).unwrap_or(&manifest.published);
    dialog.content.append(&dialog::text(
        &format!(
            "You have version {}. The new version was released on {published}.",
            env!("CARGO_PKG_VERSION")
        ),
        &[],
    ));
    let notes = gtk::LinkButton::with_label(
        &manifest.notes_url,
        &format!("What's new in {}", manifest.version),
    );
    notes.set_halign(gtk::Align::Start);
    notes.add_css_class("about-link");
    dialog.content.append(&notes);

    // On macOS the app does not replace itself (it has no Apple Developer ID to sign a
    // new copy with): the browser downloads the new disk image, which installs as the
    // first one did.
    if cfg!(target_os = "macos")
        && let Some(dmg) = &manifest.macos
    {
        dialog.content.append(&dialog::text(
            "Download it, open the disk image and drag ShuttleSCP onto Applications, \
             replacing this version.",
            &["dim-label"],
        ));
        dialog.add_close_button("Later");
        let (window, url) = (dialog.window.clone(), dmg.url.clone());
        dialog.add_button("Download", ButtonKind::Suggested, move || {
            open_uri(&url);
            window.close();
        });
        dialog.window.present();
        return;
    }
    let Some(appimage) = update::running_appimage() else {
        // Not an AppImage (e.g. built from source): nothing here can be replaced.
        dialog.content.append(&dialog::text(
            "Download the new version from the ShuttleSCP web page.",
            &["dim-label"],
        ));
        dialog.add_close_button("Later");
        let window = dialog.window.clone();
        dialog.add_button("Open Download Page", ButtonKind::Suggested, move || {
            open_uri(about::HOMEPAGE);
            window.close();
        });
        dialog.window.present();
        return;
    };

    let bar = gtk::ProgressBar::builder().visible(false).build();
    let status = dialog::text("", &["hint"]);
    status.set_visible(false);
    dialog.content.append(&bar);
    dialog.content.append(&status);
    let later = dialog.add_close_button("Later");
    let cancel = Arc::new(AtomicBool::new(false));
    let started = Rc::new(Cell::new(false));
    let install = {
        let (bar, status, later, cancel, started) = (
            bar.clone(),
            status.clone(),
            later.clone(),
            Arc::clone(&cancel),
            Rc::clone(&started),
        );
        let window = dialog.window.clone();
        let on_installed: Rc<dyn Fn(PathBuf)> = Rc::new(on_installed);
        move |button: &gtk::Button| {
            started.set(true);
            button.set_sensitive(false);
            later.set_label("Cancel");
            bar.set_visible(true);
            status.set_visible(true);
            status.set_label("Downloading...");
            start_download(
                &manifest,
                appimage.clone(),
                Widgets {
                    window: window.clone(),
                    bar: bar.clone(),
                    status: status.clone(),
                    later: later.clone(),
                    install: button.clone(),
                },
                Arc::clone(&cancel),
                Rc::clone(&on_installed),
            );
        }
    };
    let install_button = dialog.add_button("Install and Restart", ButtonKind::Suggested, || {});
    install_button.connect_clicked(install);
    // Closing the dialog (Cancel, Escape or its title bar) stops a download.
    dialog.window.connect_close_request(move |_| {
        if started.get() {
            cancel.store(true, Ordering::Relaxed);
        }
        glib::Propagation::Proceed
    });
    dialog.window.present();
    install_button.grab_focus();
}

/// The parts of the update dialog a download updates.
struct Widgets {
    window: gtk::Window,
    bar: gtk::ProgressBar,
    status: gtk::Label,
    later: gtk::Button,
    install: gtk::Button,
}

/// Download the update on a worker thread, showing its progress, and install it.
fn start_download(
    manifest: &Manifest,
    appimage: PathBuf,
    widgets: Widgets,
    cancel: Arc<AtomicBool>,
    on_installed: Rc<dyn Fn(PathBuf)>,
) {
    let Some(dir) = appimage.parent().map(std::path::Path::to_path_buf) else {
        return;
    };
    let (sender, news) = mpsc::channel();
    let asset = manifest.appimage.clone();
    tracing::info!("downloading update {}", asset.name);
    std::thread::spawn(move || {
        let result = update::download(
            &asset,
            &dir,
            |done, total| {
                let _ = sender.send(Download::Progress(done, total));
            },
            &cancel,
        );
        let _ = sender.send(Download::Done(result));
    });
    glib::timeout_add_local(PROGRESS_INTERVAL, move || {
        while let Ok(message) = news.try_recv() {
            match message {
                Download::Progress(done, total) => {
                    widgets
                        .bar
                        .set_fraction(shuttlescp_core::transfer::fraction(done, total));
                    widgets.status.set_label(&format!(
                        "Downloading... {} of {}",
                        format_size(done),
                        format_size(total)
                    ));
                }
                Download::Done(result) => {
                    finish(result, &appimage, &widgets, on_installed.as_ref());
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

/// The download ended: install it and restart, or say what went wrong.
fn finish(
    result: Result<PathBuf, String>,
    appimage: &std::path::Path,
    widgets: &Widgets,
    on_installed: &dyn Fn(PathBuf),
) {
    let installed = result.and_then(|downloaded| update::install(&downloaded, appimage));
    match installed {
        Ok(()) => {
            widgets.window.close();
            on_installed(appimage.to_path_buf());
        }
        // Closed during the download: nothing more to show.
        Err(_) if !widgets.window.is_visible() => {}
        Err(error) => {
            tracing::warn!("update not installed: {error}");
            widgets.bar.set_visible(false);
            widgets.status.add_css_class("error");
            widgets.status.set_label(&format!(
                "{}. You can download it from the ShuttleSCP web page instead.",
                capitalised(&error)
            ));
            widgets.later.set_label("Close");
            widgets.install.set_label("Open Download Page");
            widgets.install.set_sensitive(true);
            // Only the download page is offered from now on.
            let window = widgets.window.clone();
            widgets.install.connect_clicked(move |_| {
                open_uri(about::HOMEPAGE);
                window.close();
            });
        }
    }
}

/// Open a web page in the desktop's browser.
fn open_uri(uri: &str) {
    if let Err(error) = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>) {
        tracing::warn!("cannot open {uri}: {error}");
    }
}

/// A message as a sentence: the core's messages start in lower case.
fn capitalised(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_become_sentences() {
        assert_eq!(capitalised("cannot reach it"), "Cannot reach it");
        assert_eq!(capitalised(""), "");
    }
}
