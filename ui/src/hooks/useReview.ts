import { useCallback, useEffect, useState } from "react";
import { api, type ReviewFile, type ReviewFileDetail } from "../api";
import { readStore, writeStore } from "../lib/storage";
import type { ToastKind } from "./useToasts";

/** Which task the full-width Review view shows, if any. */
export function useReviewTarget(sessionId: string) {
  const [target, setTarget] = useState<{ taskId: string; path?: string } | null>(
    null,
  );
  // Another conversation closes the review.
  useEffect(() => setTarget(null), [sessionId]);
  const open = useCallback(
    (taskId: string, path?: string) => setTarget({ taskId, path }),
    [],
  );
  const close = useCallback(() => setTarget(null), []);
  return { target, open, close };
}

/** Kept files and hunks of one task's review (a reviewing convenience in
 * browser storage: Keep changes nothing on disk). */
export type Kept = { files: string[]; hunks: string[] };
export const keptKey = (taskId: string) => `shadow:review:${taskId}`;
export const hunkKey = (path: string, id: string) => `${path}#${id}`;

export function readKept(taskId: string): Kept {
  try {
    const value = JSON.parse(readStore(keptKey(taskId)) || "{}");
    return {
      files: Array.isArray(value.files) ? value.files : [],
      hunks: Array.isArray(value.hunks) ? value.hunks : [],
    };
  } catch {
    return { files: [], hunks: [] };
  }
}

/** One task's review: its changed files, the open file's hunks, Keep and
 * Undo. `readOnly` while a task runs in the project. */
export function useTaskReview({
  taskId,
  initialPath,
  appBusy,
  toast,
  refresh,
}: {
  taskId: string;
  initialPath?: string;
  appBusy: boolean;
  toast: (text: string, kind?: ToastKind) => void;
  refresh: () => Promise<void>;
}) {
  const [files, setFiles] = useState<ReviewFile[] | null>(null);
  const [engineBusy, setEngineBusy] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState(initialPath || "");
  const [detail, setDetail] = useState<ReviewFileDetail | null>(null);
  const [kept, setKept] = useState<Kept>(() => readKept(taskId));
  const [working, setWorking] = useState(false);
  const readOnly = appBusy || engineBusy;

  const load = useCallback(async () => {
    try {
      const result = await api.review(taskId);
      setFiles(result.files);
      setEngineBusy(result.busy);
      setError("");
      return result.files;
    } catch (e) {
      setError(String(e));
      setFiles([]);
      return [];
    }
  }, [taskId]);

  useEffect(() => {
    setKept(readKept(taskId));
    setSelected(initialPath || "");
    void load().then((list) => {
      setSelected((current) =>
        current && list.some((f) => f.path === current)
          ? current
          : list.find((f) => f.status !== "unchanged")?.path ||
            list[0]?.path ||
            "",
      );
    });
  }, [taskId, initialPath, load]);

  // A task finishing (or starting) changes what the review may do.
  useEffect(() => {
    void load();
  }, [appBusy, load]);

  useEffect(() => {
    if (!selected) {
      setDetail(null);
      return;
    }
    let live = true;
    api
      .reviewFile(taskId, selected)
      .then((next) => {
        if (live) setDetail(next);
      })
      .catch((e) => {
        if (live) {
          setDetail(null);
          toast(String(e), "err");
        }
      });
    return () => {
      live = false;
    };
  }, [taskId, selected, toast]);

  const saveKept = (next: Kept) => {
    setKept(next);
    writeStore(keptKey(taskId), JSON.stringify(next));
  };
  const keepFile = (path: string) =>
    saveKept({
      files: kept.files.includes(path) ? kept.files : [...kept.files, path],
      hunks: kept.hunks,
    });
  const keepHunk = (path: string, id: string) => {
    const key = hunkKey(path, id);
    saveKept({
      files: kept.files,
      hunks: kept.hunks.includes(key) ? kept.hunks : [...kept.hunks, key],
    });
  };

  async function undo(path: string, hunk?: string) {
    if (readOnly || working) return false;
    setWorking(true);
    try {
      const next = await api.reviewUndo(taskId, path, hunk);
      if (path === selected) setDetail(next);
      await load();
      void refresh().catch(() => undefined);
      return true;
    } catch (e) {
      toast(String(e), "err");
      return false;
    } finally {
      setWorking(false);
    }
  }

  /** Undo every changed file that was not kept. */
  async function undoAll() {
    const pending = (files || []).filter(
      (f) =>
        !kept.files.includes(f.path) &&
        !["unchanged", "unavailable"].includes(f.status),
    );
    let done = 0;
    for (const file of pending) {
      if (!(await undo(file.path))) break;
      done++;
    }
    if (done)
      toast(
        `Undid this task's changes in ${done} file${done === 1 ? "" : "s"}.`,
        "ok",
      );
  }

  return {
    files,
    error,
    readOnly,
    selected,
    setSelected,
    detail,
    kept,
    keepFile,
    keepHunk,
    undo,
    undoAll,
    working,
    reload: load,
  };
}
