import { useEffect, useState } from "react";
import {
  ArrowUpRight,
  ChevronDown,
  Clock3,
  FolderOpen,
  MessageSquare,
  PanelLeftClose,
  Pin,
  Plus,
  Search,
  Settings2,
  SquarePen,
} from "lucide-react";
import { api, type Job, type Project, type Session } from "../api";

/** Projects and their recent conversations. */
export function Sidebar({
  sessions,
  projects,
  selected,
  workspace,
  jobs,
  onSelect,
  onNew,
  onProject,
  onSettings,
  onHide,
}: {
  sessions: Session[];
  projects: Project[];
  selected: string;
  workspace: string;
  jobs: Job[];
  onSelect: (id: string) => void;
  onNew: () => void;
  onProject: (path?: string) => void;
  onSettings: () => void;
  onHide: () => void;
}) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Session[] | null>(null);
  const [searchError, setSearchError] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const [pins, setPins] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("shadow:pins") || "[]");
    } catch {
      return [];
    }
  });
  useEffect(() => {
    let alive = true;
    if (!query.trim()) {
      setHits(null);
      setSearchError("");
      return;
    }
    const timer = setTimeout(
      () =>
        api
          .sessions(query)
          .then((r) => {
            if (alive) {
              setHits(r.sessions);
              setSearchError("");
            }
          })
          .catch(() => {
            if (alive) setSearchError("Search unavailable. Try again.");
          }),
      200,
    );
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [query, sessions]);
  const rows = hits ?? sessions;
  const groups = [
    ...new Set([
      workspace,
      ...projects.map((p) => p.path),
      ...rows.map((s) => s.workspace),
    ]),
  ].filter(Boolean);
  function pin(id: string) {
    const next = pins.includes(id)
      ? pins.filter((p) => p !== id)
      : [...pins, id];
    setPins(next);
    localStorage.setItem("shadow:pins", JSON.stringify(next));
  }
  function task(s: Session) {
    const running = jobs.some(
      (j) =>
        j.session_id === s.id && ["running", "cancelling"].includes(j.status),
    );
    const queued = jobs.some(
      (j) => j.session_id === s.id && j.status === "queued",
    );
    return (
      <div
        className={`task-row ${s.id === selected ? "selected" : ""}`}
        key={s.id}
      >
        <button
          type="button"
          className="task-link"
          data-session-id={s.id}
          aria-current={s.id === selected ? "page" : undefined}
          title={s.title || "Untitled task"}
          onClick={() => onSelect(s.id)}
        >
          {running ? (
            <span
              className="running-dot"
              role="img"
              aria-label="Task running"
            />
          ) : queued ? (
            <Clock3 size={14} aria-label="Task queued" />
          ) : (
            <MessageSquare size={14} />
          )}
          <span>{s.title || "Untitled task"}</span>
        </button>
        <button
          type="button"
          className={`pin-task ${pins.includes(s.id) ? "pinned" : ""}`}
          aria-label={`${pins.includes(s.id) ? "Unpin" : "Pin"} ${s.title || "task"}`}
          onClick={() => pin(s.id)}
        >
          <Pin size={12} />
        </button>
      </div>
    );
  }
  return (
    <aside className="sidebar" aria-label="Projects and tasks">
      <div className="brand">
        <img src="/icon.svg" alt="" />
        <strong>ShadowCode</strong>
        <span className="grow" />
        <button
          type="button"
          className="icon-btn"
          title="Hide sidebar (Ctrl+B)"
          aria-label="Hide sidebar"
          onClick={onHide}
        >
          <PanelLeftClose size={17} />
        </button>
      </div>
      <button type="button" className="new-task" onClick={onNew}>
        <SquarePen size={17} />
        <span>New task</span>
        <kbd>Ctrl N</kbd>
      </button>
      <label className="sidebar-search">
        <Search size={15} />
        <input
          aria-label="Search tasks"
          placeholder="Search tasks"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </label>
      <div className="sidebar-section-title">
        Recent / Projects
        <button
          type="button"
          className="icon-btn"
          aria-label="Open project"
          title="Open project (Ctrl+P)"
          onClick={() => onProject()}
        >
          <Plus size={15} />
        </button>
      </div>
      <div className="project-list">
        {searchError && <p className="hint">{searchError}</p>}
        {pins.some((id) => rows.some((s) => s.id === id)) && (
          <div className="project-group">
            <div className="pinned-title">
              <Pin size={12} /> Pinned
            </div>
            {rows.filter((s) => pins.includes(s.id)).map(task)}
          </div>
        )}
        {groups.map((path) => {
          const tasks = rows.filter(
            (s) => s.workspace === path && !pins.includes(s.id),
          );
          if (query && !tasks.length) return null;
          const shut = collapsed.includes(path) && !query;
          return (
            <div className="project-group" key={path}>
              <div className="project-heading">
                <button
                  type="button"
                  title={path}
                  aria-expanded={!shut}
                  onClick={() =>
                    setCollapsed(
                      shut
                        ? collapsed.filter((p) => p !== path)
                        : [...collapsed, path],
                    )
                  }
                >
                  <ChevronDown size={13} className={shut ? "rotated" : ""} />
                  <FolderOpen size={15} />
                  <span>{path.split("/").pop() || path}</span>
                </button>
                <button
                  type="button"
                  className="project-open icon-btn"
                  aria-label={`Open ${path}`}
                  onClick={() => onProject(path)}
                >
                  <ArrowUpRight size={13} />
                </button>
              </div>
              {!shut &&
                (tasks.length ? (
                  tasks.map(task)
                ) : (
                  <button
                    type="button"
                    className="project-empty"
                    onClick={() => onProject(path)}
                  >
                    Start a task here <Plus size={12} />
                  </button>
                ))}
            </div>
          );
        })}
        {query && rows.length === 0 && (
          <div className="sidebar-empty">No tasks match “{query}”.</div>
        )}
        {!groups.length && (
          <div className="sidebar-empty">Open a project to get started.</div>
        )}
      </div>
      <div className="sidebar-bottom">
        <button type="button" onClick={onSettings}>
          <Settings2 size={16} />
          <span>Settings</span>
          <kbd>Ctrl ,</kbd>
        </button>
      </div>
    </aside>
  );
}
