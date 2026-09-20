import { useCallback, useEffect, useState } from "react";
import {
  api,
  type BackgroundTask,
  type DiffHunk,
  type DoctorReport,
  type FileEntry,
  type Goal,
  type Health,
  type RoutingView,
  type Session,
  type UpdateInfo,
} from "../api";
import { Empty } from "./cards";
import { exportSession } from "../lib/transport";

export type DrawerTab =
  | "terminal"
  | "sessions"
  | "files"
  | "changes"
  | "skills"
  | "goals"
  | "health"
  | "background";
export const DRAWER_TABS: { id: DrawerTab; label: string }[] = [
  { id: "sessions", label: "Sessions" },
  { id: "files", label: "Files" },
  { id: "terminal", label: "Terminal" },
  { id: "changes", label: "Changes" },
  { id: "skills", label: "Skills" },
  { id: "goals", label: "Goals" },
  { id: "health", label: "Health" },
  { id: "background", label: "Background" },
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
  health,
  busy,
  toast,
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
  health: Health | null;
  busy: boolean;
  toast: Toast;
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
          <ChangesTab path={diffPath} busy={busy} toast={toast} />
        )}
        {tab === "skills" && <SkillsTab toast={toast} />}
        {tab === "goals" && <GoalsTab sessionId={sessionId} toast={toast} />}
        {tab === "health" && <HealthTab health={health} toast={toast} />}
        {tab === "background" && <BackgroundTab toast={toast} />}
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

