import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type FileEntry, type Session } from "../api";
import { Empty } from "./cards";
import { ChangesTab } from "./ChangesTab";
import { exportSession } from "../lib/transport";
import {
  remembered,
  type DrawerMemory,
  type DrawerMemoryUpdate,
} from "../hooks/useDrawerMemory";

/** The optional right-hand drawer: conversations, files, a terminal and the
 * readable diff of current changes. Project tools live in Settings › Advanced.
 * Work in a tab (terminal output, the open file, a commit message) lives in
 * `memory`, owned by the app, so switching tabs keeps it. */
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
  memory,
  onMemory,
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
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
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
          <X size={16} aria-hidden="true" />
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
          <TerminalTab
            workspace={workspace}
            busy={busy}
            toast={toast}
            memory={memory}
            onMemory={onMemory}
          />
        )}
        {tab === "files" && (
          <FilesTab
            workspace={workspace}
            memory={memory}
            onMemory={onMemory}
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
            memory={memory}
            onMemory={onMemory}
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
  memory,
  onMemory,
}: {
  workspace: string;
  onShowDiff: (path: string) => void;
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
}) {
  const [dir, setDir] = remembered(memory, onMemory, "filesDir");
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [view, setView] = remembered(memory, onMemory, "filesView");

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

function TerminalTab({
  workspace,
  busy,
  toast,
  memory,
  onMemory,
}: {
  workspace: string;
  busy: boolean;
  toast: Toast;
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
}) {
  const [command, setCommand] = remembered(memory, onMemory, "terminalCommand");
  const [running, setRunning] = useState(false);
  const [history, setHistory] = remembered(memory, onMemory, "terminalHistory");
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
