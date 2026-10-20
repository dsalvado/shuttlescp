//! Small modal "are you sure?" prompt for destructive actions.

use std::cell::Cell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::dialog::{self, ButtonKind, Dialog};

/// Wording of a confirmation prompt.
#[derive(Debug, Clone, Copy)]
pub struct Confirmation<'a> {
    /// Window title, e.g. "Delete Site".
    pub title: &'a str,
    /// Bold question, e.g. "Delete me@host?".
    pub question: &'a str,
    /// Extra explanation under the question.
    pub detail: &'a str,
    /// Label of the destructive button, e.g. "Delete".
    pub confirm_label: &'a str,
}

/// Ask the user to confirm over `parent`. `on_answer` is called exactly once: `true`
/// when the user confirmed, `false` for Cancel, Escape or closing the prompt.
///
/// Cancel has the keyboard focus, so pressing Enter by accident does not confirm.
///
/// # Arguments
///
/// * `parent` - Window the prompt belongs to
/// * `wording` - Texts shown in the prompt
/// * `on_answer` - Receives the decision
pub fn ask(parent: &gtk::Window, wording: Confirmation<'_>, on_answer: impl Fn(bool) + 'static) {
    let dialog = Dialog::new(parent, wording.title);
    dialog
        .content
        .append(&dialog::text(wording.question, &["heading"]));
    dialog.content.append(&dialog::text(wording.detail, &[]));

    // Set by the confirm button, read when the window closes, so `on_answer` runs once
    // whichever way the prompt is dismissed (Cancel, Escape or the title bar).
    let confirmed = Rc::new(Cell::new(false));
    let cancel = dialog.add_close_button("Cancel");
    {
        let (confirmed, window) = (Rc::clone(&confirmed), dialog.window.clone());
        dialog.add_button(wording.confirm_label, ButtonKind::Destructive, move || {
            confirmed.set(true);
            window.close();
        });
    }
    dialog.window.connect_close_request(move |_| {
        on_answer(confirmed.get());
        gtk::glib::Propagation::Proceed
    });

    dialog.window.present();
    cancel.grab_focus();
}
