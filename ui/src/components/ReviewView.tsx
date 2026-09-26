import { useState } from "react";
import { ArrowLeft, Check, FileDiff, GitBranch } from "lucide-react";
import type { ReviewFile } from "../api";
import { ChangesTab } from "./ChangesTab";
import { ConfirmDialog } from "./ConfirmDialog";
import { DiffHunk, LayoutToggle, type DiffLayout } from "./DiffView";
import { Empty } from "./cards";
import { hunkKey, useTaskReview } from "../hooks/useReview";
import type {
  DrawerMemory,
  DrawerMemoryUpdate,
} from "../hooks/useDrawerMemory";
import type { ToastKind } from "../hooks/useToasts";
import { languageOf } from "../lib/diff";
import { readStore, writeStore } from "../lib/storage";

const STATUS: Record<string, string> = {
  added: "New",
  modified: "Edited",
  deleted: "Deleted",
  unchanged: "Undone",
  unavailable: "Unavailable",
};

type Confirm =
  { kind: "file"; path: string } | { kind: "all"; count: number } | null;

/** The files one task changed, full width: a file list, a diff viewer
 * (unified or split, highlighted), Keep or Undo per hunk and per file, and
 * Undo all. The project's Git staging stays one tab away. While a task
 * runs in the project the review is read-only. */
