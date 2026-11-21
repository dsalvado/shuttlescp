//! The Permissions dialog: read / write / execute for owner, group and others, the same
//! as a number ("0644"), the special bits, and the owner and group. For one item or
//! several; where several differ, a box shows "mixed" and is left alone unless clicked.
//!
//! Also the small prompt for the sudo password, when a change needs root.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;
use secrecy::SecretString;

use shuttlescp_core::permissions::{PermissionEdit, Scope};
use shuttlescp_core::tr;

use crate::dialog::{self, ButtonKind, Dialog};

/// One of the items the dialog is for, as its listing describes it.
#[derive(Debug, Clone)]
pub struct Item {
    /// Its name, shown when it is the only one.
    pub name: String,
    /// Its permission bits, if known.
    pub mode: Option<u32>,
    /// Its owner, if known.
    pub owner: Option<String>,
    /// Its group, if known.
    pub group: Option<String>,
    /// It is a folder (then the dialog offers to change what is inside too).
    pub is_dir: bool,
}

/// What the dialog may offer, besides the permissions.
#[derive(Debug, Clone, Copy)]
pub struct Offers {
    /// Owner and group can be changed (not over FTP).
    pub owners: bool,
    /// Sudo can be used (SSH servers).
    pub sudo: bool,
}

/// The dialog's answer.
#[derive(Debug, Clone)]
pub struct Answer {
    /// What to change.
    pub edit: PermissionEdit,
    /// Change everything inside the folders too: which items, and whether execute goes
    /// only on folders (and files that have it already).
    pub inside: Option<(Scope, bool)>,
    /// Make the changes as root, through sudo.
    pub sudo: bool,
}

/// The rwx boxes, row by row (owner, group, others), with the bit each one stands for.
const RWX: [[u32; 3]; 3] = [
    [0o400, 0o200, 0o100],
    [0o040, 0o020, 0o010],
    [0o004, 0o002, 0o001],
];
/// Setuid, setgid and sticky.
const SPECIAL: [u32; 3] = [0o4000, 0o2000, 0o1000];

