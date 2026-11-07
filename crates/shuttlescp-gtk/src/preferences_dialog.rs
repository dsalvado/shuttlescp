//! The Preferences window (File > Preferences): settings that apply to the whole app:
//! the interface language, and verifying file checksums after completion.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::dialog::{self, Dialog};
use shuttlescp_core::format::format_size;
use shuttlescp_core::i18n;
use shuttlescp_core::tr;
use shuttlescp_core::transfer::{DEFAULT_READ_BACK_UP_TO, Verify};

/// The sizes "Read back files up to" offers: never, 100 MiB, 1 GiB, 10 GiB, any size
/// (in the units sizes are shown in, so a file's shown size compares directly).
const READ_BACK_CHOICES: [Option<u64>; 5] = [
    None,
    Some(100 << 20),
    Some(DEFAULT_READ_BACK_UP_TO),
    Some(10 << 30),
    Some(u64::MAX),
];

/// Open the Preferences window over `parent`.
///
/// # Arguments
///
/// * `parent` - The main window
/// * `chosen` - The interface language in the settings
/// * `running` - The interface language the app runs in (it changes at the next start)
/// * `on_language` - Receives the language tag the user picks
/// * `verify` - How copies are checked, as the settings say
/// * `on_verify` - Receives each change to it
pub fn show(
    parent: &gtk::Window,
    chosen: &str,
    running: &str,
    on_language: impl Fn(String) + 'static,
    verify: Verify,
    on_verify: impl Fn(Verify) + 'static,
) {
    let dialog = Dialog::new(parent, &tr!("preferences-title"));

    // Each translation's own name for its language, English first.
    let offered = i18n::languages();
    let names: Vec<&str> = offered.iter().map(|(_, name)| name.as_str()).collect();
    let languages = gtk::DropDown::from_strings(&names);
    let position = offered
        .iter()
        .position(|(tag, _)| tag == chosen)
        .unwrap_or(0);
    languages.set_selected(u32::try_from(position).unwrap_or(0));

    let label = gtk::Label::with_mnemonic(&tr!("preferences-language"));
    label.set_xalign(0.0);
    label.set_mnemonic_widget(Some(&languages));
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(dialog::SPACING)
        .build();
    grid.attach(&label, 0, 0, 1, 1);
    grid.attach(&languages, 1, 0, 1, 1);
    dialog.content.append(&grid);

    // Shown while the language picked is not the one the app runs in.
    let restart = dialog::text(&tr!("preferences-restart"), &["dim-label"]);
    restart.set_visible(chosen != running);
    dialog.content.append(&restart);

    dialog.content.append(&verify_settings(verify, on_verify));

    let running = running.to_owned();
    languages.connect_selected_notify(move |languages| {
        let Some((tag, _)) = usize::try_from(languages.selected())
            .ok()
            .and_then(|index| offered.get(index))
        else {
            return;
        };
        restart.set_visible(*tag != running);
        on_language(tag.clone());
    });

    let close = dialog.add_close_button(&tr!("dialog-close"));
    dialog.window.set_default_widget(Some(&close));
    dialog.window.present();
}

/// "Verify file checksums after completion" and "Read back files up to", which follows
/// it; each change goes to `on_verify` at once.
fn verify_settings(verify: Verify, on_verify: impl Fn(Verify) + 'static) -> gtk::Box {
    let check = gtk::CheckButton::with_mnemonic(&tr!("preferences-verify"));
    check.set_active(verify.enabled);

    let names: Vec<String> = READ_BACK_CHOICES
        .iter()
        .map(|choice| match choice {
            None => tr!("read-back-never"),
            Some(u64::MAX) => tr!("read-back-any-size"),
            Some(bytes) => format_size(*bytes),
        })
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let sizes = gtk::DropDown::from_strings(&names);
    // A size not offered (a hand-edited file) shows as the default, 1 GiB.
    let position = READ_BACK_CHOICES
        .iter()
        .position(|choice| *choice == verify.read_back_up_to)
        .unwrap_or(2);
    sizes.set_selected(u32::try_from(position).unwrap_or(2));
    sizes.set_sensitive(verify.enabled);
    let label = gtk::Label::with_mnemonic(&tr!("preferences-read-back"));
    label.set_xalign(0.0);
    label.set_mnemonic_widget(Some(&sizes));
    label.set_sensitive(verify.enabled);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_margin_start(24);
    row.append(&label);
    row.append(&sizes);
    let note = dialog::text(&tr!("preferences-read-back-note"), &["dim-label"]);
    note.set_margin_start(24);

    let changed = std::rc::Rc::new(move |check: &gtk::CheckButton, sizes: &gtk::DropDown| {
        let read_back_up_to = usize::try_from(sizes.selected())
            .ok()
            .and_then(|index| READ_BACK_CHOICES.get(index).copied())
            .flatten();
        on_verify(Verify {
            enabled: check.is_active(),
            read_back_up_to,
        });
    });
    check.connect_toggled({
        let (sizes, label, changed) = (sizes.clone(), label.clone(), changed.clone());
        move |check| {
            sizes.set_sensitive(check.is_active());
            label.set_sensitive(check.is_active());
            changed(check, &sizes);
        }
    });
    sizes.connect_selected_notify({
        let check = check.clone();
        move |sizes| changed(&check, sizes)
    });

    let settings = gtk::Box::new(gtk::Orientation::Vertical, dialog::SPACING);
    settings.set_margin_top(dialog::SPACING);
    settings.append(&check);
    settings.append(&row);
    settings.append(&note);
    settings
}
