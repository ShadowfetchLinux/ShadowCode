import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { api, type Job, type SessionDetail, type WorktreeTask } from "../api";
import type { ToastKind } from "./useToasts";

export type WorktreeAction = "apply" | "keep-branch" | "discard";

/** Is the worktree task still open (its worktree exists)? */
export const isOpenTask = (task: WorktreeTask | null | undefined) =>
  Boolean(task && ["starting", "running", "done"].includes(task.state));

/** The worktree task of the open conversation ("Run in new worktree"): its
 * state, changed files and the Apply / Keep as branch / Discard actions.
 * Closing it moves the conversation back to its project, so the window
 * reopens it there. */
export function useWorktreeTask(ctx: {
  sessionId: string;
  selectedRef: RefObject<string>;
  jobs: Job[];
  openSession: (id: string) => Promise<void>;
  refresh: () => Promise<void>;
  toast: (text: string, kind?: ToastKind) => void;
}) {
  const [task, setTask] = useState<WorktreeTask | null>(null);
  const [acting, setActing] = useState<WorktreeAction | "">("");
  const context = useRef(ctx);
  context.current = ctx;

  const reload = useCallback(async (id: string) => {
    try {
      const fresh = await api.worktreeTask(id);
      if (context.current.selectedRef.current === fresh.session_id)
        setTask(isOpenTask(fresh) ? fresh : null);
    } catch {
      /* The last known state stays. */
    }
  }, []);

  /** The navigation opened a conversation. */
  const track = useCallback(
    (id: string, detail: Pick<SessionDetail, "worktree">) => {
      const known = detail.worktree;
      setTask(isOpenTask(known) ? known! : null);
      if (known && isOpenTask(known) && known.session_id === id)
        void reload(known.id);
    },
    [reload],
  );

  // Its turns finishing (or starting) change its state and files.
  const statusKey = ctx.jobs
    .filter((job) => job.session_id === ctx.sessionId)
    .map((job) => `${job.id}:${job.status}`)
    .join(",");
  const taskId = task?.id || "";
  useEffect(() => {
    if (taskId) void reload(taskId);
  }, [statusKey, taskId, reload]);

  async function act(action: WorktreeAction) {
    if (!task || acting) return;
    setActing(action);
    const { toast, openSession, refresh } = context.current;
    try {
      const result = await api.closeWorktreeTask(task.id, action);
      if (isOpenTask(result)) {
        setTask(result);
        if (result.conflicts.length)
          toast(
            `Nothing was applied: ${result.conflicts.join(", ")} changed in the project since this task started.`,
            "err",
          );
        return;
      }
      setTask(null);
      toast(
        result.state === "applied"
          ? result.applied_files.length
            ? `Applied ${result.applied_files.length} file${result.applied_files.length === 1 ? "" : "s"} to the project. Review them in Changes.`
            : "Nothing to apply; the worktree was removed."
          : result.state === "branch"
            ? `Kept on branch ${result.kept_branch}. The worktree was removed.`
            : "Discarded. The worktree and its branch were removed.",
        "ok",
      );
      if (context.current.selectedRef.current === result.session_id)
        await openSession(result.session_id);
      await refresh().catch(() => undefined);
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setActing("");
    }
  }

  return { task, acting, track, act, reload };
}