/// Ask how to change `items`. `on_answer` gets the answer, or `None` for Cancel.
///
/// # Arguments
///
/// * `parent` - Window the dialog belongs to
/// * `items` - What is being changed (at least one)
/// * `offers` - What else can be changed, and how
/// * `on_answer` - Receives the answer, exactly once
pub fn ask(
    parent: &gtk::Window,
    items: &[Item],
    offers: Offers,
    on_answer: impl FnOnce(Option<Answer>) + 'static,
) {
    let owners = offers.owners;
    let dialog = Dialog::new(parent, &tr!("permissions-title"));
    let heading = match items {
        [item] => item.name.clone(),
        _ => tr!("permissions-items", count = items.len()),
    };
    dialog.content.append(&dialog::text(&heading, &["heading"]));

    // The boxes: rows Owner / Group / Others, columns Read / Write / Execute.
    let grid = gtk::Grid::builder()
        .row_spacing(dialog::SPACING)
        .column_spacing(dialog::SPACING * 3)
        .build();
    for (column, title) in [
        tr!("permissions-read"),
        tr!("permissions-write"),
        tr!("permissions-execute"),
    ]
    .iter()
    .enumerate()
    {
        grid.attach(&gtk::Label::new(Some(title)), column as i32 + 1, 0, 1, 1);
    }
    let mut checks: Vec<(u32, gtk::CheckButton)> = Vec::new();
    for (row, (title, bits)) in [
        tr!("permissions-owner-row"),
        tr!("permissions-group-row"),
        tr!("permissions-others-row"),
    ]
    .iter()
    .zip(RWX)
    .enumerate()
    {
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.0);
        grid.attach(&label, 0, row as i32 + 1, 1, 1);
        for (column, bit) in bits.into_iter().enumerate() {
            let check = gtk::CheckButton::new();
            check.set_halign(gtk::Align::Center);
            grid.attach(&check, column as i32 + 1, row as i32 + 1, 1, 1);
            checks.push((bit, check));
        }
    }
    dialog.content.append(&grid);

    // The same as a number, kept in step with the boxes.
    let number_row = gtk::Box::new(gtk::Orientation::Horizontal, dialog::SPACING);
    let number = gtk::Entry::builder()
        .max_length(4)
        .width_chars(6)
        .placeholder_text(tr!("permissions-mixed"))
        .build();
    number.add_css_class("monospace");
    number_row.append(&gtk::Label::new(Some(&tr!("permissions-number"))));
    number_row.append(&number);
    dialog.content.append(&number_row);

    // Special bits, out of the way.
    let special = gtk::Box::new(gtk::Orientation::Horizontal, dialog::SPACING * 2);
    for (bit, title) in SPECIAL.into_iter().zip([
        tr!("permissions-setuid"),
        tr!("permissions-setgid"),
        tr!("permissions-sticky"),
    ]) {
        let check = gtk::CheckButton::with_label(&title);
        special.append(&check);
        checks.push((bit, check));
    }
    let advanced = gtk::Expander::new(Some(&tr!("permissions-special")));
    advanced.set_child(Some(&special));
    dialog.content.append(&advanced);

    // Start each box from the items: ticked if all have it, clear if none, mixed else.
    let modes: Vec<Option<u32>> = items.iter().map(|item| item.mode).collect();
    for (bit, check) in &checks {
        match bit_state(&modes, *bit) {
            Some(on) => check.set_active(on),
            None => check.set_inconsistent(true),
        }
    }
    // Specials in use: open the section, so they are not changed unseen.
    advanced.set_expanded(
        SPECIAL
            .iter()
            .any(|&bit| bit_state(&modes, bit) != Some(false)),
    );

    let checks = Rc::new(checks);
    // Set while one side updates the other, so they don't echo back and forth.
    let syncing = Rc::new(Cell::new(false));
    show_number(&number, &checks, &syncing);
    for (_, check) in checks.iter() {
        let (number, checks, syncing) = (number.clone(), Rc::clone(&checks), Rc::clone(&syncing));
        check.connect_toggled(move |check| {
            // A click decides a mixed box.
            check.set_inconsistent(false);
            if !syncing.get() {
                show_number(&number, &checks, &syncing);
            }
        });
    }
    {
        let (checks, syncing) = (Rc::clone(&checks), Rc::clone(&syncing));
        number.connect_changed(move |number| {
            if syncing.get() {
                return;
            }
            // Only a whole number (3 or 4 octal digits) sets the boxes.
            let text = number.text();
            let Some(mode) = parse_number(&text) else {
                return;
            };
            syncing.set(true);
            for (bit, check) in checks.iter() {
                check.set_inconsistent(false);
                check.set_active(mode & bit != 0);
            }
            syncing.set(false);
        });
    }

    // Owner and group: filled in when all items agree, empty (meaning "keep") otherwise.
    let owner_grid = gtk::Grid::builder()
        .row_spacing(dialog::SPACING)
        .column_spacing(dialog::SPACING)
        .margin_top(dialog::SPACING)
        .build();
    let common_owner = common(items.iter().map(|item| item.owner.as_deref()));
    let common_group = common(items.iter().map(|item| item.group.as_deref()));
    let mut fields = Vec::new();
    for (row, (title, value)) in [
        (tr!("permissions-owner"), &common_owner),
        (tr!("permissions-group"), &common_group),
    ]
    .into_iter()
    .enumerate()
    {
        let label = gtk::Label::new(Some(&title));
        label.set_xalign(0.0);
        let entry = gtk::Entry::builder()
            .text(value.as_deref().unwrap_or_default())
            .placeholder_text(tr!("permissions-keep"))
            .hexpand(true)
            .activates_default(true)
            .sensitive(owners)
            .build();
        if !owners {
            entry.set_tooltip_text(Some(&tr!("permissions-no-owners")));
        }
        owner_grid.attach(&label, 0, row as i32, 1, 1);
        owner_grid.attach(&entry, 1, row as i32, 1, 1);
        fields.push(entry);
    }
    dialog.content.append(&owner_grid);

    // Folders: also everything inside them, as one job.
    let inside = gtk::CheckButton::with_label(&tr!("permissions-inside"));
    let scope = gtk::DropDown::from_strings(&[
        &tr!("permissions-scope-all"),
        &tr!("permissions-scope-files"),
        &tr!("permissions-scope-folders"),
    ]);
    let execute_on_folders = gtk::CheckButton::with_label(&tr!("permissions-x-folders"));
    execute_on_folders.set_active(true);
    execute_on_folders.set_tooltip_text(Some(&tr!("permissions-x-folders-hint")));
    let inside_options = gtk::Box::new(gtk::Orientation::Horizontal, dialog::SPACING * 2);
    inside_options.set_margin_start(dialog::SPACING * 4);
    inside_options.append(&scope);
    inside_options.append(&execute_on_folders);
    inside_options.set_sensitive(false);
    {
        let options = inside_options.clone();
        inside.connect_toggled(move |inside| options.set_sensitive(inside.is_active()));
    }
    if items.iter().any(|item| item.is_dir) {
        inside.set_margin_top(dialog::SPACING);
        dialog.content.append(&inside);
        dialog.content.append(&inside_options);
    }
    let sudo = gtk::CheckButton::with_label(&tr!("permissions-use-sudo"));
    sudo.set_tooltip_text(Some(&tr!("permissions-use-sudo-hint")));
    if offers.sudo {
        dialog.content.append(&sudo);
    }

    // Set by Change, handed over when the window closes, so `on_answer` runs once
    // however the dialog is dismissed.
    let answer: Rc<RefCell<Option<Answer>>> = Rc::default();
    dialog.add_close_button(&tr!("dialog-cancel"));
    let change = {
        let (answer, checks, window) = (
            Rc::clone(&answer),
            Rc::clone(&checks),
            dialog.window.clone(),
        );
        let (owner, group) = (fields[0].clone(), fields[1].clone());
        let (inside, scope, execute_on_folders, sudo) = (
            inside.clone(),
            scope.clone(),
            execute_on_folders.clone(),
            sudo.clone(),
        );
        dialog.add_button(
            &tr!("permissions-change"),
            ButtonKind::Suggested,
            move || {
                let mut edit = PermissionEdit::default();
                for (bit, check) in checks.iter() {
                    if check.is_inconsistent() {
                        continue;
                    }
                    if check.is_active() {
                        edit.set |= bit;
                    } else {
                        edit.clear |= bit;
                    }
                }
                edit.owner = changed_name(&owner.text(), common_owner.as_deref());
                edit.group = changed_name(&group.text(), common_group.as_deref());
                let chosen_scope = match scope.selected() {
                    1 => Scope::Files,
                    2 => Scope::Folders,
                    _ => Scope::All,
                };
                *answer.borrow_mut() = Some(Answer {
                    edit,
                    inside: (inside.is_active() && inside.parent().is_some())
                        .then(|| (chosen_scope, execute_on_folders.is_active())),
                    sudo: sudo.is_active() && sudo.parent().is_some(),
                });
                window.close();
            },
        )
    };
    let window = dialog.window;
    window.set_default_widget(Some(&change));
    let on_answer = RefCell::new(Some(on_answer));
    window.connect_close_request(move |_| {
        if let Some(on_answer) = on_answer.borrow_mut().take() {
            on_answer(answer.borrow_mut().take());
        }
        glib::Propagation::Proceed
    });
    window.present();
}