export function ReviewView({
  taskId,
  initialPath,
  busy,
  onClose,
  toast,
  refresh,
  onAskAgent,
  memory,
  onMemory,
}: {
  taskId: string;
  initialPath?: string;
  busy: boolean;
  onClose: () => void;
  toast: (text: string, kind?: ToastKind) => void;
  refresh: () => Promise<void>;
  onAskAgent: (prompt: string) => void;
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
}) {
  const [tab, setTab] = useState<"task" | "git">("task");
  const [layout, setLayoutState] = useState<DiffLayout>(() =>
    readStore("shadow:diff-layout") === "split" ? "split" : "unified",
  );
  const setLayout = (next: DiffLayout) => {
    setLayoutState(next);
    writeStore("shadow:diff-layout", next);
  };
  const [confirm, setConfirm] = useState<Confirm>(null);
  const review = useTaskReview({
    taskId,
    initialPath,
    appBusy: busy,
    toast,
    refresh,
  });
  const { files, detail, kept, readOnly } = review;
  const open = (files || []).filter(
    (f) =>
      !kept.files.includes(f.path) &&
      !["unchanged", "unavailable"].includes(f.status),
  );
  const language = detail ? languageOf(detail.path) : undefined;
  const fileKept = detail ? kept.files.includes(detail.path) : false;
  return (
    <section className="review-view" aria-label="Review changes">
      <header className="review-head">
        <button type="button" className="ghost review-back" onClick={onClose}>
          <ArrowLeft size={14} aria-hidden="true" /> Back to conversation
        </button>
        <h2>Review changes</h2>
        <div className="seg" role="tablist" aria-label="Review">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "task"}
            className={tab === "task" ? "on" : ""}
            onClick={() => setTab("task")}
          >
            <FileDiff size={13} aria-hidden="true" /> This task
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "git"}
            className={tab === "git" ? "on" : ""}
            onClick={() => setTab("git")}
          >
            <GitBranch size={13} aria-hidden="true" /> Git
          </button>
        </div>
        <span className="grow" />
        {tab === "task" && (
          <>
            <LayoutToggle layout={layout} onLayout={setLayout} />
            <button
              type="button"
              className="mini danger-text"
              disabled={readOnly || !open.length || review.working}
              onClick={() => setConfirm({ kind: "all", count: open.length })}
            >
              Undo all
            </button>
          </>
        )}
      </header>
      {readOnly && tab === "task" && (
        <p className="notice review-readonly" role="status">
          A task is running in this project, so the review is read-only. You can
          undo changes when it finishes.
        </p>
      )}
      {tab === "git" ? (
        <div className="review-git">
          <ChangesTab
            path=""
            busy={busy}
            toast={toast}
            onAskAgent={onAskAgent}
            memory={memory}
            onMemory={onMemory}
          />
        </div>
      ) : files === null ? (
        <div className="skel-rows">
          <span className="skel" />
          <span className="skel short" />
        </div>
      ) : review.error ? (
        <Empty title="Could not load this task's changes" body={review.error} />
      ) : !files.length ? (
        <Empty
          title="This task changed no files"
          body="Nothing to review. The Git tab shows everything uncommitted in the project."
        />
      ) : (
        <div className="review-body">
          <nav className="review-files" aria-label="Changed files">
            {files.map((file: ReviewFile) => (
              <button
                type="button"
                key={file.path}
                className={`review-file ${review.selected === file.path ? "active" : ""}`}
                aria-current={
                  review.selected === file.path ? "true" : undefined
                }
                onClick={() => review.setSelected(file.path)}
              >
                <span className={`review-status is-${file.status}`}>
                  {kept.files.includes(file.path) ? (
                    <>
                      <Check size={11} aria-hidden="true" /> Kept
                    </>
                  ) : (
                    STATUS[file.status] || file.status
                  )}
                </span>
                <span className="review-path">{file.path}</span>
                {!file.binary && (file.added > 0 || file.removed > 0) && (
                  <span className="diff-stat">
                    <span className="add">+{file.added}</span>{" "}
                    <span className="del">−{file.removed}</span>
                  </span>
                )}
              </button>
            ))}
          </nav>
          <div className="review-diff">
            {detail ? (
              <>
                <div className="review-file-head">
                  <code>{detail.path}</code>
                  {detail.source === "git" && (
                    <span className="hint">
                      Changed by a vendor agent; compared with the last commit.
                    </span>
                  )}
                  <span className="grow" />
                  <button
                    type="button"
                    className="mini"
                    disabled={fileKept}
                    onClick={() => review.keepFile(detail.path)}
                  >
                    {fileKept ? "Kept" : "Keep file"}
                  </button>
                  <button
                    type="button"
                    className="mini danger-text"
                    disabled={
                      readOnly ||
                      review.working ||
                      detail.status === "unchanged"
                    }
                    onClick={() =>
                      setConfirm({ kind: "file", path: detail.path })
                    }
                  >
                    Undo file
                  </button>
                </div>
                {detail.binary ? (
                  <p className="hint">
                    Binary or very large file: no line view. Undo file puts it
                    back as a whole.
                  </p>
                ) : !detail.hunks.length ? (
                  <p className="hint">
                    {detail.status === "unchanged"
                      ? "This file is back to how it was before the task."
                      : "No text changes."}
                  </p>
                ) : (
                  detail.hunks.map((hunk) => {
                    const hunkKept =
                      fileKept ||
                      kept.hunks.includes(hunkKey(detail.path, hunk.id));
                    return (
                      <DiffHunk
                        key={hunk.id}
                        hunk={hunk}
                        layout={layout}
                        language={language}
                        status={hunkKept ? "Kept" : undefined}
                        actions={
                          <>
                            <button
                              type="button"
                              className="mini"
                              disabled={hunkKept}
                              aria-label={`Keep change ${hunk.header}`}
                              onClick={() =>
                                review.keepHunk(detail.path, hunk.id)
                              }
                            >
                              Keep
                            </button>
                            <button
                              type="button"
                              className="mini danger-text"
                              disabled={readOnly || review.working}
                              aria-label={`Undo change ${hunk.header}`}
                              onClick={() =>
                                void review.undo(detail.path, hunk.id)
                              }
                            >
                              Undo
                            </button>
                          </>
                        }
                      />
                    );
                  })
                )}
              </>
            ) : (
              <p className="hint">Choose a file.</p>
            )}
          </div>
        </div>
      )}
      {confirm && (
        <ConfirmDialog
          title={
            confirm.kind === "all"
              ? "Undo this task's changes?"
              : `Undo the changes to ${confirm.path}?`
          }
          confirmLabel={
            confirm.kind === "all"
              ? `Undo ${confirm.count} file${confirm.count === 1 ? "" : "s"}`
              : "Undo file"
          }
          danger
          onCancel={() => setConfirm(null)}
          onConfirm={async () => {
            if (confirm.kind === "all") await review.undoAll();
            else if (await review.undo(confirm.path))
              toast(`Undid this task's changes to ${confirm.path}.`, "ok");
            setConfirm(null);
          }}
        >
          <p>
            {confirm.kind === "all"
              ? "Every file this task changed that you have not kept goes back to how it was before the task. Files it created are removed."
              : "The file goes back to how it was before the task (a file the task created is removed)."}
          </p>
        </ConfirmDialog>
      )}
    </section>
  );
}
