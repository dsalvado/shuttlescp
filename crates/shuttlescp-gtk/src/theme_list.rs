//! Theme list shown inside the View menu: the current theme first with a check mark, a
//! separator, then every other theme, in a short scrollable list. Each theme's name is
//! centred, with a swatch of its window and accent colours beside it.

use std::collections::HashMap;
use std::f64::consts::PI;

use relm4::RelmRemoveAllExt;
use relm4::gtk;
use relm4::gtk::glib::variant::ToVariant;
use relm4::gtk::prelude::*;

use shuttlescp_core::config::ThemeChoice;

use crate::theme::{self, Swatch};

/// Action every row triggers; its target is the row's `ThemeChoice::key()`.
const THEME_ACTION: &str = "win.theme";
/// Tallest the list grows before it scrolls, in pixels (about ten rows).
const MAX_HEIGHT: i32 = 240;
/// Label of the entry that follows the desktop theme.
const SYSTEM_THEME_LABEL: &str = "System default";
/// Icon marking the current theme.
const CHECK_ICON: &str = "object-select-symbolic";
/// Width of the list, in pixels: wide enough to centre the names with room either side.
const LIST_WIDTH: i32 = 280;
/// Size of a theme's colour swatch, in pixels.
const SWATCH_WIDTH: i32 = 30;
const SWATCH_HEIGHT: i32 = 14;

/// The theme list widget and the themes it offers.
pub struct ThemeList {
    root: gtk::ScrolledWindow,
    list: gtk::ListBox,
    themes: Vec<String>,
    /// Each theme's preview colours, read once (themes without named colours have none).
    swatches: HashMap<String, Swatch>,
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
            .width_request(LIST_WIDTH)
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
        let dirs = theme::search_dirs();
        let swatches = themes
            .iter()
            .filter_map(|name| Some((name.clone(), theme::swatch(name, &dirs)?)))
            .collect();
        let theme_list = Self {
            root,
            list,
            themes,
            swatches,
        };
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
        self.list
            .append(&theme_row(&first, true, self.swatch_of(&first)));
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
            self.list
                .append(&theme_row(choice, false, self.swatch_of(choice)));
        }
        // Start at the top, where the current theme is.
        self.root.vadjustment().set_value(0.0);
    }

    /// The preview colours of a choice ("System default" has none: it is whatever the
    /// desktop uses).
    fn swatch_of(&self, choice: &ThemeChoice) -> Option<Swatch> {
        match choice {
            ThemeChoice::System => None,
            ThemeChoice::Named(name) => self.swatches.get(name).copied(),
        }
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

/// One theme entry: a check mark slot, the theme name centred, and its colour swatch.
///
/// # Arguments
///
/// * `choice` - Theme the row selects
/// * `checked` - Whether to show the check mark
/// * `swatch` - The theme's colours, if known
fn theme_row(choice: &ThemeChoice, checked: bool, swatch: Option<Swatch>) -> gtk::ListBoxRow {
    let check = gtk::Image::from_icon_name(CHECK_ICON);
    // `set_opacity` instead of hiding keeps every name aligned in the same column.
    check.set_opacity(if checked { 1.0 } else { 0.0 });
    let label = gtk::Label::builder()
        .label(match choice {
            ThemeChoice::System => SYSTEM_THEME_LABEL,
            ThemeChoice::Named(name) => name,
        })
        .xalign(0.5)
        .hexpand(true)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&check);
    content.append(&label);
    // Every row keeps the swatch's slot, so the names stay centred alike.
    content.append(&swatch_area(swatch));

    let row = gtk::ListBoxRow::builder().child(&content).build();
    row.set_action_name(Some(THEME_ACTION));
    row.set_action_target_value(Some(&choice.key().to_variant()));
    row
}

/// A small rounded chip: the window colour on the left half, the accent on the right,
/// with a thin outline that shows on light and dark menus alike. Empty without colours.
fn swatch_area(swatch: Option<Swatch>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(SWATCH_WIDTH)
        .content_height(SWATCH_HEIGHT)
        .valign(gtk::Align::Center)
        .build();
    let Some(swatch) = swatch else {
        return area;
    };
    area.set_tooltip_text(Some("Window and accent colours"));
    area.set_draw_func(move |_, cairo, width, height| {
        let (width, height) = (f64::from(width), f64::from(height));
        let rounded = |cairo: &gtk::cairo::Context| {
            let (x0, y0, x1, y1, radius) = (0.5, 0.5, width - 0.5, height - 0.5, 3.0);
            cairo.new_sub_path();
            cairo.arc(x1 - radius, y0 + radius, radius, -PI / 2.0, 0.0);
            cairo.arc(x1 - radius, y1 - radius, radius, 0.0, PI / 2.0);
            cairo.arc(x0 + radius, y1 - radius, radius, PI / 2.0, PI);
            cairo.arc(x0 + radius, y0 + radius, radius, PI, 1.5 * PI);
            cairo.close_path();
        };
        let paint = |colour: &gtk::gdk::RGBA, x: f64| {
            cairo.set_source_rgba(
                f64::from(colour.red()),
                f64::from(colour.green()),
                f64::from(colour.blue()),
                f64::from(colour.alpha()),
            );
            cairo.rectangle(x, 0.0, width / 2.0, height);
            let _ = cairo.fill();
        };
        let _ = cairo.save();
        rounded(cairo);
        cairo.clip();
        paint(&swatch.background, 0.0);
        paint(&swatch.accent, width / 2.0);
        let _ = cairo.restore();
        rounded(cairo);
        cairo.set_source_rgba(0.5, 0.5, 0.5, 0.6);
        cairo.set_line_width(1.0);
        let _ = cairo.stroke();
    });
    area
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
