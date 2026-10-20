//! "File already exists" prompt: overwrite, skip, or cancel the transfer.

use std::cell::Cell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::prelude::*;

use shuttlescp_core::format::{format_modified, format_size};
use shuttlescp_core::transfer::{ConflictAnswer, ConflictChoice, FileInfo};

use crate::dialog::{self, ButtonKind, Dialog};

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
    let dialog = Dialog::new(parent, "File Already Exists");
    let question = dialog::text(
        &format!("{name} already exists in {dest_dir}"),
        &["heading"],
    );
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
    dialog.content.append(&question);
    dialog.content.append(&details);
    dialog.content.append(&for_all);

    // Set by the buttons, read when the window closes, so `on_answer` runs once
    // whichever way the prompt is dismissed (closing it cancels).
    let choice = Rc::new(Cell::new(ConflictChoice::CancelJob));
    let mut skip = None;
    for (label, kind, picked) in [
        (
            "Cancel Transfer",
            ButtonKind::Normal,
            ConflictChoice::CancelJob,
        ),
        ("Skip", ButtonKind::Normal, ConflictChoice::Skip),
        (
            "Overwrite",
            ButtonKind::Destructive,
            ConflictChoice::Overwrite,
        ),
    ] {
        let (choice, window) = (Rc::clone(&choice), dialog.window.clone());
        let button = dialog.add_button(label, kind, move || {
            choice.set(picked);
            window.close();
        });
        if picked == ConflictChoice::Skip {
            skip = Some(button);
        }
    }
    let window = dialog.window;
    window.connect_close_request(move |_| {
        on_answer(ConflictAnswer {
            choice: choice.get(),
            apply_to_all: for_all.is_active(),
        });
        gtk::glib::Propagation::Proceed
    });

    window.present();
    // Skip is the safe default for Enter.
    if let Some(skip) = skip {
        skip.grab_focus();
    }
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
