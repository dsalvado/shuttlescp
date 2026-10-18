//! Small modal "are you sure?" prompt for destructive actions.

use std::cell::Cell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

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
    let window = gtk::Window::builder()
        .title(wording.title)
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .build();
    // Added rather than set with the builder: setting `css_classes` would replace the
    // window's built-in classes (such as "background"), leaving its text unstyled.
    window.add_css_class("login-dialog");

    let question = gtk::Label::builder()
        .label(wording.question)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["heading"])
        .build();
    let detail = gtk::Label::builder()
        .label(wording.detail)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(48)
        .build();
    let cancel = gtk::Button::with_label("Cancel");
    let confirm = gtk::Button::builder()
        .label(wording.confirm_label)
        .css_classes(["destructive-action"])
        .build();
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    buttons.append(&cancel);
    buttons.append(&confirm);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(14);
    content.set_margin_end(14);
    content.append(&question);
    content.append(&detail);
    content.append(&buttons);
    window.set_child(Some(&content));

    // Set by the confirm button, read when the window closes, so `on_answer` runs once
    // whichever way the prompt is dismissed.
    let confirmed = Rc::new(Cell::new(false));
    {
        let (confirmed, window) = (Rc::clone(&confirmed), window.clone());
        confirm.connect_clicked(move |_| {
            confirmed.set(true);
            window.close();
        });
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    // Escape cancels, like in standard dialogs.
    let keys = gtk::EventControllerKey::new();
    {
        let window = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                window.close();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
    }
    window.add_controller(keys);
    window.connect_close_request(move |_| {
        on_answer(confirmed.get());
        gtk::glib::Propagation::Proceed
    });

    window.present();
    cancel.grab_focus();
}
