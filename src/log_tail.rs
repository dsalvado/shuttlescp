//! A live view of the end of the log file, like `tail -f`, for the bottom panel.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

/// How often the file is checked for new lines.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// How much of the end of an existing file is shown when the view opens.
const INITIAL_TAIL_BYTES: u64 = 64 * 1024;
/// Lines kept in the view; older ones are dropped.
const MAX_LINES: i32 = 2000;

/// Where the reader has got to in the file.
struct TailState {
    path: PathBuf,
    /// Bytes of the file already shown.
    offset: u64,
    /// End of the last read was mid-line: wait for the rest before showing it.
    partial: Vec<u8>,
}

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

    let state = Rc::new(RefCell::new(TailState {
        offset: initial_offset(&path),
        path,
        partial: Vec::new(),
    }));
    // Show what is already there right away instead of after the first interval.
    if let Some(text) = read_new_lines(&mut state.borrow_mut()) {
        append(&view, &text);
    }
    let weak_view = view.downgrade();
    glib::timeout_add_local(POLL_INTERVAL, move || {
        // Stop polling once the view is gone (window closed).
        let Some(view) = weak_view.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Some(text) = read_new_lines(&mut state.borrow_mut()) {
            append(&view, &text);
        }
        glib::ControlFlow::Continue
    });
    scroller
}

/// Where to start reading: the last [`INITIAL_TAIL_BYTES`] of the file.
///
/// # Arguments
///
/// * `path` - Log file
fn initial_offset(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map_or(0, |meta| meta.len().saturating_sub(INITIAL_TAIL_BYTES))
}

/// Read whatever was added to the file since the last call.
///
/// # Arguments
///
/// * `state` - Reader position, updated in place
///
/// # Returns
///
/// Complete new lines (ending in a newline), or `None` if there is nothing new
fn read_new_lines(state: &mut TailState) -> Option<String> {
    let mut file = File::open(&state.path).ok()?;
    let length = file.metadata().ok()?.len();
    if length < state.offset {
        // The file was cleared or rotated: start again from its beginning.
        state.offset = 0;
        state.partial.clear();
    }
    if length == state.offset {
        return None;
    }
    file.seek(SeekFrom::Start(state.offset)).ok()?;
    let mut bytes = std::mem::take(&mut state.partial);
    let read = file.read_to_end(&mut bytes).ok()?;
    // `read_to_end` returns a `usize`; on 64-bit Linux it always fits a `u64`.
    state.offset += u64::try_from(read).unwrap_or(0);
    complete_lines(&mut bytes, &mut state.partial)
}

/// Split off the complete lines of `bytes`, keeping any unfinished last line in
/// `partial` for next time.
///
/// # Arguments
///
/// * `bytes` - Newly read data (consumed)
/// * `partial` - Receives the trailing incomplete line, if any
///
/// # Returns
///
/// The complete lines as text, or `None` if there are none yet
fn complete_lines(bytes: &mut Vec<u8>, partial: &mut Vec<u8>) -> Option<String> {
    let end = bytes.iter().rposition(|&byte| byte == b'\n')? + 1;
    *partial = bytes.split_off(end);
    Some(String::from_utf8_lossy(bytes).into_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_lines_holds_back_an_unfinished_line() {
        let mut bytes = b"one\ntwo\nthr".to_vec();
        let mut partial = Vec::new();

        let lines = complete_lines(&mut bytes, &mut partial);

        assert_eq!(lines.as_deref(), Some("one\ntwo\n"));
        assert_eq!(partial, b"thr");
    }

    #[test]
    fn complete_lines_waits_when_no_line_is_finished() {
        let mut bytes = b"no newline yet".to_vec();
        let mut partial = Vec::new();

        assert_eq!(complete_lines(&mut bytes, &mut partial), None);
    }

    #[test]
    fn read_new_lines_follows_growth_and_restarts_after_truncation() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("app.log");
        std::fs::write(&path, "first\n").expect("write log");
        let mut state = TailState {
            path: path.clone(),
            offset: 0,
            partial: Vec::new(),
        };

        // Act + Assert: initial content, then nothing new, then appended lines.
        assert_eq!(read_new_lines(&mut state).as_deref(), Some("first\n"));
        assert_eq!(read_new_lines(&mut state), None);
        std::fs::write(&path, "first\nsecond\n").expect("append");
        assert_eq!(read_new_lines(&mut state).as_deref(), Some("second\n"));

        // The file is cleared and rewritten: reading restarts from the top.
        std::fs::write(&path, "new\n").expect("truncate");
        assert_eq!(read_new_lines(&mut state).as_deref(), Some("new\n"));
    }
}
