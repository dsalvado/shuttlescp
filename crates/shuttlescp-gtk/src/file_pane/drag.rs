//! Dragging files and folders: onto a folder in the same pane, or into the other pane.
//!
//! Every cell of the file list is a drag source: dragging any cell of a row drags the
//! selected rows. Folder rows and ".." are drop targets for their folder. The list
//! itself takes drops from the other pane anywhere else, into the folder it shows.
//! Either way the drop is handed to the pane, which asks whether to copy or move.

use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use relm4::gtk::prelude::*;
use relm4::gtk::{self, gdk, glib, graphene};

use shuttlescp_core::entry::EntryKind;
use shuttlescp_core::path::{RemotePath, SidePath};

use super::{ACTION_GROUP, Dropped, Row};

/// Action a drop on a folder row triggers, with the drop as a [`DropVariant`].
pub(super) const DROP_ACTION: &str = "drop";
/// CSS class marking the row a drag would drop into (see style.css).
const DROP_ROW_CLASS: &str = "drop-row";

/// A drop as passed through [`DROP_ACTION`]: (sources, dragged from this pane,
/// destination folder, x, y in the file list). Paths travel as bytes (see
/// [`side_path_bytes`]), since local ones need not be valid UTF-8.
pub(super) type DropVariant = (Vec<Vec<u8>>, bool, Vec<u8>, f64, f64);

/// What is being dragged; carried by the drag inside a `glib::BoxedAnyObject`, so only
/// ShuttleSCP's own panes accept it.
#[derive(Clone)]
struct DragPayload {
    /// The file list it was dragged from, to tell a drop within one pane from a drop
    /// into the other.
    view: glib::WeakRef<gtk::ColumnView>,
    /// The dragged files and folders.
    paths: Vec<SidePath>,
}

/// Make dragging a cell drag the selected rows (or just the cell's row, if it is not
/// selected). The ".." row cannot be dragged.
///
/// # Arguments
///
/// * `cell` - The cell's root widget
/// * `item` - The list item the cell belongs to
pub(super) fn attach_drag_source(cell: &impl IsA<gtk::Widget>, item: &gtk::ListItem) {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::COPY)
        .build();
    let item = item.downgrade();
    let cell_widget = cell.clone().upcast::<gtk::Widget>();
    source.connect_prepare(move |source, x, y| {
        let item = item.upgrade()?;
        if with_row(&item, |row| matches!(row, Row::Parent { .. }))? {
            return None;
        }
        let view = list_of(&cell_widget)?;
        let selection = view.model()?;
        // Dragging a row outside the selection drags just that row, as file managers do.
        if !selection.is_selected(item.position()) {
            selection.select_item(item.position(), true);
        }
        let paths: Vec<SidePath> = (0..selection.n_items())
            .filter(|&position| selection.is_selected(position))
            .filter_map(|position| selection.item(position))
            .filter_map(|object| entry_path(&object))
            .collect();
        if paths.is_empty() {
            return None;
        }
        // The picture dragged along is the row, held where the pointer took it.
        if let Some(row) = row_widget(&cell_widget) {
            let spot = cell_widget
                .compute_point(&row, &graphene::Point::new(x as f32, y as f32))
                .unwrap_or_else(|| graphene::Point::new(x as f32, y as f32));
            source.set_icon(
                Some(&gtk::WidgetPaintable::new(Some(&row))),
                spot.x() as i32,
                spot.y() as i32,
            );
        }
        tracing::debug!("dragging {} items", paths.len());
        let payload = DragPayload {
            view: view.downgrade(),
            paths,
        };
        Some(gdk::ContentProvider::for_value(
            &glib::BoxedAnyObject::new(payload).to_value(),
        ))
    });
    cell.add_controller(source);
}

