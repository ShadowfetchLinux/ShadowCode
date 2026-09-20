import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type ManagedWorktree, type WorktreeInspection } from "../api";
export function WorktreeSettings({
  onOpen,
  onToast,
}: {
  onOpen?: (path: string) => void;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
}) {
  const [workspace, setWorkspace] = useState("");
  const [records, setRecords] = useState<ManagedWorktree[]>([]);
  const [reference, setReference] = useState("HEAD");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [review, setReview] = useState<WorktreeInspection | null>(null);
  const reviewRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let live = true;
    void api
      .worktrees()
      .then((data) => {
        if (live) {
          setWorkspace(data.workspace);
          setRecords(data.worktrees);
        }
      })
      .catch((e) => {
        if (live) setError(String(e));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, []);
  useLayoutEffect(() => {
    if (review) {
      reviewRef.current?.scrollIntoView({ block: "nearest" });
      reviewRef.current?.focus({ preventScroll: true });
    }
  }, [review]);
  async function refresh() {
    const data = await api.worktrees();
    setWorkspace(data.workspace);
    setRecords(data.worktrees);
    setReview(null);
  }
  async function perform(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
      setReview(null);
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="settings-section" aria-label="Managed worktrees">
      <h3>Isolated worktrees</h3>
      <p className="hint">
        Work on a separate branch while keeping this checkout intact. New
        worktrees start from a local commit; existing uncommitted edits stay
        here.
      </p>
      <p className="hint">Source project</p>
      <code className="worktree-path">
        {workspace || (loading ? "Loading project…" : "Project unavailable")}
      </code>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <div className="row worktree-create">
        <label>
          Starting commit or branch
          <input
            value={reference}
            onChange={(e) => setReference(e.target.value)}
            disabled={busy || loading}
            placeholder="HEAD"
            maxLength={256}
            spellCheck={false}
          />
        </label>
        <button
          type="button"
          className="primary"
          disabled={busy || loading || !workspace || !reference.trim()}
          onClick={() =>
            void perform(async () => {
              const created = await api.createWorktree(
                workspace,
                reference.trim(),
              );
              await refresh();
              onToast(`Created ${created.branch}`, "ok");
            })
          }
        >
          Create worktree
        </button>
        <button
          type="button"
          className="ghost"
          disabled={busy}
          onClick={() => void perform(refresh)}
        >
          Refresh
        </button>
      </div>
      {loading && <p role="status">Loading worktrees…</p>}
      {!loading && !records.length && !error && (
        <p className="hint">No managed worktrees for this project yet.</p>
      )}
      <div className="worktree-list">
        {records.map((record) => (
          <article className="worktree-card" key={record.id}>
            <strong>{record.branch}</strong>
            <span className="hint">{record.state.replaceAll("_", " ")}</span>
            <code className="worktree-path">{record.path}</code>
            <p className="hint">{record.detail}</p>
            <div className="row">
              <button
                type="button"
                className="ghost"
                disabled={busy || !onOpen}
                onClick={() => onOpen?.(record.path)}
              >
                Open worktree
              </button>
              <button
                type="button"
                className="ghost"
                disabled={busy}
                onClick={() =>
                  void perform(async () =>
                    setReview(await api.inspectWorktree(workspace, record.id)),
                  )
                }
              >
                Inspect removal
              </button>
            </div>
          </article>
        ))}
      </div>
      {review && (
        <div
          className="worktree-review"
          ref={reviewRef}
          tabIndex={-1}
          role="region"
          aria-label="Review worktree removal"
        >
          <h4>Review removal</h4>
          <code className="worktree-path">{review.record.path}</code>
          <p>
            Branch retained:{" "}
            <strong>{review.current_branch || "Detached HEAD"}</strong>
          </p>
          <p>
            Commit retained: <code>{review.head}</code>
          </p>
          <p>{review.reason}</p>
          {review.status && <pre>{review.status}</pre>}
          <p className="hint">
            Removal also checks for active tasks and background processes. Your
            source checkout and committed branch history are preserved.
          </p>
          <div className="row">
            <button
              type="button"
              className="ghost"
              disabled={busy}
              onClick={() => setReview(null)}
            >
              Keep worktree
            </button>
            <button
              type="button"
              className="ghost"
              disabled={busy || !review.can_remove}
              onClick={() =>
                void perform(async () => {
                  await api.removeWorktree(
                    workspace,
                    review.record.id,
                    review.hash,
                  );
                  await refresh();
                  onToast(
                    "Worktree removed; branch and commits retained",
                    "ok",
                  );
                })
              }
            >
              Remove clean worktree
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
