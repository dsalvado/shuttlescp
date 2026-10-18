//! Theme list shown inside the View menu: the current theme first with a check mark, a
//! separator, then every other theme, in a short scrollable list.

use relm4::RelmRemoveAllExt;
use relm4::gtk;
use relm4::gtk::glib::variant::ToVariant;
use relm4::gtk::prelude::*;

use crate::theme::ThemeChoice;

/// Action every row triggers; its target is the row's `ThemeChoice::action_target()`.
const THEME_ACTION: &str = "win.theme";
/// Tallest the list grows before it scrolls, in pixels (about ten rows).
const MAX_HEIGHT: i32 = 240;
/// Label of the entry that follows the desktop theme.
const SYSTEM_THEME_LABEL: &str = "System default";
/// Icon marking the current theme.
const CHECK_ICON: &str = "object-select-symbolic";

/// The theme list widget and the themes it offers.
pub struct ThemeList {
    root: gtk::ScrolledWindow,
    list: gtk::ListBox,
    themes: Vec<String>,
}

impl ThemeList {
    /// Build the list.
    ///
    /// # Arguments
    ///
    /// * `themes` - Installed theme names (offered after "System default")
    /// * `current` - Theme in use, shown first with a check mark
    pub fn new(themes: Vec<String>, current: &ThemeChoice) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["theme-list"])
            .build();
        // Close the menu once a theme is picked, as a normal menu item would.
        list.connect_row_activated(|list, _| {
            if let Some(popover) = list
                .ancestor(gtk::Popover::static_type())
                .and_downcast::<gtk::Popover>()
            {
                popover.popdown();
            }
        });
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(MAX_HEIGHT)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        let theme_list = Self { root, list, themes };
        theme_list.show_current(current);
        theme_list
    }

    /// The scrollable widget to embed in the menu.
    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// Rebuild the rows so `current` is first and checked.
    ///
    /// # Arguments
    ///
    /// * `current` - Theme now in use
    pub fn show_current(&self, current: &ThemeChoice) {
        self.list.remove_all();
        let (first, others) = menu_order(&self.themes, current);
        self.list.append(&theme_row(&first, true));
        let separator = gtk::ListBoxRow::builder()
            .activatable(false)
            .selectable(false)
            .can_focus(false)
            // Styled as a thin line in style.css, not as a full-height row.
            .css_classes(["separator-row"])
            .child(&gtk::Separator::new(gtk::Orientation::Horizontal))
            .build();
        self.list.append(&separator);
        for choice in &others {
            self.list.append(&theme_row(choice, false));
        }
        // Start at the top, where the current theme is.
        self.root.vadjustment().set_value(0.0);
    }
}

/// Order the menu entries: the current theme, then every other choice ("System default"
/// first, then installed themes in their given order).
///
/// # Arguments
///
/// * `themes` - Installed theme names
/// * `current` - Theme in use
///
/// # Returns
///
/// `(current, others)`; `others` never contains `current`
fn menu_order(themes: &[String], current: &ThemeChoice) -> (ThemeChoice, Vec<ThemeChoice>) {
    let others = std::iter::once(ThemeChoice::System)
        .chain(themes.iter().cloned().map(ThemeChoice::Named))
        .filter(|choice| choice != current)
        .collect();
    (current.clone(), others)
}

/// One theme entry: a check mark slot followed by the theme name.
///
/// # Arguments
///
/// * `choice` - Theme the row selects
/// * `checked` - Whether to show the check mark
fn theme_row(choice: &ThemeChoice, checked: bool) -> gtk::ListBoxRow {
    let check = gtk::Image::from_icon_name(CHECK_ICON);
    // `set_opacity` instead of hiding keeps every name aligned in the same column.
    check.set_opacity(if checked { 1.0 } else { 0.0 });
    let label = gtk::Label::builder()
        .label(match choice {
            ThemeChoice::System => SYSTEM_THEME_LABEL,
            ThemeChoice::Named(name) => name,
        })
        .xalign(0.0)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&check);
    content.append(&label);

    let row = gtk::ListBoxRow::builder().child(&content).build();
    row.set_action_name(Some(THEME_ACTION));
    row.set_action_target_value(Some(&choice.action_target().to_variant()));
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themes() -> Vec<String> {
        vec![String::from("Adwaita"), String::from("Zorin")]
    }

    #[test]
    fn menu_order_puts_named_current_theme_first() {
        let current = ThemeChoice::Named(String::from("Zorin"));
        let (first, others) = menu_order(&themes(), &current);
        assert_eq!(first, current);
        assert_eq!(
            others,
            [
                ThemeChoice::System,
                ThemeChoice::Named(String::from("Adwaita"))
            ]
        );
    }

    #[test]
    fn menu_order_lists_system_first_when_not_current() {
        let (first, others) = menu_order(&themes(), &ThemeChoice::System);
        assert_eq!(first, ThemeChoice::System);
        assert_eq!(
            others,
            [
                ThemeChoice::Named(String::from("Adwaita")),
                ThemeChoice::Named(String::from("Zorin"))
            ]
        );
    }
}
