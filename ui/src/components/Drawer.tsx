import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type FileEntry, type Session } from "../api";
import { Empty } from "./cards";
import { ChangesTab } from "./ChangesTab";
import { GitPanel } from "./GitPanel";
import { TerminalPanel } from "./TerminalPanel";
import { ToolsTab, type ToolsView } from "./ToolsTab";
import { exportSession } from "../lib/transport";
import {
  remembered,
  type DrawerMemory,
  type DrawerMemoryUpdate,
} from "../hooks/useDrawerMemory";

/** The optional right-hand drawer: the readable diff of current changes,
 * Git (commit, push, pull requests), the user's own terminals, files,
 * conversations, and Tools used while working (goals, background processes,
 * worktrees). Work in a tab (the open file, a commit message, the terminal in
 * front) lives in `memory`, owned by the app, so switching tabs keeps it; the
 * terminals themselves run in the engine. */
export type DrawerTab =
  | "changes"
  | "git"
  | "terminal"
  | "files"
  | "sessions"
  | ToolsView;
const TOOLS: readonly DrawerTab[] = ["goals", "background", "worktrees"];
export const isToolTab = (tab: DrawerTab): tab is ToolsView =>
  TOOLS.includes(tab);
export const DRAWER_TABS: { id: DrawerTab | "tools"; label: string }[] = [
  { id: "changes", label: "Changes" },
  { id: "git", label: "Git" },
  { id: "terminal", label: "Terminal" },
  { id: "files", label: "Files" },
  { id: "sessions", label: "Sessions" },
  { id: "tools", label: "Tools" },
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
  onOpenProject,
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
  onOpenProject?: (path: string) => void;
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
}) {
  const tool = isToolTab(tab) ? tab : null;
  useEffect(() => {
    if (tool) onMemory("toolsView", tool);
  }, [tool, onMemory]);
  return (
    <aside className="drawer" aria-label="Drawer">
      <div className="drawer-tabs">
        {DRAWER_TABS.map((t) => {
          const on = t.id === "tools" ? Boolean(tool) : tab === t.id;
          return (
            <button
              type="button"
              key={t.id}
              className={on ? "on" : ""}
              aria-pressed={on}
              onClick={() =>
                onTab(t.id === "tools" ? memory.toolsView || "goals" : t.id)
              }
            >
              {t.label}
            </button>
          );
        })}
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
          <TerminalPanel
            workspace={workspace}
            toast={toast}
            memory={memory}
            onMemory={onMemory}
          />
        )}
        {tab === "git" && (
          <GitPanel
            busy={busy}
            toast={toast}
            memory={memory}
            onMemory={onMemory}
            onOpenTerminal={() => onTab("terminal")}
          />
        )}
        {tool && (
          <ToolsTab
            view={tool}
            onView={onTab}
            sessionId={sessionId}
            toast={toast}
            onOpenSession={onOpenSession}
            onOpenProject={onOpenProject}
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
