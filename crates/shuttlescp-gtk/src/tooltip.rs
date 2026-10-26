//! Setting tooltips without keeping them from ever showing.
//!
//! Setting a widget's tooltip makes GTK look again at the tooltip under the pointer,
//! which restarts the delay before it shows. Widgets updated several times a second
//! (transfer rows, the file panes during a transfer) that set their tooltips on every
//! update kept every tooltip in the window from appearing, so they only set one when
//! its text changes.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// [`set_tooltip_if_changed`](TooltipExt::set_tooltip_if_changed) for every widget.
pub trait TooltipExt {
    /// Set the tooltip only if it differs from the one shown now.
    fn set_tooltip_if_changed(&self, text: Option<&str>);
}

impl<W: IsA<gtk::Widget>> TooltipExt for W {
    fn set_tooltip_if_changed(&self, text: Option<&str>) {
        if self.tooltip_text().as_deref() != text {
            self.set_tooltip_text(text);
        }
    }
}
