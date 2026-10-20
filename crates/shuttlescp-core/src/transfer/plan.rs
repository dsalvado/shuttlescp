//! Planning a job: walking its sources into a tree of items, and noting what already
//! exists at the destination.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::endpoint::{Endpoint, Node};
use super::{FileInfo, ItemKind, Job, JobKind, PARTIAL_SUFFIX, PlanItem};

/// Deepest folder nesting walked; deeper usually means a symbolic link loop.
const MAX_DEPTH: usize = 64;

/// What is in a destination folder, by name.
type Existing = HashMap<OsString, Node>;

/// Walk the job's sources into a tree of items.
///
/// Copies follow symbolic links (the copy gets what the link points to); deletes do not
/// (deleting a link removes the link, never what it points to). For copies, each
/// destination folder is listed once to find files already there and partial copies
/// left by an interrupted run.
///
/// # Arguments
///
/// * `job` - The job
/// * `src` - Where the sources are
/// * `dst` - Where copies go (unused when deleting)
///
/// # Errors
///
/// Returns a message if a source or one of its folders cannot be read. When resuming,
/// a missing source is passed over instead: a move already finished it.
pub(super) async fn plan(
    job: &Job,
    src: &Endpoint,
    dst: Option<&Endpoint>,
) -> Result<Vec<PlanItem>, String> {
    let follow = job.kind != JobKind::Delete;
    let dst = dst.filter(|_| job.kind != JobKind::Delete);
    let mut items: Vec<PlanItem> = Vec::new();
    let mut folders = Vec::new();

    let at_top = existing_in(dst, &job.dest_dir).await;
    for source in &job.sources {
        let name = source
            .file_name()
            .ok_or_else(|| format!("{} cannot be copied or deleted", source.display()))?;
        let node = match src.stat(source, follow).await {
            Ok(node) => node,
            Err(error) if job.resume => {
                tracing::info!("passing over {}: {error}", source.display());
                continue;
            }
            Err(error) => return Err(error),
        };
        let id = push_item(
            &mut items,
            None,
            name,
            &node,
            source.clone(),
            &job.dest_dir,
            &at_top,
        );
        if node.is_dir {
            folders.push((id, 0));
        }
    }

    while let Some((folder, depth)) = folders.pop() {
        if depth >= MAX_DEPTH {
            return Err(format!(
                "{} is nested too deeply (a symbolic link loop?)",
                items[folder].source.display()
            ));
        }
        let (source, dest) = (items[folder].source.clone(), items[folder].dest.clone());
        let existing = if items[folder].existing_is_dir {
            existing_in(dst, &dest).await
        } else {
            Existing::new()
        };
        let mut children = src.children(&source, follow).await?;
        // Folders first, then by name, like the file panes.
        children
            .sort_by(|(a_name, a), (b_name, b)| b.is_dir.cmp(&a.is_dir).then(a_name.cmp(b_name)));
        for (name, node) in children {
            let child = push_item(
                &mut items,
                Some(folder),
                &name,
                &node,
                source.join(&name),
                &dest,
                &existing,
            );
            if node.is_dir {
                folders.push((child, depth + 1));
            }
        }
    }

    // Children always come after their parent, so walking backwards adds each folder's
    // complete total into its parent.
    for id in (0..items.len()).rev() {
        if let Some(parent) = items[id].parent {
            items[parent].size += items[id].size;
        }
    }
    Ok(items)
}

/// List a destination folder; a folder that does not exist (yet) holds nothing.
async fn existing_in(dst: Option<&Endpoint>, dir: &Path) -> Existing {
    let Some(dst) = dst else {
        return Existing::new();
    };
    dst.children(dir, false)
        .await
        .map(|children| children.into_iter().collect())
        .unwrap_or_default()
}

/// Add an item to the plan.
///
/// # Arguments
///
/// * `existing` - What is in the destination folder, to spot files already there
///
/// # Returns
///
/// The new item's id
fn push_item(
    items: &mut Vec<PlanItem>,
    parent: Option<usize>,
    name: &std::ffi::OsStr,
    node: &Node,
    source: PathBuf,
    dest_dir: &Path,
    existing: &Existing,
) -> usize {
    let id = items.len();
    let there = existing.get(name);
    let mut partial_name = name.to_os_string();
    partial_name.push(PARTIAL_SUFFIX);
    let partial = existing
        .get(&partial_name)
        .filter(|partial| !node.is_dir && !partial.is_dir)
        .map_or(0, |partial| partial.size);
    items.push(PlanItem {
        id,
        parent,
        name: name.to_string_lossy().into_owned(),
        kind: if node.is_dir {
            ItemKind::Folder
        } else {
            ItemKind::File
        },
        // A folder's total is added up once the whole tree is known.
        size: if node.is_dir { 0 } else { node.size },
        source,
        dest: dest_dir.join(name),
        existing: there.filter(|there| !there.is_dir).map(|there| FileInfo {
            size: there.size,
            modified: there.modified,
        }),
        modified: node.modified,
        existing_is_dir: there.is_some_and(|there| there.is_dir),
        partial,
    });
    id
}
