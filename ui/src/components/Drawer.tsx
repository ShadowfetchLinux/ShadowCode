import { useCallback, useEffect, useState } from "react";
import { api, type DiffHunk, type FileEntry, type Session } from "../api";
import { Empty } from "./cards";
import { exportSession } from "../lib/transport";

/** The optional right-hand drawer: conversations, files, a terminal and the
 * readable diff of current changes. Project tools live in Settings › Advanced. */
export type DrawerTab = "terminal" | "sessions" | "files" | "changes";
export const DRAWER_TABS: { id: DrawerTab; label: string }[] = [
  { id: "changes", label: "Changes" },
  { id: "terminal", label: "Terminal" },
  { id: "files", label: "Files" },
  { id: "sessions", label: "Sessions" },
];

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

export function Drawer({
  tab,
  onTab,
  onClose,
  workspace,
  sessions,
  sessionId,
  onOpenSession,
  onNewSession,
  onRefreshSessions,
  diffPath,
  onDiffPath,
  busy,
  toast,
  onAskAgent,
}: {
  tab: DrawerTab;
  onTab: (t: DrawerTab) => void;
  onClose: () => void;
  workspace: string;
  sessions: Session[];
  sessionId: string;
  onOpenSession: (id: string) => void;
  onNewSession: () => void;
  onRefreshSessions: () => Promise<void>;
  diffPath: string;
  onDiffPath: (path: string) => void;
  busy: boolean;
  toast: Toast;
  onAskAgent?: (prompt: string) => void;
}) {
  return (
    <aside className="drawer" aria-label="Drawer">
      <div className="drawer-tabs">
        {DRAWER_TABS.map((t) => (
          <button
            type="button"
            key={t.id}
            className={tab === t.id ? "on" : ""}
            onClick={() => onTab(t.id)}
          >
            {t.label}
          </button>
        ))}
        <button
          type="button"
          className="icon-btn drawer-close"
          aria-label="Close drawer"
          title="Close (Esc)"
          onClick={onClose}
        >
          ×
        </button>
      </div>
      <div className="drawer-body">
        {tab === "sessions" && (
          <SessionsTab
            sessions={sessions}
            sessionId={sessionId}
            onOpen={onOpenSession}
            onNew={onNewSession}
            onRefresh={onRefreshSessions}
            toast={toast}
          />
        )}
        {tab === "terminal" && (
          <TerminalTab workspace={workspace} busy={busy} toast={toast} />
        )}
        {tab === "files" && (
          <FilesTab
            workspace={workspace}
            onShowDiff={(p) => {
              onDiffPath(p);
              onTab("changes");
            }}
          />
        )}
        {tab === "changes" && (
          <ChangesTab
            path={diffPath}
            busy={busy}
            toast={toast}
            onAskAgent={onAskAgent}
          />
        )}
      </div>
    </aside>
  );
}

// --- Sessions: search · rename · delete · branch ----------------------------