function ChangesTab({
  path,
  busy,
  toast,
}: {
  path: string;
  busy: boolean;
  toast: Toast;
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
      setHunks([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, busy]);
  useEffect(() => {
    setSelected(path);
  }, [path]);
  useEffect(() => {
    void loadHunks(selected);
  }, [selected, loadHunks, busy]);

  async function act(h: DiffHunk, action: "accept" | "reject") {
    try {
      await api.hunkAction(selected, h, action);
      toast(action === "accept" ? "Hunk staged" : "Hunk reverted", "ok");
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
            <span className={`file-label l-${f.label.toLowerCase()}`}>
              {f.label}
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

// --- Skills / instructions ------------------------------------------------------

function SkillsTab({ toast }: { toast: Toast }) {
  const [instructions, setInstructions] = useState("");
  const [skills, setSkills] = useState<{ name: string; content: string }[]>([]);
  const [name, setName] = useState("workflow");
  const [body, setBody] = useState("");
  const load = useCallback(async () => {
    try {
      const [inst, sk] = await Promise.all([api.instructions(), api.skills()]);
      setInstructions(inst.content);
      setSkills(sk.skills);
    } catch {
      /* not in a workspace */
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <>
      <p className="hint">
        Injected into every run from <code>.shadow/</code>.
      </p>
      <div className="field">
        <label>.shadow/instructions.md</label>
        <textarea
          rows={7}
          value={instructions}
          onChange={(e) => setInstructions(e.target.value)}
          placeholder="How this project likes to be worked on…"
        />
        <button
          type="button"
          className="mini"
          onClick={() =>
            void api
              .saveInstructions(instructions)
              .then(() => toast("Instructions saved", "ok"))
          }
        >
          Save
        </button>
      </div>
      <div className="field">
        <label>Skill</label>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="skill name"
        />
        <textarea
          rows={5}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="# How to run tests…"
        />
        <button
          type="button"
          className="mini"
          onClick={() =>
            void api
              .saveSkill(name, body)
              .then(load)
              .then(() => toast(`Saved skill ${name}`, "ok"))
          }
        >
          Save skill
        </button>
      </div>
      <div className="list">
        {skills.map((s) => (
          <button
            type="button"
            key={s.name}
            className="file"
            onClick={() => {
              setName(s.name);
              setBody(s.content);
            }}
          >
            {s.name}
          </button>
        ))}
      </div>
    </>
  );
}

// --- Goals: milestone checklist · resume · progress ----------------------------

function GoalsTab({ sessionId, toast }: { sessionId: string; toast: Toast }) {
  const [goals, setGoals] = useState<Goal[]>([]);
  const [text, setText] = useState("");
  const load = useCallback(async () => {
    try {
      setGoals((await api.goals()).goals);
    } catch {
      setGoals([]);
    }
  }, []);
  useEffect(() => {
    void load();
    const id = setInterval(() => void load(), 3000);
    return () => clearInterval(id);
  }, [load]);

  async function create(run: boolean) {
    if (!text.trim()) return;
    try {
      await api.createGoal(text.trim(), run, sessionId || undefined);
      setText("");
      await load();
      toast(run ? "Goal started" : "Goal created", "ok");
    } catch (err) {
      toast(String(err), "err");
    }
  }

  return (
    <>
      <div className="field">
        <textarea
          rows={2}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="One line: what should be true when this goal is done?"
        />
        <div className="row">
          <button
            type="button"
            className="mini"
            onClick={() => void create(false)}
          >
            Plan
          </button>
          <button
            type="button"
            className="mini primary-mini"
            onClick={() => void create(true)}
          >
            Plan & run
          </button>
        </div>
      </div>
      {goals.length === 0 && (
        <Empty
          title="No goals yet"
          body="A goal is a checklist of milestones the agent works through, one verified task at a time."
        />
      )}
      {goals.map((g) => (
        <div key={g.id} className={`goal ${g.status}`}>
          <header>
            <strong>{g.title || g.instruction}</strong>
            <span className="goal-pct">{g.progress_pct}%</span>
          </header>
          <div className="bar">
            <i style={{ width: `${g.progress_pct}%` }} />
          </div>
          <ul className="milestones">
            {g.milestones.map((m) => (
              <li key={m.id} className={m.status}>
                <button
                  type="button"
                  className="tick"
                  title="Toggle done"
                  onClick={() =>
                    void api
                      .setMilestone(
                        g.id,
                        m.id,
                        m.status === "done" ? "pending" : "done",
                      )
                      .then(load)
                  }
                >
                  {m.status === "done"
                    ? "✓"
                    : m.status === "in_progress"
                      ? "▸"
                      : m.status === "failed"
                        ? "✗"
                        : "○"}
                </button>
                <span>{m.title}</span>
              </li>
            ))}
          </ul>
          <div className="row goal-actions">
            <span className="dim">{g.running ? "running…" : g.status}</span>
            {!g.running && g.status !== "completed" && (
              <button
                type="button"
                className="mini"
                onClick={() =>
                  void api
                    .runGoal(g.id, sessionId || undefined)
                    .then(load)
                    .catch((err) => toast(String(err), "err"))
                }
              >
                {g.progress > 0 ? "Resume" : "Run"}
              </button>
            )}
            {!g.running && g.status === "active" && (
              <button
                type="button"
                className="mini"
                onClick={() => void api.abandonGoal(g.id).then(load)}
              >
                Abandon
              </button>
            )}
            {!g.running && (
              <button
                type="button"
                className="mini danger-text"
                onClick={() =>
                  void api
                    .deleteGoal(g.id)
                    .then(load)
                    .catch((err) => toast(String(err), "err"))
                }
              >
                Delete
              </button>
            )}
          </div>
        </div>
      ))}
    </>
  );
}

// --- Health: provider · tools · doctor · router · update ----------------------

function HealthTab({ health, toast }: { health: Health | null; toast: Toast }) {
  const [report, setReport] = useState<DoctorReport | null>(null);
  const [routing, setRouting] = useState<RoutingView | null>(null);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [fixing, setFixing] = useState(false);
  const load = useCallback(async () => {
    void api
      .doctor()
      .then(setReport)
      .catch(() => setReport(null));
    void api
      .routing()
      .then(setRouting)
      .catch(() => setRouting(null));
    void api
      .updateCheck()
      .then(setUpdate)
      .catch(() => setUpdate(null));
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  return (
    <>
      <div className="kv">
        <div>
          <span>version</span>
          <code>{health?.version || "…"}</code>
        </div>
        <div>
          <span>provider</span>
          <code className={health?.provider?.ok ? "health-ok" : "health-bad"}>
            {health?.provider?.name} — {health?.provider?.detail}
          </code>
        </div>
        {Object.entries(health?.tools || {}).map(([name, info]) => (
          <div key={name}>
            <span>{name}</span>
            <code className={info.ok ? "health-ok" : "health-bad"}>
              {info.ok ? info.detail || "yes" : "not found"}
            </code>
          </div>
        ))}
      </div>
      {update && (
        <div className={`update-row ${update.update_available ? "avail" : ""}`}>
          {update.update_available ? (
            <>
              ShadowCode {update.latest} is available — run{" "}
              <code>shadow update</code>.
            </>
          ) : (
            <>Up to date ({update.current}).</>
          )}
        </div>
      )}
      <h4>Doctor</h4>
      {report && (
        <div className="kv">
          {report.checks.map((c) => (
            <div key={c.id}>
              <span className={c.ok ? "health-ok" : "health-bad"}>
                {c.ok ? "✓" : "✗"} {c.label}
              </span>
              <code>{c.detail}</code>
            </div>
          ))}
        </div>
      )}
      <div className="row">
        <button
          type="button"
          className="mini"
          disabled={fixing}
          onClick={() => {
            setFixing(true);
            void api
              .doctorFix()
              .then((r) => {
                setReport(r.report);
                toast(
                  r.applied.length ? r.applied.join("; ") : "Nothing to fix",
                  "ok",
                );
              })
              .catch((err) => toast(String(err), "err"))
              .finally(() => setFixing(false));
          }}
        >
          {fixing ? "Fixing…" : "Auto-fix"}
        </button>
        <button type="button" className="mini" onClick={() => void load()}>
          Refresh
        </button>
      </div>
      <h4>Router</h4>
      {routing && (
        <>
          <p className="hint">
            {routing.enabled
              ? "Per-purpose routing is on."
              : `Routing off — every purpose uses ${routing.default}.`}
          </p>
          <div className="kv">
            {Object.entries(routing.table).map(([purpose, model]) => (
              <div key={purpose}>
                <span>{purpose}</span>
                <code>{model}</code>
              </div>
            ))}
          </div>
          <button
            type="button"
            className="mini"
            onClick={() =>
              void api
                .saveRouting({ enabled: !routing.enabled })
                .then(setRouting)
            }
          >
            {routing.enabled ? "Disable routing" : "Enable routing"}
          </button>
        </>
      )}
    </>
  );
}

// --- Background processes -----------------------------------------------------

function BackgroundTab({ toast }: { toast: Toast }) {
  const [tasks, setTasks] = useState<BackgroundTask[]>([]);
  const [name, setName] = useState("dev");
  const [command, setCommand] = useState("");
  const load = useCallback(async () => {
    try {
      setTasks((await api.background()).tasks);
    } catch {
      setTasks([]);
    }
  }, []);
  useEffect(() => {
    void load();
    const id = setInterval(() => void load(), 3000);
    return () => clearInterval(id);
  }, [load]);
  return (
    <>
      <p className="hint">
        Long-running processes (dev servers, watchers) that outlive a single
        task.
      </p>
      <div className="field inline">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="name"
          style={{ maxWidth: 90 }}
        />
        <input
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder="npm run dev"
          onKeyDown={(e) => {
            if (e.key === "Enter" && command.trim())
              void api
                .startBackground(name, command)
                .then(() => {
                  setCommand("");
                  void load();
                })
                .catch((err) => toast(String(err), "err"));
          }}
        />
        <button
          type="button"
          className="mini"
          disabled={!command.trim()}
          onClick={() =>
            void api
              .startBackground(name, command)
              .then(() => {
                setCommand("");
                void load();
              })
              .catch((err) => toast(String(err), "err"))
          }
        >
          Start
        </button>
      </div>
      {tasks.length === 0 && <Empty title="Nothing running" />}
      {tasks.map((t) => (
        <div key={t.id} className="bg-task">
          <header>
            <strong>{t.name}</strong>
            <span className={`dim st-${t.status.toLowerCase()}`}>
              {t.status}
              {t.exit_code !== null ? ` · exit ${t.exit_code}` : ""}
            </span>
            {t.status.toUpperCase() === "RUNNING" && (
              <button
                type="button"
                className="mini danger-text"
                onClick={() => void api.stopBackground(t.id).then(load)}
              >
                Stop
              </button>
            )}
          </header>
          <code className="dim">{t.command}</code>
          {t.output && <pre className="plan log">{t.output.slice(-1500)}</pre>}
        </div>
      ))}
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
