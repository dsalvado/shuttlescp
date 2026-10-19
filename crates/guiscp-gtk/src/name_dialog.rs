//! Small modal prompt for a file or folder name (New Folder, Rename).

use relm4::gtk;
use relm4::gtk::prelude::*;

use guiscp_core::transfer::check_name;

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
    let window = gtk::Window::builder()
        .title(wording.title)
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .build();
    // Added rather than set with the builder, which would drop the window's own classes.
    window.add_css_class("login-dialog");

    let prompt = gtk::Label::builder()
        .label(wording.prompt)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(48)
        .build();
    let entry = gtk::Entry::builder()
        .text(wording.initial)
        .activates_default(true)
        .width_chars(40)
        .build();
    let problem = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["error", "dim-label"])
        .build();
    let cancel = gtk::Button::with_label("Cancel");
    let confirm = gtk::Button::builder()
        .label(wording.confirm_label)
        .css_classes(["suggested-action"])
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
    content.append(&prompt);
    content.append(&entry);
    content.append(&problem);
    content.append(&buttons);
    window.set_child(Some(&content));
    window.set_default_widget(Some(&confirm));

    // Check the name as it is typed.
    let validate = {
        let (confirm, problem) = (confirm.clone(), problem.clone());
        move |text: &str| {
            let result = check_name(text);
            confirm.set_sensitive(result.is_ok());
            problem.set_label(&result.err().unwrap_or_default());
        }
    };
    validate(wording.initial);
    entry.connect_changed(move |entry| validate(&entry.text()));

    {
        let (window, entry) = (window.clone(), entry.clone());
        confirm.connect_clicked(move |_| {
            let name = entry.text().to_string();
            if check_name(&name).is_ok() {
                window.close();
                on_name(name);
            }
        });
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
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
