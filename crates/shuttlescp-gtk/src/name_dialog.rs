//! Small modal prompt for a file or folder name (New Folder, Rename).

use relm4::gtk;
use relm4::gtk::prelude::*;

use shuttlescp_core::transfer::check_name;

use crate::dialog::{self, ButtonKind, Dialog};

/// Wording and starting text of a name prompt.
#[derive(Debug, Clone, Copy)]
pub struct NamePrompt<'a> {
    /// Window title, e.g. "Rename".
    pub title: &'a str,
    /// Text above the entry, e.g. "New name for report.pdf:".
    pub prompt: &'a str,
    /// Starting text of the entry.
    pub initial: &'a str,
    /// Label of the confirm button, e.g. "Create".
    pub confirm_label: &'a str,
}

/// Ask over `parent` for a name. `on_name` gets the name only if the user confirms;
/// invalid names cannot be confirmed (the problem is shown under the entry).
///
/// When renaming a file, the part before its extension starts selected, so typing
/// replaces the name but keeps ".pdf".
///
/// # Arguments
///
/// * `parent` - Window the prompt belongs to
/// * `wording` - Texts shown in the prompt
/// * `on_name` - Receives the confirmed name
pub fn ask(parent: &gtk::Window, wording: NamePrompt<'_>, on_name: impl Fn(String) + 'static) {
    let dialog = Dialog::new(parent, wording.title);
    let entry = gtk::Entry::builder()
        .text(wording.initial)
        .activates_default(true)
        .width_chars(40)
        .build();
    // Shown only while the name cannot be used.
    let problem = dialog::text("", &["error", "hint"]);
    dialog.content.append(&dialog::text(wording.prompt, &[]));
    dialog.content.append(&entry);
    dialog.content.append(&problem);

    dialog.add_close_button("Cancel");
    let confirm = {
        let (window, entry) = (dialog.window.clone(), entry.clone());
        dialog.add_button(wording.confirm_label, ButtonKind::Suggested, move || {
            let name = entry.text().to_string();
            if check_name(&name).is_ok() {
                window.close();
                on_name(name);
            }
        })
    };
    let window = dialog.window;
    window.set_default_widget(Some(&confirm));

    // Check the name as it is typed.
    let validate = {
        let (confirm, problem) = (confirm.clone(), problem.clone());
        move |text: &str| {
            let result = check_name(text);
            confirm.set_sensitive(result.is_ok());
            problem.set_visible(result.is_err());
            problem.set_label(&result.err().unwrap_or_default());
        }
    };
    validate(wording.initial);
    entry.connect_changed(move |entry| validate(&entry.text()));

    window.present();
    entry.grab_focus();
    // Select the name without its extension ("report" of "report.pdf"); a name with
    // no extension, or starting with a dot (".bashrc"), is selected whole.
    let stem_chars = match wording.initial.rfind('.') {
        Some(dot) if dot > 0 => wording.initial[..dot].chars().count(),
        _ => wording.initial.chars().count(),
    };
    entry.select_region(0, i32::try_from(stem_chars).unwrap_or(-1));
}
