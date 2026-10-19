//! Small modal prompt for the passphrase that protects passwords in site exports.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;
use secrecy::SecretString;

pub use guiscp_core::site_export::PassphrasePurpose;
use guiscp_core::site_export::passphrase_problem;

/// What the user decided.
#[derive(Debug)]
pub enum PassphraseAnswer {
    /// Export without any passwords (only offered for [`PassphrasePurpose::Export`]).
    WithoutPasswords,
    /// The passphrase to use.
    Passphrase(SecretString),
}

/// Show the prompt over `parent`. `on_answer` is called exactly once, with `None` if the
/// user cancelled.
///
/// # Arguments
///
/// * `parent` - Window the prompt belongs to
/// * `purpose` - Export (choose and confirm) or Import (enter once)
/// * `on_answer` - Receives the user's decision
pub fn ask(
    parent: &gtk::Window,
    purpose: PassphrasePurpose,
    on_answer: impl Fn(Option<PassphraseAnswer>) + 'static,
) {
    let exporting = purpose == PassphrasePurpose::Export;
    let window = gtk::Window::builder()
        .title(if exporting {
            "Export Sites"
        } else {
            "Import Sites"
        })
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .build();

    let explanation = gtk::Label::builder()
        .label(if exporting {
            "Saved passwords are encrypted with a passphrase you choose.\n\
             You will need it to import the file on another computer."
        } else {
            "This file contains encrypted passwords.\n\
             Enter the passphrase it was exported with."
        })
        .xalign(0.0)
        .wrap(true)
        .build();
    let include = gtk::CheckButton::builder()
        .label("Include saved passwords")
        .active(true)
        .visible(exporting)
        .build();
    let passphrase = password_entry(if exporting {
        "Passphrase (at least 8 characters)"
    } else {
        "Passphrase"
    });
    let confirm = password_entry("Repeat passphrase");
    confirm.set_visible(exporting);
    let hint = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();

    let cancel = gtk::Button::with_label("Cancel");
    let ok = gtk::Button::builder()
        .label(if exporting { "Export" } else { "Import" })
        .css_classes(["suggested-action"])
        .sensitive(false)
        .build();
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&ok);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    for margin in [
        gtk::Widget::set_margin_top,
        gtk::Widget::set_margin_bottom,
        gtk::Widget::set_margin_start,
        gtk::Widget::set_margin_end,
    ] {
        margin(content.upcast_ref(), 12);
    }
    content.append(&explanation);
    content.append(&include);
    content.append(&passphrase);
    content.append(&confirm);
    content.append(&hint);
    content.append(&buttons);
    window.set_child(Some(&content));

    // Re-check the inputs whenever any of them changes.
    let validate = {
        let (include, passphrase, confirm, hint, ok) = (
            include.clone(),
            passphrase.clone(),
            confirm.clone(),
            hint.clone(),
            ok.clone(),
        );
        move || {
            let problem = passphrase_problem(
                purpose,
                include.is_active(),
                &passphrase.text(),
                &confirm.text(),
            );
            passphrase.set_sensitive(!exporting || include.is_active());
            confirm.set_sensitive(include.is_active());
            hint.set_label(problem.unwrap_or(""));
            ok.set_sensitive(problem.is_none());
        }
    };
    let validate = Rc::new(validate);
    for entry in [&passphrase, &confirm] {
        let validate = Rc::clone(&validate);
        entry.connect_changed(move |_| validate());
    }
    {
        let validate = Rc::clone(&validate);
        include.connect_toggled(move |_| validate());
    }
    validate();

    // The answer is stored here by OK and handed over when the window closes, so
    // `on_answer` runs exactly once however the prompt is dismissed.
    let answer: Rc<RefCell<Option<PassphraseAnswer>>> = Rc::default();
    {
        // Pressing Enter in the last field acts like OK when the input is valid.
        let last_field = if exporting {
            confirm.clone()
        } else {
            passphrase.clone()
        };
        let (answer, include, passphrase, window) = (
            Rc::clone(&answer),
            include.clone(),
            passphrase.clone(),
            window.clone(),
        );
        let activate_ok = move || {
            *answer.borrow_mut() = Some(if exporting && !include.is_active() {
                PassphraseAnswer::WithoutPasswords
            } else {
                PassphraseAnswer::Passphrase(SecretString::from(passphrase.text().as_str()))
            });
            window.close();
        };
        let activate_ok = Rc::new(activate_ok);
        let on_click = Rc::clone(&activate_ok);
        ok.connect_clicked(move |_| on_click());
        let ok_button = ok.clone();
        last_field.connect_activate(move |_| {
            if ok_button.is_sensitive() {
                activate_ok();
            }
        });
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    window.connect_close_request(move |_| {
        on_answer(answer.borrow_mut().take());
        gtk::glib::Propagation::Proceed
    });
    window.present();
}

/// A password-style entry (characters hidden).
///
/// # Arguments
///
/// * `placeholder` - Grey text shown while empty
fn password_entry(placeholder: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .visibility(false)
        .input_purpose(gtk::InputPurpose::Password)
        .placeholder_text(placeholder)
        .width_chars(32)
        .build()
}
