//! Asking what to do with unfinished transfers (running or paused) before the app
//! quits: keep them to resume later, or give them up and delete their partial files.

use std::cell::Cell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::dialog::{self, ButtonKind, Dialog};

/// What to do with the unfinished transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitChoice {
    /// Quit, keeping them (and their partial files) to resume the next time the site
    /// is connected.
    Keep,
    /// Stop them, delete their partial files and forget them, then quit.
    Discard,
}

/// Ask over `parent`. `on_choice` is called once a quitting button is pressed; Cancel,
/// Escape or closing the prompt leaves the app open.
///
/// # Arguments
///
/// * `parent` - The main window
/// * `running` - Transfers running
/// * `paused` - Transfers paused
/// * `on_choice` - Receives the decision
pub fn ask(
    parent: &gtk::Window,
    running: usize,
    paused: usize,
    on_choice: impl Fn(QuitChoice) + 'static,
) {
    let dialog = Dialog::new(parent, "Quit");
    dialog
        .content
        .append(&dialog::text("Quit ShuttleSCP?", &["heading"]));
    dialog.content.append(&dialog::text(
        &format!(
            "{} Keep them to resume the next time you connect, or delete their partial \
             files.",
            unfinished_text(running, paused)
        ),
        &[],
    ));
    // Set by a button, read when the window closes, so `on_choice` runs at most once.
    let choice = Rc::new(Cell::new(None));
    dialog.add_close_button("Cancel");
    let choose = |label: &str, kind: ButtonKind, picked: QuitChoice| {
        let (choice, window) = (Rc::clone(&choice), dialog.window.clone());
        dialog.add_button(label, kind, move || {
            choice.set(Some(picked));
            window.close();
        })
    };
    choose(
        "Delete Partial Files and Quit",
        ButtonKind::Destructive,
        QuitChoice::Discard,
    );
    let keep = choose(
        "Keep for Later and Quit",
        ButtonKind::Suggested,
        QuitChoice::Keep,
    );
    dialog.window.connect_close_request(move |_| {
        if let Some(choice) = choice.get() {
            on_choice(choice);
        }
        gtk::glib::Propagation::Proceed
    });
    dialog.window.present();
    keep.grab_focus();
}

/// "2 transfers are unfinished (1 running, 1 paused)." and the like.
fn unfinished_text(running: usize, paused: usize) -> String {
    let count = running + paused;
    let subject = if count == 1 {
        String::from("1 transfer is")
    } else {
        format!("{count} transfers are")
    };
    match (running, paused) {
        (_, 0) => format!("{subject} still running."),
        (0, _) => format!("{subject} paused."),
        _ => format!("{subject} unfinished ({running} running, {paused} paused)."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_says_what_is_unfinished() {
        assert_eq!(unfinished_text(1, 0), "1 transfer is still running.");
        assert_eq!(unfinished_text(0, 2), "2 transfers are paused.");
        assert_eq!(
            unfinished_text(1, 1),
            "2 transfers are unfinished (1 running, 1 paused)."
        );
    }
}
