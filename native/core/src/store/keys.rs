//! Every `native_meta` key, and the `session_meta` keys more than one module
//! reads, built in one place. `native_meta` is a small key/value table that
//! also holds JSON documents (compare records, indexes, scoreboards); a key's
//! spelling is part of the on-disk format, so change none of these without a
//! migration.
use std::path::Path;

/// `native_meta`: the picker id last used in a project, for new
/// conversations there.
pub fn execution_target(workspace: &Path) -> String {
    format!("execution_target:{}", workspace.display())
}
/// `native_meta`: the last local GGUF model used in a project, preferred
/// when a subscription runs out and the task continues locally.
pub fn last_local_target(workspace: &Path) -> String {
    format!("last_local_target:{}", workspace.display())
}
/// `native_meta`: one compare record (JSON `compare::Record`).
pub fn compare_record(id: &str) -> String {
    format!("compare:{id}")
}
/// `native_meta`: a project's compare ids, newest first (JSON list).
pub fn compare_index(workspace: &Path) -> String {
    format!("compare_index:{}", workspace.display())
}
/// `native_meta`: a project's per-model compare wins and runs (JSON list).
pub fn compare_scoreboard(workspace: &Path) -> String {
    format!("compare_scoreboard:{}", workspace.display())
}
/// `native_meta`: a rewind that can be undone (JSON `review::Rewind`); the
/// files as they were before it are checkpoint rows of task `rewind:<id>`.
pub fn rewind_undo(id: &str) -> String {
    format!("rewind_undo:{id}")
}
/// `native_meta`: set once `goals.db` from before 0.28 was imported.
pub const LEGACY_GOALS_IMPORTED: &str = "legacy_goals_imported";
/// `native_meta`: set once the pre-0.28 background process list was imported.
pub const LEGACY_BACKGROUND_IMPORTED: &str = "legacy_background_imported";

/// `session_meta`: the picker id a conversation runs its next turn on.
pub const EXECUTION_TARGET: &str = "execution_target";
/// `session_meta`: the compare a lane conversation belongs to.
pub const COMPARE_ID: &str = "compare_id";
/// `session_meta`: the model id of a compare lane conversation.
pub const COMPARE_LANE: &str = "compare_lane";
/// `session_meta` prefix: a vendor CLI's own session/thread id, per vendor.
pub const NATIVE_SESSION_PREFIX: &str = "native_session:";
/// `session_meta`: a vendor CLI's own session/thread id for this conversation.
pub fn native_session(vendor: &str) -> String {
    format!("{NATIVE_SESSION_PREFIX}{vendor}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spellings are the on-disk format.
    #[test]
    fn keys_keep_their_stored_spelling() {
        let project = Path::new("/home/u/project");
        assert_eq!(
            execution_target(project),
            "execution_target:/home/u/project"
        );
        assert_eq!(
            last_local_target(project),
            "last_local_target:/home/u/project"
        );
        assert_eq!(compare_record("ab12"), "compare:ab12");
        assert_eq!(compare_index(project), "compare_index:/home/u/project");
        assert_eq!(
            compare_scoreboard(project),
            "compare_scoreboard:/home/u/project"
        );
        assert_eq!(native_session("codex"), "native_session:codex");
        assert_eq!(rewind_undo("ab12"), "rewind_undo:ab12");
    }
}