/// Ask for the password sudo needs on `server`. `on_answer` gets it, or `None` for
/// Cancel.
///
/// # Arguments
///
/// * `parent` - Window the prompt belongs to
/// * `server` - The server, to name it
/// * `again` - The last password was refused: say so
/// * `on_answer` - Receives the answer, exactly once
pub fn ask_sudo_password(
    parent: &gtk::Window,
    server: &str,
    again: bool,
    on_answer: impl FnOnce(Option<SecretString>) + 'static,
) {
    let dialog = Dialog::new(parent, &tr!("permissions-password-title"));
    let question = if again {
        tr!("permissions-password-wrong")
    } else {
        tr!("permissions-password-prompt", server = server.to_owned())
    };
    dialog
        .content
        .append(&dialog::text(&question, &["heading"]));
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .width_request(280)
        .build();
    dialog.content.append(&entry);

    let answer: Rc<RefCell<Option<SecretString>>> = Rc::default();
    dialog.add_close_button(&tr!("dialog-cancel"));
    let ok = {
        let (answer, entry, window) = (Rc::clone(&answer), entry.clone(), dialog.window.clone());
        dialog.add_button(
            &tr!("permissions-password-ok"),
            ButtonKind::Suggested,
            move || {
                *answer.borrow_mut() = Some(SecretString::from(entry.text().as_str()));
                // Don't leave it in the field.
                entry.set_text("");
                window.close();
            },
        )
    };
    let window = dialog.window;
    window.set_default_widget(Some(&ok));
    let on_answer = RefCell::new(Some(on_answer));
    window.connect_close_request(move |_| {
        if let Some(on_answer) = on_answer.borrow_mut().take() {
            on_answer(answer.borrow_mut().take());
        }
        glib::Propagation::Proceed
    });
    window.present();
    entry.grab_focus();
}

