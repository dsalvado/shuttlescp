//! Asking for the passphrase of a protected SSH private key, with the choice to save it
//! in the desktop keyring.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;
use secrecy::SecretString;

use crate::dialog::{self, ButtonKind, Dialog};
use shuttlescp_core::tr;

/// What the user answered.
#[derive(Debug)]
pub struct KeyPassphrase {
    /// The passphrase typed.
    pub passphrase: SecretString,
    /// Save it in the keyring once it has worked.
    pub save: bool,
}

/// Ask for the passphrase of `key_file` over `parent`. `on_answer` is called exactly
/// once, with `None` if the user cancelled.
///
/// # Arguments
///
/// * `parent` - Window the prompt belongs to
/// * `key_file` - The protected key
/// * `wrong` - The last passphrase tried did not open the key
/// * `on_answer` - Receives the answer
pub fn ask(
    parent: &gtk::Window,
    key_file: &Path,
    wrong: bool,
    on_answer: impl Fn(Option<KeyPassphrase>) + 'static,
) {
    let dialog = Dialog::new(parent, &tr!("key-passphrase-title"));
    let name = key_file.file_name().map_or_else(
        || key_file.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    dialog.content.append(&dialog::text(
        &tr!("key-passphrase-question", key = name.to_string()),
        &["heading"],
    ));
    let explanation = if wrong {
        tr!("key-passphrase-wrong")
    } else {
        tr!("key-passphrase-explain")
    };
    let explanation = dialog::text(&explanation, if wrong { &["error"] } else { &[] });
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .width_request(320)
        .build();
    let save = gtk::CheckButton::with_label(&tr!("key-passphrase-save"));
    save.set_tooltip_text(Some(&tr!("key-passphrase-save-tooltip")));
    dialog.content.append(&explanation);
    dialog.content.append(&entry);
    dialog.content.append(&save);

    // Set by Unlock, handed over when the window closes, so `on_answer` runs exactly
    // once however the prompt is dismissed.
    let answer: Rc<RefCell<Option<KeyPassphrase>>> = Rc::default();
    dialog.add_close_button(&tr!("dialog-cancel"));
    let unlock = {
        let (answer, entry, save, window) = (
            Rc::clone(&answer),
            entry.clone(),
            save.clone(),
            dialog.window.clone(),
        );
        dialog.add_button(
            &tr!("key-passphrase-unlock"),
            ButtonKind::Suggested,
            move || {
                if entry.text().is_empty() {
                    return;
                }
                *answer.borrow_mut() = Some(KeyPassphrase {
                    passphrase: SecretString::from(entry.text().as_str()),
                    save: save.is_active(),
                });
                window.close();
            },
        )
    };
    let window = dialog.window;
    window.set_default_widget(Some(&unlock));
    window.connect_close_request(move |_| {
        on_answer(answer.borrow_mut().take());
        gtk::glib::Propagation::Proceed
    });
    window.present();
    entry.grab_focus();
}
