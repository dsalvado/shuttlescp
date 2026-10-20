//! A live view of the end of the log file, like `tail -f`, for the bottom panel. Each
//! line's level is coloured (errors in bold red stand out most), error and warning
//! lines are tinted, and lines alternate between two shades for easier reading.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::{gdk, glib};

use guiscp_core::log_reader::{LogReader, line_level};
use guiscp_core::logging::LogLevel;

/// How often the file is checked for new lines.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Lines kept in the view; older ones are dropped.
const MAX_LINES: i32 = 2000;
/// Name of the tag shading every other line.
const STRIPE: &str = "stripe";

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

    add_tags(&view.buffer());
    // Lines added so far, to alternate the shading; counted rather than read from the
    // buffer, so trimming old lines does not swap the shades of the rest.
    let lines = Rc::new(Cell::new(0_u64));
    let reader = Rc::new(RefCell::new(LogReader::new(path)));
    // Show what is already there right away instead of after the first interval.
    if let Some(text) = reader.borrow_mut().read_new_lines() {
        append(&view, &text, &lines);
    }
    let weak_view = view.downgrade();
    glib::timeout_add_local(POLL_INTERVAL, move || {
        // Stop polling once the view is gone (window closed).
        let Some(view) = weak_view.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Some(text) = reader.borrow_mut().read_new_lines() {
            append(&view, &text, &lines);
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

/// Create the text styles: the alternate-line shade, the error and warning line tints,
/// and a colour for each level's word. Colours are translucent or mid-tone so they read
/// on light and dark themes alike.
fn add_tags(buffer: &gtk::TextBuffer) {
    let rgba = |red, green, blue, alpha| gdk::RGBA::new(red, green, blue, alpha);
    let table = buffer.tag_table();
    // Added in increasing priority: a tinted line's colour wins over the shade.
    let line_tags = [
        (STRIPE, rgba(0.5, 0.5, 0.5, 0.07)),
        (line_tag(LogLevel::Warn), rgba(0.9, 0.6, 0.0, 0.10)),
        (line_tag(LogLevel::Error), rgba(0.88, 0.11, 0.14, 0.14)),
    ];
    for (name, background) in line_tags {
        table.add(
            &gtk::TextTag::builder()
                .name(name)
                .paragraph_background_rgba(&background)
                .build(),
        );
    }
    let level_tags = [
        (LogLevel::Error, rgba(0.88, 0.11, 0.14, 1.0), true),
        (LogLevel::Warn, rgba(0.85, 0.5, 0.0, 1.0), true),
        (LogLevel::Info, rgba(0.15, 0.6, 0.35, 1.0), false),
        (LogLevel::Debug, rgba(0.5, 0.5, 0.5, 1.0), false),
        (LogLevel::Trace, rgba(0.5, 0.5, 0.5, 0.7), false),
    ];
    for (level, colour, bold) in level_tags {
        let tag = gtk::TextTag::builder()
            .name(level_tag(level))
            .foreground_rgba(&colour)
            .build();
        if bold {
            tag.set_weight(700);
        }
        table.add(&tag);
    }
}

/// Name of the tag colouring a level's word.
fn level_tag(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "level-error",
        LogLevel::Warn => "level-warn",
        LogLevel::Info => "level-info",
        LogLevel::Debug => "level-debug",
        LogLevel::Trace => "level-trace",
    }
}

/// Name of the tag tinting a whole line of this level (only errors and warnings have
/// one; the others are never looked up).
fn line_tag(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "line-error",
        _ => "line-warn",
    }
}

/// Add `text` to the view, styled line by line, and trim the oldest lines beyond
/// [`MAX_LINES`].
///
/// # Arguments
///
/// * `view` - The text view
/// * `text` - New complete lines
/// * `lines` - Lines added so far (updated)
fn append(view: &gtk::TextView, text: &str, lines: &Cell<u64>) {
    let buffer = view.buffer();
    for line in text.split_inclusive('\n') {
        let start = buffer.end_iter().offset();
        buffer.insert(&mut buffer.end_iter(), line);
        let at = |offset: i32| buffer.iter_at_offset(offset);
        let end = buffer.end_iter().offset();
        if lines.get() % 2 == 1 {
            buffer.apply_tag_by_name(STRIPE, &at(start), &at(end));
        }
        lines.set(lines.get() + 1);
        if let Some((level, word)) = line_level(line) {
            // Tags count in characters, the parser in bytes.
            let chars = |bytes: usize| i32::try_from(line[..bytes].chars().count()).unwrap_or(0);
            buffer.apply_tag_by_name(
                level_tag(level),
                &at(start + chars(word.start)),
                &at(start + chars(word.end)),
            );
            if matches!(level, LogLevel::Error | LogLevel::Warn) {
                buffer.apply_tag_by_name(line_tag(level), &at(start), &at(end));
            }
        }
    }

    let excess = buffer.line_count() - MAX_LINES;
    if excess > 0
        && let Some(mut cut) = buffer.iter_at_line(excess)
    {
        buffer.delete(&mut buffer.start_iter(), &mut cut);
    }
}