/// Make a cell take drops into its row's folder, if the row is a folder or "..".
///
/// A file row, or a folder that is itself being dragged, turns the drop down, so it
/// falls through to the list (see [`attach_list_drop`]).
///
/// # Arguments
///
/// * `cell` - The cell's root widget
/// * `item` - The list item the cell belongs to
pub(super) fn attach_folder_drop(cell: &impl IsA<gtk::Widget>, item: &gtk::ListItem) {
    let target = gtk::DropTarget::new(glib::BoxedAnyObject::static_type(), gdk::DragAction::COPY);
    let cell_widget = cell.clone().upcast::<gtk::Widget>();
    let item = item.downgrade();
    let folder = move || {
        item.upgrade()
            .and_then(|item| with_row(&item, folder_of).flatten())
    };
    {
        let folder = folder.clone();
        target.connect_accept(move |_, drop| {
            let (Some(folder), Some(payload)) = (folder(), dragged(drop)) else {
                return false;
            };
            !payload.paths.contains(&folder)
        });
    }
    {
        let cell = cell_widget.clone();
        target.connect_enter(move |_, _, _| {
            if let Some(row) = row_widget(&cell) {
                row.add_css_class(DROP_ROW_CLASS);
            }
            gdk::DragAction::COPY
        });
    }
    {
        let cell = cell_widget.clone();
        target.connect_leave(move |_| {
            if let Some(row) = row_widget(&cell) {
                row.remove_css_class(DROP_ROW_CLASS);
            }
        });
    }
    target.connect_drop(move |_, value, x, y| {
        if let Some(row) = row_widget(&cell_widget) {
            row.remove_css_class(DROP_ROW_CLASS);
        }
        let (Some(payload), Some(folder), Some(view)) =
            (payload_in(value), folder(), list_of(&cell_widget))
        else {
            return false;
        };
        let from_this_pane = payload.view.upgrade().as_ref() == Some(&view);
        let point = cell_widget
            .compute_point(&view, &graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| graphene::Point::new(x as f32, y as f32));
        let parameter: DropVariant = (
            payload.paths.iter().map(side_path_bytes).collect(),
            from_this_pane,
            side_path_bytes(&folder),
            f64::from(point.x()),
            f64::from(point.y()),
        );
        cell_widget
            .activate_action(
                &format!("{ACTION_GROUP}.{DROP_ACTION}"),
                Some(&parameter.to_variant()),
            )
            .is_ok()
    });
    cell.add_controller(target);
}

/// Make the file list take drops from the other pane, into the folder it shows. Drops
/// from its own pane are turned down: they only make sense on a folder row.
///
/// # Arguments
///
/// * `view` - The file list
/// * `on_drop` - Receives each drop
pub(super) fn attach_list_drop(view: &gtk::ColumnView, on_drop: impl Fn(Dropped) + 'static) {
    let target = gtk::DropTarget::new(glib::BoxedAnyObject::static_type(), gdk::DragAction::COPY);
    let list = view.downgrade();
    target.connect_accept(move |_, drop| {
        dragged(drop).is_some_and(|payload| payload.view.upgrade() != list.upgrade())
    });
    target.connect_drop(move |_, value, x, y| {
        let Some(payload) = payload_in(value) else {
            return false;
        };
        on_drop(Dropped {
            sources: payload.paths,
            from_this_pane: false,
            dest_dir: None,
            x,
            y,
        });
        true
    });
    view.add_controller(target);
}

/// Read a drop passed through [`DROP_ACTION`].
pub(super) fn dropped_from(parameter: &glib::Variant) -> Option<Dropped> {
    let (sources, from_this_pane, dest_dir, x, y) = parameter.get::<DropVariant>()?;
    Some(Dropped {
        sources: sources
            .into_iter()
            .map(bytes_side_path)
            .collect::<Option<_>>()?,
        from_this_pane,
        dest_dir: Some(bytes_side_path(dest_dir)?),
        x,
        y,
    })
}

/// What a drag in progress carries, if it comes from a ShuttleSCP pane. Known before the
/// drop, since the drag started in this app.
fn dragged(drop: &gdk::Drop) -> Option<DragPayload> {
    let value = drop
        .drag()?
        .content()
        .value(glib::BoxedAnyObject::static_type())
        .ok()?;
    payload_in(&value)
}

/// The payload inside a dropped value.
fn payload_in(value: &glib::Value) -> Option<DragPayload> {
    let boxed = value.get::<glib::BoxedAnyObject>().ok()?;
    let payload = boxed.try_borrow::<DragPayload>().ok()?.clone();
    Some(payload)
}

/// Run `f` on the row a list item shows.
fn with_row<R>(item: &gtk::ListItem, f: impl FnOnce(&Row) -> R) -> Option<R> {
    let boxed = item.item().and_downcast::<glib::BoxedAnyObject>()?;
    let row = boxed.try_borrow::<Row>().ok()?;
    Some(f(&row))
}