/// Show the boxes as a number, or nothing ("mixed") while any box is mixed.
fn show_number(number: &gtk::Entry, checks: &[(u32, gtk::CheckButton)], syncing: &Cell<bool>) {
    let text = if checks.iter().any(|(_, check)| check.is_inconsistent()) {
        String::new()
    } else {
        let mode = checks
            .iter()
            .filter(|(_, check)| check.is_active())
            .fold(0, |mode, (bit, _)| mode | bit);
        format!("{mode:04o}")
    };
    syncing.set(true);
    number.set_text(&text);
    syncing.set(false);
}

/// Whether every item has `bit` (`Some(true)`), none has (`Some(false)`), or they
/// differ or some are unknown (`None`, shown as mixed).
fn bit_state(modes: &[Option<u32>], bit: u32) -> Option<bool> {
    let mut states = modes.iter().map(|mode| mode.map(|mode| mode & bit != 0));
    let first = states.next().flatten()?;
    states.all(|state| state == Some(first)).then_some(first)
}

/// A permission number such as "644" or "2775" (3 or 4 octal digits).
fn parse_number(text: &str) -> Option<u32> {
    let text = text.trim();
    (matches!(text.len(), 3 | 4) && text.bytes().all(|byte| (b'0'..=b'7').contains(&byte)))
        .then(|| u32::from_str_radix(text, 8).ok())
        .flatten()
}

/// The value all items share, if they do and it is known.
fn common<'a>(mut values: impl Iterator<Item = Option<&'a str>>) -> Option<String> {
    let first = values.next().flatten()?;
    values
        .all(|value| value == Some(first))
        .then(|| first.to_owned())
}

/// The owner or group to set: what was typed, unless empty (keep) or what all items
/// already have.
fn changed_name(typed: &str, common: Option<&str>) -> Option<String> {
    let typed = typed.trim();
    (!typed.is_empty() && Some(typed) != common).then(|| typed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_start_mixed_where_items_differ() {
        let modes = [Some(0o644), Some(0o664)];
        assert_eq!(bit_state(&modes, 0o400), Some(true));
        assert_eq!(bit_state(&modes, 0o020), None);
        assert_eq!(bit_state(&modes, 0o001), Some(false));
        // An item whose permissions are unknown makes every box mixed.
        assert_eq!(bit_state(&[Some(0o644), None], 0o400), None);
    }

    #[test]
    fn numbers_need_three_or_four_octal_digits() {
        assert_eq!(parse_number("644"), Some(0o644));
        assert_eq!(parse_number("2775"), Some(0o2775));
        assert_eq!(parse_number("64"), None);
        assert_eq!(parse_number("648"), None);
    }

    #[test]
    fn owner_fields_change_only_what_was_typed() {
        assert_eq!(changed_name("", Some("alex")), None);
        assert_eq!(changed_name("alex", Some("alex")), None);
        assert_eq!(
            changed_name(" www-data ", Some("alex")),
            Some("www-data".into())
        );
        assert_eq!(common([Some("a"), Some("b")].into_iter()), None);
        assert_eq!(common([Some("a"), Some("a")].into_iter()), Some("a".into()));
    }
}
