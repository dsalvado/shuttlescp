//! Asking what to do with unfinished transfers (running or paused) before the app
//! quits: keep them to resume later, or give them up and delete their partial files.

use std::cell::Cell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::dialog::{self, ButtonKind, Dialog};
use crate::i18n::tr;

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
    let dialog = Dialog::new(parent, &tr!("quit-title"));
    dialog
        .content
        .append(&dialog::text(&tr!("quit-question"), &["heading"]));
    dialog.content.append(&dialog::text(
        &tr!("quit-detail", unfinished = unfinished_text(running, paused)),
        &[],
    ));
    // Set by a button, read when the window closes, so `on_choice` runs at most once.
    let choice = Rc::new(Cell::new(None));
    dialog.add_close_button(&tr!("dialog-cancel"));
    let choose = |label: &str, kind: ButtonKind, picked: QuitChoice| {
        let (choice, window) = (Rc::clone(&choice), dialog.window.clone());
        dialog.add_button(label, kind, move || {
            choice.set(Some(picked));
            window.close();
        })
    };
    choose(
        &tr!("quit-discard"),
        ButtonKind::Destructive,
        QuitChoice::Discard,
    );
    let keep = choose(&tr!("quit-keep"), ButtonKind::Suggested, QuitChoice::Keep);
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
    match (running, paused) {
        (_, 0) => tr!("quit-running", count = running),
        (0, _) => tr!("quit-paused", count = paused),
        _ => tr!(
            "quit-unfinished",
            count = running + paused,
            running = running,
            paused = paused
        ),
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
