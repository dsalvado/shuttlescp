//! The Preferences window (File > Preferences): settings that apply to the whole app.
//! For now, the interface language.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::dialog::{self, Dialog};

/// The interface languages offered: language tag (as the translation folders are named)
/// and name, the first being the default.
pub const LANGUAGES: &[(&str, &str)] = &[("en-US", "English")];

/// Open the Preferences window over `parent`.
///
/// # Arguments
///
/// * `parent` - The main window
/// * `chosen` - The interface language in the settings
/// * `running` - The interface language the app runs in (it changes at the next start)
/// * `on_language` - Receives the language tag the user picks
pub fn show(
    parent: &gtk::Window,
    chosen: &str,
    running: &str,
    on_language: impl Fn(String) + 'static,
) {
    let dialog = Dialog::new(parent, "Preferences");

    let names: Vec<&str> = LANGUAGES.iter().map(|(_, name)| *name).collect();
    let languages = gtk::DropDown::from_strings(&names);
    let position = LANGUAGES
        .iter()
        .position(|(tag, _)| *tag == chosen)
        .unwrap_or(0);
    languages.set_selected(u32::try_from(position).unwrap_or(0));

    let label = gtk::Label::with_mnemonic("Interface _language:");
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
    let restart = dialog::text(
        "Restart ShuttleSCP to use the new language.",
        &["dim-label"],
    );
    restart.set_visible(chosen != running);
    dialog.content.append(&restart);

    let running = running.to_owned();
    languages.connect_selected_notify(move |languages| {
        let Some((tag, _)) = usize::try_from(languages.selected())
            .ok()
            .and_then(|index| LANGUAGES.get(index))
        else {
            return;
        };
        restart.set_visible(*tag != running);
        on_language((*tag).to_owned());
    });

    let close = dialog.add_close_button("Close");
    dialog.window.set_default_widget(Some(&close));
    dialog.window.present();
}
