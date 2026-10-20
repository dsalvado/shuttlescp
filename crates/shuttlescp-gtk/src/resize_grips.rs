//! Invisible grab areas in the window corners that start an interactive resize.
//!
//! GTK already offers resizing from the window's drop shadow, but that area sits outside
//! the visible window and depends on the theme and compositor. These grips make the
//! visible corners themselves resize handles.

use relm4::gtk;
use relm4::gtk::gdk;
use relm4::gtk::prelude::*;

/// Edge length of each square grip, in pixels.
const GRIP_SIZE: i32 = 16;

/// The four corners: where the grip sits, which edge it drags, and its cursor name.
const CORNERS: [(gtk::Align, gtk::Align, gdk::SurfaceEdge, &str); 4] = [
    (
        gtk::Align::Start,
        gtk::Align::Start,
        gdk::SurfaceEdge::NorthWest,
        "nw-resize",
    ),
    (
        gtk::Align::End,
        gtk::Align::Start,
        gdk::SurfaceEdge::NorthEast,
        "ne-resize",
    ),
    (
        gtk::Align::Start,
        gtk::Align::End,
        gdk::SurfaceEdge::SouthWest,
        "sw-resize",
    ),
    (
        gtk::Align::End,
        gtk::Align::End,
        gdk::SurfaceEdge::SouthEast,
        "se-resize",
    ),
];

/// Add a resize grip to each corner of `overlay`, which must fill `window`.
///
/// Grips hide themselves while the window is maximized, where resizing makes no sense.
///
/// # Arguments
///
/// * `window` - Window to resize
/// * `overlay` - Overlay covering the whole window content, including the title bar
pub fn add_corner_grips(window: &gtk::ApplicationWindow, overlay: &gtk::Overlay) {
    for (halign, valign, edge, cursor) in CORNERS {
        let grip = gtk::Box::builder()
            .halign(halign)
            .valign(valign)
            .width_request(GRIP_SIZE)
            .height_request(GRIP_SIZE)
            .build();
        grip.set_cursor_from_name(Some(cursor));

        let click = gtk::GestureClick::new();
        click.connect_pressed(move |gesture, _presses, x, y| begin_resize(gesture, edge, x, y));
        grip.add_controller(click);

        // Keep `visible` equal to "not maximized" for the window's whole lifetime.
        window
            .bind_property("maximized", &grip, "visible")
            .invert_boolean()
            .sync_create()
            .build();
        overlay.add_overlay(&grip);
    }
}

/// Hand the pointer over to the compositor to resize the window from `edge`.
///
/// # Arguments
///
/// * `gesture` - The click gesture that was pressed on a grip
/// * `edge` - Window edge or corner being dragged
/// * `x` - Press position, relative to the grip
/// * `y` - Press position, relative to the grip
fn begin_resize(gesture: &gtk::GestureClick, edge: gdk::SurfaceEdge, x: f64, y: f64) {
    let Some(grip) = gesture.widget() else {
        return;
    };
    let Some(native) = grip.native() else {
        return;
    };
    let Some(toplevel) = native.surface().and_downcast::<gdk::Toplevel>() else {
        return;
    };
    // The compositor needs surface coordinates: convert from the grip to the window
    // widget, then add the offset of the window inside its surface (the shadow margin).
    let Some(point) = grip.compute_point(&native, &gtk::graphene::Point::new(x as f32, y as f32))
    else {
        return;
    };
    let (offset_x, offset_y) = native.surface_transform();
    // `as i32` is safe: GDK button numbers are small (1 = left, 2 = middle, 3 = right).
    let button = gesture.current_button() as i32;
    toplevel.begin_resize(
        edge,
        gesture.current_event_device().as_ref(),
        button,
        f64::from(point.x()) + offset_x,
        f64::from(point.y()) + offset_y,
        gesture.current_event_time(),
    );
    // The compositor now owns the drag; stop GTK from also treating it as a click.
    gesture.set_state(gtk::EventSequenceState::Claimed);
}
