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

/// What to do with an installed update while transfers run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartChoice {
    /// Restart now, stopping the transfers (they can be resumed later).
    Now,
    /// Restart by itself once the transfers have finished.
    WhenTransfersFinish,
    /// Not now: restart from the status bar's notice later.
    Later,
}

/// Ask when to restart into an installed update while transfers run. `on_choice` is
/// called once a button is pressed; closing the prompt otherwise (Escape, its title
/// bar) leaves things as they were.
///
/// # Arguments
///
/// * `parent` - Window the prompt stays above
/// * `version` - The new version
/// * `running` - How many transfers run
/// * `on_choice` - Receives the decision
pub fn ask_restart(
    parent: &gtk::Window,
    version: &str,
    running: usize,
    on_choice: impl Fn(RestartChoice) + 'static,
) {
    let dialog = Dialog::new(parent, "Restart");
    dialog.content.append(&dialog::text(
        &format!("Restart into ShuttleSCP {version}?"),
        &["heading"],
    ));
    let running = match running {
        1 => String::from("A transfer is still running."),
        count => format!("{count} transfers are still running."),
    };
    dialog.content.append(&dialog::text(
        &format!(
            "{running} ShuttleSCP can restart by itself once they finish, or now: they \
             stop, and can be resumed the next time you connect to the server."
        ),
        &[],
    ));
    // Set by a button, read when the window closes, so `on_choice` runs at most once.
    let choice = Rc::new(Cell::new(None));
    let choose = |label: &str, kind: ButtonKind, picked: RestartChoice| {
        let (choice, window) = (Rc::clone(&choice), dialog.window.clone());
        dialog.add_button(label, kind, move || {
            choice.set(Some(picked));
            window.close();
        })
    };
    choose("Later", ButtonKind::Normal, RestartChoice::Later);
    choose("Restart Now", ButtonKind::Destructive, RestartChoice::Now);
    let when_finished = choose(
        "When Transfers Finish",
        ButtonKind::Suggested,
        RestartChoice::WhenTransfersFinish,
    );
    dialog.window.connect_close_request(move |_| {
        if let Some(choice) = choice.get() {
            on_choice(choice);
        }
        glib::Propagation::Proceed
    });
    dialog.window.present();
    when_finished.grab_focus();
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

/// Offer a newer release. From an AppImage, or on Windows with the installer, it can be
/// downloaded and installed here (see [`self_update`]); on macOS the browser downloads
/// the disk image; otherwise the download page is offered instead.
///
/// # Arguments
///
/// * `parent` - Window the dialog stays above
/// * `manifest` - The newer release, from a verified manifest
/// * `on_installed` - Called once the download is in place, with the program to start
///   after quitting: the new AppImage, or on Windows the installer
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
    let Some((asset, target)) = self_update(&manifest) else {
        // E.g. built from source on Linux: nothing here can be replaced.
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
                asset.clone(),
                target.clone(),
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

/// Download the update on a worker thread, showing its progress, and put it at `target`.
fn start_download(
    asset: update::Asset,
    target: PathBuf,
    widgets: Widgets,
    cancel: Arc<AtomicBool>,
    on_installed: Rc<dyn Fn(PathBuf)>,
) {
    let Some(dir) = target.parent().map(std::path::Path::to_path_buf) else {
        return;
    };
    let (sender, news) = mpsc::channel();
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
                    finish(result, &target, &widgets, on_installed.as_ref());
                    return glib::ControlFlow::Break;
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

/// The download ended: put it in place and restart, or say what went wrong.
fn finish(
    result: Result<PathBuf, String>,
    target: &std::path::Path,
    widgets: &Widgets,
    on_installed: &dyn Fn(PathBuf),
) {
    let installed = result.and_then(|downloaded| update::install(&downloaded, target));
    match installed {
        Ok(()) => {
            widgets.window.close();
            on_installed(target.to_path_buf());
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

/// What this copy of the app can update itself with: the release's file and where the
/// download goes. On Linux that is the running AppImage, replaced in place; on Windows
/// the installer, in a temporary folder, which runs once the app has closed. `None`
/// when the app cannot update itself (a build from source on Linux; a release without
/// a Windows installer).
fn self_update(manifest: &Manifest) -> Option<(update::Asset, PathBuf)> {
    if cfg!(windows) {
        let asset = manifest.windows.clone()?;
        // The name comes from the signed manifest; only its last part is used.
        let name = std::path::Path::new(&asset.name).file_name()?.to_owned();
        let dir = std::env::temp_dir().join("shuttlescp-update");
        std::fs::create_dir_all(&dir)
            .inspect_err(|error| tracing::warn!("cannot create {}: {error}", dir.display()))
            .ok()?;
        return Some((asset, dir.join(name)));
    }
    Some((manifest.appimage.clone(), update::running_appimage()?))
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
