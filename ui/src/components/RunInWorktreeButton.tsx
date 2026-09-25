import { GitBranchPlus } from "lucide-react";

/** Next to Send: start this message as a new conversation in a fresh
 * worktree of the project (Ctrl+Shift+Enter), so it runs beside a task in
 * the main checkout instead of waiting in the queue. */
export function RunInWorktreeButton({
  reason,
  enabled,
  queueing,
  onRun,
}: {
  /** Why it cannot be used here (hidden entirely when set). */
  reason: string | null;
  enabled: boolean;
  /** Another task is running here: Send would queue. */
  queueing: boolean;
  onRun: () => void;
}) {
  if (reason) return null;
  const label = queueing
    ? "Run now in a new worktree instead of queueing"
    : "Run in a new worktree";
  return (
    <button
      type="button"
      className={`worktree-btn${queueing ? " emphasis" : ""}`}
      aria-label={label}
      title={`${label} (Ctrl+Shift+Enter). It starts from the project's current files, including uncommitted work; you apply, keep or discard the result.`}
      aria-disabled={!enabled}
      onClick={() => {
        if (enabled) onRun();
      }}
    >
      <GitBranchPlus size={15} aria-hidden="true" />
      <span className="worktree-btn-text">
        {queueing ? "Run now in worktree" : "Worktree"}
      </span>
    </button>
  );
}