function SessionsTab({
  sessions,
  sessionId,
  onOpen,
  onNew,
  onRefresh,
  toast,
}: {
  sessions: Session[];
  sessionId: string;
  onOpen: (id: string) => void;
  onNew: () => void;
  onRefresh: () => Promise<void>;
  toast: Toast;
}) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Session[] | null>(null);
  const [renaming, setRenaming] = useState<{
    id: string;
    title: string;
  } | null>(null);

  useEffect(() => {
    if (!q.trim()) {
      setHits(null);
      return;
    }
    const handle = setTimeout(
      () =>
        void api
          .sessions(q)
          .then((d) => setHits(d.sessions))
          .catch(() => setHits([])),
      180,
    );
    return () => clearTimeout(handle);
  }, [q]);

  const rows = hits ?? sessions;

  async function rename() {
    if (!renaming) return;
    try {
      await api.renameSession(renaming.id, renaming.title);
      setRenaming(null);
      await onRefresh();
      if (q) setHits((await api.sessions(q)).sessions);
    } catch (err) {
      toast(String(err), "err");
    }
  }

  async function remove(id: string) {
    if (!window.confirm("Delete this session and its transcript?")) return;
    try {
      await api.deleteSession(id);
      toast("Session deleted", "ok");
      await onRefresh();
      if (q) setHits((await api.sessions(q)).sessions);
    } catch (err) {
      toast(String(err), "err");
    }
  }

  return (
    <>
      <div className="drawer-toolbar">
        <input
          className="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search sessions…"
        />
        <button type="button" className="mini" onClick={onNew}>
          New
        </button>
      </div>
      {rows.length === 0 && (
        <Empty
          title={q ? "No matches" : "No sessions yet"}
          body={q ? undefined : "Run a task and it shows up here."}
        />
      )}
      <div className="list">
        {rows.map((s) => (
          <div
            key={s.id}
            className={`item ${s.id === sessionId ? "active" : ""}`}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (
                e.target === e.currentTarget &&
                (e.key === "Enter" || e.key === " ")
              )
                onOpen(s.id);
            }}
            onClick={() => onOpen(s.id)}
          >
            {renaming?.id === s.id ? (
              <input
                autoFocus
                className="rename"
                value={renaming.title}
                onClick={(e) => e.stopPropagation()}
                onChange={(e) =>
                  setRenaming({ id: s.id, title: e.target.value })
                }
                onKeyDown={(e) => {
                  if (e.key === "Enter") void rename();
                  if (e.key === "Escape") setRenaming(null);
                }}
                onBlur={() => void rename()}
              />
            ) : (
              <strong>
                {s.title || "Untitled"}
                {s.parent_id ? " ↳" : ""}
              </strong>
            )}
            <span>
              {s.status} · {s.workspace.split("/").pop()}
            </span>
            <div className="item-actions" onClick={(e) => e.stopPropagation()}>
              <button
                type="button"
                className="mini"
                onClick={() => setRenaming({ id: s.id, title: s.title || "" })}
              >
                Rename
              </button>
              <button
                type="button"
                className="mini"
                title="Fork this session"
                onClick={() =>
                  void api.branchSession(s.id).then(() => onRefresh())
                }
              >
                Branch
              </button>
              <button
                type="button"
                className="mini"
                title="Export as Markdown"
                onClick={() =>
                  void exportSession(s.id).catch((e) => toast(String(e), "err"))
                }
              >
                Export
              </button>
              <button
                type="button"
                className="mini danger-text"
                onClick={() => void remove(s.id)}
              >
                Delete
              </button>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

// --- Files -------------------------------------------------------------------

function FilesTab({
  workspace,
  onShowDiff,
}: {
  workspace: string;
  onShowDiff: (path: string) => void;
}) {
  const [dir, setDir] = useState(".");
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [view, setView] = useState<{ path: string; content: string } | null>(
    null,
  );

  useEffect(() => {
    void api
      .files(dir)
      .then((d) => setFiles(d.entries))
      .catch(() => setFiles([]));
  }, [dir, workspace]);

  return (
    <>
      <div className="crumb">
        <button
          type="button"
          className="mini"
          disabled={dir === "."}
          onClick={() => setDir(dir.split("/").slice(0, -1).join("/") || ".")}
        >
          ↑
        </button>
        <span>
          {workspace.split("/").pop()}
          {dir === "." ? "" : `/${dir}`}
        </span>
      </div>
      <div className="list">
        {files.length === 0 && <Empty title="Empty folder" />}
        {files.map((f) => (
          <button
            type="button"
            key={f.path}
            className="file"
            onClick={() => {
              if (f.type === "dir") {
                setDir(f.path);
                return;
              }
              void api
                .file(f.path)
                .then((r) => setView({ path: f.path, content: r.content }));
            }}
          >
            <span className="file-icon">{f.type === "dir" ? "▸" : "·"}</span>
            {f.name}
          </button>
        ))}
      </div>
      {view && (
        <div className="file-view">
          <div className="crumb">
            <span>{view.path}</span>
            <button
              type="button"
              className="mini"
              onClick={() => onShowDiff(view.path)}
            >
              Diff
            </button>
            <button
              type="button"
              className="mini"
              onClick={() => setView(null)}
            >
              ×
            </button>
          </div>
          <pre className="plan">{view.content.slice(0, 12000)}</pre>
        </div>
      )}
    </>
  );
}

// --- Changes: git status + per-hunk accept / reject --------------------------

/** Git porcelain codes in words (tooltip of the short label). */
const FILE_STATES: Record<string, string> = {
  "??": "New file, not tracked by git yet",
  M: "Modified",
  A: "Added",
  D: "Deleted",
  R: "Renamed",
  C: "Copied",
  U: "Conflict",
};

function ChangesTab({
  path,
  busy,
  toast,
  onAskAgent,
}: {
  path: string;
  busy: boolean;
  toast: Toast;
  onAskAgent?: (prompt: string) => void;
}) {
  const [git, setGit] = useState<{
    status: string;
    log: string;
    files: { path: string; label: string }[];
    repo: boolean;
    loaded: boolean;
  }>({ status: "", log: "", files: [], repo: true, loaded: false });
  const [selected, setSelected] = useState(path);
  const [hunks, setHunks] = useState<DiffHunk[]>([]);
  const [msg, setMsg] = useState("");
  const [review, setReview] = useState<Awaited<
    ReturnType<typeof api.gitDiff>
  > | null>(null);
  const [view, setView] = useState<"unstaged" | "staged">("unstaged");

  const load = useCallback(async () => {
    try {
      const g = await api.git();
      setGit({
        status: g.status,
        log: g.log,
        files: g.files || [],
        repo: Boolean(g.repo),
        loaded: true,
      });
    } catch {
      setGit({ status: "", log: "", files: [], repo: false, loaded: true });
    }
  }, []);

  const loadHunks = useCallback(async (p: string) => {
    if (!p) {
      setHunks([]);
      setReview(null);
      return;
    }
    try {
      const result = await api.gitDiff(p);
      setReview(result);
      setHunks(result.hunks);
    } catch {
      // Clear both views; a stale staged list next to an empty unstaged list
      // would misrepresent the file.
      setHunks([]);
      setReview(null);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, busy]);
  useEffect(() => {
    setSelected(path);
  }, [path]);
  // Opening the tab without a file shows the first change right away.
  useEffect(() => {
    if (!selected && git.files.length) setSelected(git.files[0].path);
  }, [selected, git.files]);
  useEffect(() => {
    void loadHunks(selected);
  }, [selected, loadHunks, busy]);

  async function act(h: DiffHunk, action: "accept" | "reject") {
    try {
      await api.hunkAction(selected, h, action);
      toast(action === "accept" ? "Hunk staged" : "Hunk discarded", "ok");
      await loadHunks(selected);
      await load();
    } catch (err) {
      toast(String(err), "err");
    }
  }

  if (!git.loaded)
    return (
      <div className="skel-rows">
        <span className="skel" />
        <span className="skel short" />
      </div>
    );
  if (!git.repo)
    return (
      <Empty
        title="Not a git repository"
        body="Initialize git in this folder to review and stage changes."
      />
    );
  return (
    <>
      <div className="list">
        {git.files.length === 0 && <Empty title="Working tree clean" />}
        {git.files.map((f) => (
          <button
            type="button"
            key={f.path}
            className={`file ${selected === f.path ? "active" : ""}`}
            onClick={() => setSelected(f.path)}
          >
            <span
              className="file-label"
              title={FILE_STATES[f.label] || f.label}
            >
              {f.label === "??" ? "New" : f.label}
            </span>
            {f.path}
          </button>
        ))}
      </div>
      {selected && (
        <div className="hunks">
          <div className="crumb">
            <span>{selected}</span>
          </div>
          <div className="seg">
            <button
              type="button"
              className={view === "unstaged" ? "on" : ""}
              onClick={() => setView("unstaged")}
            >
              Unstaged
            </button>
            <button
              type="button"
              className={view === "staged" ? "on" : ""}
              onClick={() => setView("staged")}
            >
              Staged
            </button>
          </div>
          {review?.binary && (
            <p className="hint">Binary file. Text preview is unavailable.</p>
          )}
          {review?.truncated && (
            <p className="hint">
              Preview truncated. Review the full file before staging.
            </p>
          )}
          {review?.untracked && (
            <div className="row">
              <p className="hint">New file</p>
              <button
                type="button"
                className="mini"
                disabled={busy}
                onClick={() =>
                  void api
                    .gitAdd([selected])
                    .then(async () => {
                      await load();
                      await loadHunks(selected);
                      setView("staged");
                    })
                    .catch((e) => toast(String(e), "err"))
                }
              >
                Stage file
              </button>
            </div>
          )}
          {(view === "unstaged" ? hunks : review?.staged_hunks || []).length ===
            0 && <p className="hint">No {view} text changes for this file.</p>}
          {(view === "unstaged" ? hunks : review?.staged_hunks || []).map(
            (h, i) => (
              <div key={i} className="hunk">
                <header>
                  <span>{h.header}</span>
                  {view === "unstaged" &&
                    !review?.untracked &&
                    !review?.truncated && (
                      <span className="row">
                        <button
                          type="button"
                          className="mini"
                          disabled={busy}
                          onClick={() => void act(h, "accept")}
                        >
                          Stage
                        </button>
                        <button
                          type="button"
                          className="mini danger-text"
                          disabled={busy}
                          onClick={() => {
                            if (
                              window.confirm(
                                "Discard this hunk from the working file?",
                              )
                            )
                              void act(h, "reject");
                          }}
                        >
                          Discard
                        </button>
                        {onAskAgent && (
                          <button
                            type="button"
                            className="mini"
                            onClick={() => {
                              const body = h.lines
                                .map(
                                  (line) =>
                                    `${line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}${line.text}`,
                                )
                                .join("");
                              onAskAgent(
                                `Review this hunk in ${selected}:\n${h.header}\n${body}\nAdjust or explain as needed.`,
                              );
                              toast("Hunk queued in composer", "info");
                            }}
                          >
                            Ask agent
                          </button>
                        )}
                      </span>
                    )}
                </header>
                {h.lines.map((line, j) => (
                  <div
                    key={j}
                    className={`diff-line ${line.kind === "add" ? "diff-add" : line.kind === "del" ? "diff-del" : "diff-ctx"}`}
                  >
                    {(line.kind === "add"
                      ? "+"
                      : line.kind === "del"
                        ? "-"
                        : " ") + line.text}
                  </div>
                ))}
              </div>
            ),
          )}
        </div>
      )}
      <div className="commit">
        <input
          value={msg}
          onChange={(e) => setMsg(e.target.value)}
          placeholder="Commit message"
        />
        <button
          type="button"
          className="mini"
          disabled={busy}
          onClick={() =>
            void api
              .gitAdd(["."])
              .then(async () => {
                await load();
                await loadHunks(selected);
              })
              .catch((e) => toast(String(e), "err"))
          }
        >
          Stage all
        </button>
        <button
          type="button"
          className="mini primary-mini"
          disabled={busy || !msg.trim()}
          onClick={() =>
            void api
              .gitCommit(msg)
              .then(() => {
                setMsg("");
                void load();
                toast("Committed", "ok");
              })
              .catch((err) => toast(String(err), "err"))
          }
        >
          Commit
        </button>
      </div>
      <pre className="plan log">{git.log}</pre>
    </>
  );
}

function TerminalTab({
  workspace,
  busy,
  toast,
}: {
  workspace: string;
  busy: boolean;
  toast: Toast;
}) {
  const [command, setCommand] = useState("");
  const [running, setRunning] = useState(false);
  const [history, setHistory] = useState<
    Awaited<ReturnType<typeof api.exec>>[]
  >([]);
  return (
    <section aria-label="Workspace terminal">
      <h3>Terminal</h3>
      <p className="hint">
        Run a command in {workspace.split("/").pop()}. Interactive programs are
        not supported. Commands time out after 60 seconds.
      </p>
      <form
        className="terminal-form"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!command.trim() || running || busy) return;
          setRunning(true);
          const text = command;
          setCommand("");
          try {
            const result = await api.exec(text);
            setHistory((prev) => [result, ...prev].slice(0, 20));
          } catch (err) {
            toast(String(err), "err");
            setCommand(text);
          } finally {
            setRunning(false);
          }
        }}
      >
        <span aria-hidden="true">$</span>
        <input
          aria-label="Terminal command"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder="git status"
        />
        <button
          type="submit"
          className="mini"
          disabled={running || busy || !command.trim()}
        >
          {running ? "Running…" : "Run"}
        </button>
      </form>
      {busy && (
        <p className="hint">
          The agent is working. Terminal commands are available when it
          finishes.
        </p>
      )}
      {history.length > 0 && (
        <button type="button" className="mini" onClick={() => setHistory([])}>
          Clear output
        </button>
      )}
      {history.map((r, i) => (
        <div className="terminal-result" key={i}>
          <header>
            <code>$ {r.command}</code>
            <span className={r.ok ? "health-ok" : "health-bad"}>
              exit {r.exit_code}
            </span>
          </header>
          <pre>
            {r.stdout}
            {r.stderr}
            {r.error && `\n${r.error}`}
          </pre>
        </div>
      ))}
    </section>
  );
}
