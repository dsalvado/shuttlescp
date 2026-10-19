//! A live view of the end of the log file, like `tail -f`, for the bottom panel.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use guiscp_core::log_reader::LogReader;

/// How often the file is checked for new lines.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Lines kept in the view; older ones are dropped.
const MAX_LINES: i32 = 2000;

/// Build the scrollable, read-only log view and start following `path`.
///
/// # Arguments
///
/// * `path` - Log file to follow, or `None` if logging to a file is unavailable
///
/// # Returns
///
/// The widget to put in the bottom panel
pub fn log_tail(path: Option<PathBuf>) -> gtk::ScrolledWindow {
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .left_margin(6)
        .top_margin(4)
        .bottom_margin(4)
        .css_classes(["log-tail"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&view)
        .vexpand(true)
        .build();
    follow_the_end(&scroller);

    let Some(path) = path else {
        view.buffer()
            .set_text("Logging to a file is not available, so there is nothing to show.");
        return scroller;
    };

    let reader = Rc::new(RefCell::new(LogReader::new(path)));
    // Show what is already there right away instead of after the first interval.
    if let Some(text) = reader.borrow_mut().read_new_lines() {
        append(&view, &text);
    }
    let weak_view = view.downgrade();
    glib::timeout_add_local(POLL_INTERVAL, move || {
        // Stop polling once the view is gone (window closed).
        let Some(view) = weak_view.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Some(text) = reader.borrow_mut().read_new_lines() {
            append(&view, &text);
        }
        glib::ControlFlow::Continue
    });
    scroller
}

/// Keep the view scrolled to the newest lines at the bottom, unless the user scrolls up
/// to read something; getting back to the bottom resumes following.
///
/// Only user actions stop following (scrolling up with the wheel or touchpad, or
/// grabbing the scrollbar). The text view also moves the scroll position by itself while
/// it measures long text after it appears, and those moves must not count.
///
/// # Arguments
///
/// * `scroller` - The log view's scrolled window
fn follow_the_end(scroller: &gtk::ScrolledWindow) {
    let following = Rc::new(Cell::new(true));
    let adjustment = scroller.vadjustment();

    // Wheel or touchpad: scrolling up means the user wants to read older lines. The
    // controller runs before the view scrolls and lets the event through unchanged.
    let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let following = Rc::clone(&following);
        wheel.connect_scroll(move |_, _, dy| {
            if dy < 0.0 {
                following.set(false);
            }
            glib::Propagation::Proceed
        });
    }
    scroller.add_controller(wheel);

    // Grabbing the scrollbar also hands control to the user.
    let grab = gtk::GestureClick::new();
    grab.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let following = Rc::clone(&following);
        grab.connect_pressed(move |_, _, _, _| following.set(false));
    }
    scroller.vscrollbar().add_controller(grab);

    // Reaching the bottom again, by any means, resumes following.
    {
        let following = Rc::clone(&following);
        adjustment.connect_value_changed(move |adjustment| {
            if at_end(adjustment) {
                following.set(true);
            }
        });
    }
    // The content or the view size changed: jump to the end if following.
    adjustment.connect_changed(move |adjustment| {
        if following.get() {
            adjustment.set_value(adjustment.upper() - adjustment.page_size());
        }
    });
}

/// Whether the scroll position shows the last line.
///
/// # Arguments
///
/// * `adjustment` - Vertical scroll position
fn at_end(adjustment: &gtk::Adjustment) -> bool {
    adjustment.value() + adjustment.page_size() >= adjustment.upper() - 2.0
}

/// Add `text` to the view and trim the oldest lines beyond [`MAX_LINES`].
///
/// # Arguments
///
/// * `view` - The text view
/// * `text` - New complete lines
fn append(view: &gtk::TextView, text: &str) {
    let buffer = view.buffer();
    buffer.insert(&mut buffer.end_iter(), text);

    let excess = buffer.line_count() - MAX_LINES;
    if excess > 0
        && let Some(mut cut) = buffer.iter_at_line(excess)
    {
        buffer.delete(&mut buffer.start_iter(), &mut cut);
    }
}
