//! Running a permissions job: changing the planned items one by one, each logged with
//! its permissions before and after.

use crate::format::format_owner;
use crate::permissions::{self, PermissionsJob};

use super::endpoint::Endpoint;
use super::{CancelFlag, ItemKind, ItemOutcome, PlanItem, Sink, TransferEvent};

/// Change every planned item as `job` says: contents before the folders holding them,
/// so taking a right away from a folder cannot stop the job reaching what is inside.
/// Symbolic links are left alone (changing one would change what it points to, which
/// may be anywhere), as `chmod -R` does.
///
/// # Returns
///
/// Each item's outcome, by id
pub(super) async fn change_items(
    job: &PermissionsJob,
    items: &[PlanItem],
    src: &Endpoint,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Vec<ItemOutcome> {
    let remote = match src {
        Endpoint::Local => None,
        Endpoint::Remote(remote) => Some(remote),
    };
    let run_as = job.run_as();
    let who = if run_as.is_sudo() {
        " (as root, sudo)"
    } else {
        ""
    };
    let mut outcomes = vec![ItemOutcome::Cancelled; items.len()];
    for item in items.iter().rev() {
        let edit = job.edit_for(item.kind == ItemKind::Folder, item.mode);
        let outcome = if cancel.is_cancelled() {
            ItemOutcome::Cancelled
        } else if cancel.is_item_stopped(item.id) {
            ItemOutcome::Stopped
        } else if item.is_link {
            tracing::info!("not changed (symbolic link): {}", item.source);
            ItemOutcome::Skipped
        } else if let Some(edit) = edit {
            match permissions::apply(remote, &item.source, item.mode, &edit, &run_as).await {
                Ok(()) => {
                    // The log keeps every change: what it was, what it is now.
                    let mode = |mode: Option<u32>| {
                        mode.map_or_else(|| "?".to_owned(), |mode| format!("{mode:04o}"))
                    };
                    tracing::info!(
                        "changed {}{who}: permissions {} -> {}{}",
                        item.source,
                        mode(item.mode),
                        mode(edit.mode_after(item.mode)),
                        if edit.owner.is_some() || edit.group.is_some() {
                            format!(
                                ", owner -> {}",
                                format_owner(edit.owner.as_deref(), edit.group.as_deref())
                            )
                        } else {
                            String::new()
                        }
                    );
                    ItemOutcome::Done
                }
                Err(error) => {
                    tracing::warn!("{}", error.message);
                    ItemOutcome::Failed(error.message)
                }
            }
        } else {
            // Outside the scope (a file in a "folders only" job, say).
            ItemOutcome::Skipped
        };
        sink(TransferEvent::ItemFinished {
            id: item.id,
            outcome: outcome.clone(),
        });
        outcomes[item.id] = outcome;
    }
    outcomes
}
