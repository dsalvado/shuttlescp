//! "File already exists" prompt: overwrite, skip, or cancel the transfer.

use std::cell::Cell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use guiscp_core::format::{format_modified, format_size};
use guiscp_core::transfer::{ConflictAnswer, ConflictChoice, FileInfo};

/// Ask over `parent` what to do about `name`, which already exists in `dest_dir`.
/// `on_answer` is called exactly once; closing the prompt cancels the transfer, so
/// nothing is overwritten without a clear yes.
///
/// # Arguments
///
/// * `parent` - Window the prompt belongs to
/// * `name` - The file's name
/// * `dest_dir` - The folder it is in
/// * `existing` - The file there now
/// * `incoming` - The file that would replace it
/// * `on_answer` - Receives the decision
pub fn ask(
    parent: &gtk::Window,
    name: &str,
    dest_dir: &str,
    existing: FileInfo,
    incoming: FileInfo,
    on_answer: impl Fn(ConflictAnswer) + 'static,
) {
    let window = gtk::Window::builder()
        .title("File Already Exists")
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .build();
    // Added rather than set with the builder, which would drop the window's own classes.
    window.add_css_class("login-dialog");

    let question = gtk::Label::builder()
        .label(format!("{name} already exists in {dest_dir}"))
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(56)
        .css_classes(["heading"])
        .build();
    let newer = match (existing.modified, incoming.modified) {
        (Some(there), Some(new)) if new > there => (false, true),
        (Some(there), Some(new)) if there > new => (true, false),
        _ => (false, false),
    };
    let details = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(2)
        .build();
    for (row, (label, info, is_newer)) in [
        ("Existing:", existing, newer.0),
        ("New:", incoming, newer.1),
    ]
    .into_iter()
    .enumerate()
    {
        // `row` is 0 or 1, so it always fits.
        let row = i32::try_from(row).unwrap_or(0);
        details.attach(&dim_label(label), 0, row, 1, 1);
        details.attach(
            &gtk::Label::builder()
                .label(describe(info, is_newer))
                .xalign(0.0)
                .build(),
            1,
            row,
            1,
            1,
        );
    }
    let for_all = gtk::CheckButton::with_label("Do the same for the other existing files");
    let cancel = gtk::Button::with_label("Cancel Transfer");
    let skip = gtk::Button::with_label("Skip");
    let overwrite = gtk::Button::builder()
        .label("Overwrite")
        .css_classes(["destructive-action"])
        .build();
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    buttons.append(&cancel);
    buttons.append(&skip);
    buttons.append(&overwrite);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(14);
    content.set_margin_end(14);
    content.append(&question);
    content.append(&details);
    content.append(&for_all);
    content.append(&buttons);
    window.set_child(Some(&content));

    // Set by the buttons, read when the window closes, so `on_answer` runs once
    // whichever way the prompt is dismissed.
    let choice = Rc::new(Cell::new(ConflictChoice::CancelJob));
    for (button, picked) in [
        (&cancel, ConflictChoice::CancelJob),
        (&skip, ConflictChoice::Skip),
        (&overwrite, ConflictChoice::Overwrite),
    ] {
        let (choice, window) = (Rc::clone(&choice), window.clone());
        button.connect_clicked(move |_| {
            choice.set(picked);
            window.close();
        });
    }
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
        on_answer(ConflictAnswer {
            choice: choice.get(),
            apply_to_all: for_all.is_active(),
        });
        gtk::glib::Propagation::Proceed
    });

    window.present();
    // Skip is the safe default for Enter.
    skip.grab_focus();
}

/// "3.1 MiB, 2026-10-19 15:41 (newer)".
fn describe(info: FileInfo, newer: bool) -> String {
    let mut text = format_size(info.size);
    if let Some(modified) = info.modified {
        text.push_str(", ");
        text.push_str(&format_modified(modified));
    }
    if newer {
        text.push_str(" (newer)");
    }
    text
}

/// A dimmed, left-aligned caption.
fn dim_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build()
}