/// Full path of a list model item, if it is a file or folder (not "..").
fn entry_path(object: &glib::Object) -> Option<SidePath> {
    let boxed = object.downcast_ref::<glib::BoxedAnyObject>()?;
    let row = boxed.try_borrow::<Row>().ok()?;
    match &*row {
        Row::Entry { path, .. } => Some(path.clone()),
        Row::Parent { .. } => None,
    }
}

/// The folder a drop on this row goes into: the row's own folder, or the parent folder
/// for ".."; `None` for a file.
fn folder_of(row: &Row) -> Option<SidePath> {
    match row {
        Row::Parent { path } => Some(path.clone()),
        Row::Entry { entry, path, .. } => match entry.kind() {
            EntryKind::Directory => Some(path.clone()),
            EntryKind::File => None,
        },
    }
}

/// The file list a cell is in.
fn list_of(cell: &gtk::Widget) -> Option<gtk::ColumnView> {
    cell.ancestor(gtk::ColumnView::static_type()).and_downcast()
}

/// The whole row a cell is in: the cell's root sits in a cell widget, inside the row.
fn row_widget(cell: &gtk::Widget) -> Option<gtk::Widget> {
    cell.parent()?.parent()
}

/// Tag byte of a local path in [`side_path_bytes`].
const LOCAL_TAG: u8 = 0;
/// Tag byte of a server path in [`side_path_bytes`].
const REMOTE_TAG: u8 = 1;

/// A path on either side as bytes, for a variant: a tag byte for the side, then the
/// path (a server path as UTF-8, a local one as [`path_bytes`] makes it).
fn side_path_bytes(path: &SidePath) -> Vec<u8> {
    let (tag, bytes) = match path {
        SidePath::Local(path) => (LOCAL_TAG, path_bytes(path)),
        SidePath::Remote(path) => (REMOTE_TAG, path.as_str().as_bytes().to_vec()),
    };
    let mut tagged = Vec::with_capacity(bytes.len() + 1);
    tagged.push(tag);
    tagged.extend(bytes);
    tagged
}

/// A path from [`side_path_bytes`]'s bytes; `None` if they are not such bytes.
fn bytes_side_path(bytes: Vec<u8>) -> Option<SidePath> {
    let (&tag, rest) = bytes.split_first()?;
    match tag {
        LOCAL_TAG => Some(SidePath::Local(bytes_path(rest.to_vec()))),
        REMOTE_TAG => Some(SidePath::Remote(RemotePath::new(
            String::from_utf8(rest.to_vec()).ok()?,
        ))),
        _ => None,
    }
}

/// A path as bytes, for a variant.
#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

/// A path as bytes, for a variant: each 16-bit unit of the Windows path as two
/// little-endian bytes, so paths that are not valid UTF-16 survive too.
#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// A path from [`path_bytes`]'s bytes.
#[cfg(unix)]
fn bytes_path(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}

/// A path from [`path_bytes`]'s bytes.
#[cfg(windows)]
fn bytes_path(bytes: Vec<u8>) -> PathBuf {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    PathBuf::from(OsString::from_wide(&units))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn paths_survive_the_trip_through_bytes() {
        let odd = PathBuf::from(OsString::from_vec(b"/data/caf\xe9 \xff.txt".to_vec()));

        assert_eq!(bytes_path(path_bytes(&odd)), odd);
    }

    #[cfg(windows)]
    #[test]
    fn paths_survive_the_trip_through_bytes() {
        // A lone surrogate (0xD800) is not valid UTF-16, but Windows allows it in names.
        let mut units: Vec<u16> = r"C:\data\café ".encode_utf16().collect();
        units.push(0xD800);
        units.extend(".txt".encode_utf16());
        let odd = PathBuf::from(OsString::from_wide(&units));

        assert_eq!(bytes_path(path_bytes(&odd)), odd);
    }

    #[test]
    fn paths_keep_their_side_through_bytes() {
        let local = SidePath::Local(PathBuf::from("/srv/a"));
        let server = SidePath::Remote(RemotePath::new("/srv/a"));

        assert_eq!(bytes_side_path(side_path_bytes(&local)), Some(local));
        assert_eq!(bytes_side_path(side_path_bytes(&server)), Some(server));
        assert_eq!(bytes_side_path(Vec::new()), None);
    }
}
