import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Approval, type DiffHunk, type EventRow, type FileEntry, type Health, type ModelInfo, type Project, type Session } from "./api";

type CenterTab = "conversation" | "plan" | "tools";
type RightTab = "files" | "diff" | "git" | "skills" | "health";
type Overlay = "" | "settings" | "help" | "palette" | "project";

type ChatItem =
  | { kind: "user"; text: string }
  | { kind: "agent"; text: string }
  | { kind: "tool"; tool: string; ok?: boolean; text: string; live?: boolean };

const SHORTCUTS = [
  ["Ctrl+Enter", "Run task"],
  ["Ctrl+.", "Stop agent"],
  ["Ctrl+K", "Command palette"],
  [", or Ctrl+,", "Settings"],
  ["Ctrl+P", "Open project"],
  ["Ctrl+L", "Focus prompt"],
  ["Ctrl+N", "New session"],
  ["Ctrl+Shift+E", "Export transcript"],
  ["?", "Keyboard cheat sheet"],
  ["Esc", "Close overlay"],
];

export default function App() {
  const [ready, setReady] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [events, setEvents] = useState<EventRow[]>([]);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [dir, setDir] = useState(".");
  const [fileView, setFileView] = useState("");
  const [filePath, setFilePath] = useState("");
  const [git, setGit] = useState({ status: "", log: "", diff: "", files: [] as { path: string; label: string }[], repo: false });
  const [hunks, setHunks] = useState<DiffHunk[]>([]);
  const [health, setHealth] = useState<Health | null>(null);
  const [status, setStatus] = useState({ workspace: "", model: { default: "mock", provider: "mock" }, permissions: { level: "workspace" } });
  const [center, setCenter] = useState<CenterTab>("conversation");
  const [right, setRight] = useState<RightTab>("files");
  const [task, setTask] = useState("");
  const [chat, setChat] = useState<ChatItem[]>([]);
  const [busy, setBusy] = useState(false);
  const [jobId, setJobId] = useState("");
  const [summary, setSummary] = useState("");
  const [planMd, setPlanMd] = useState("");
  const [todos, setTodos] = useState<{ id?: string; title: string; status?: string }[]>([]);
  const [usage, setUsage] = useState<Record<string, number>>({});
  const [approvals, setApprovals] = useState<Approval[]>([]);
  const [overlay, setOverlay] = useState<Overlay>("");
  const [paletteQ, setPaletteQ] = useState("");
  const [error, setError] = useState("");
  const [chips, setChips] = useState<string[]>([]);
  const [instructions, setInstructions] = useState("");
  const [skills, setSkills] = useState<{ name: string; content: string }[]>([]);
  const [skillName, setSkillName] = useState("workflow");
  const [skillBody, setSkillBody] = useState("");
  const [commitMsg, setCommitMsg] = useState("");
  const [cfg, setCfg] = useState<Record<string, unknown>>({});
  const [settingsKey, setSettingsKey] = useState("");
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const sourceRef = useRef<EventSource | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [healthData, onboard, modelData, projectData, sessionData, statusData, cfgData] = await Promise.all([
        api.health(),
        api.onboarding(),
        api.models(),
        api.projects(),
        api.sessions(),
        api.status(),
        api.config(),
      ]);
      setHealth(healthData);
      setNeedsOnboard(!onboard.completed);
      setWorkspace(healthData.workspace || statusData.workspace);
      setModels(modelData.models);
      setProjects(projectData.projects);
      setSessions(sessionData.sessions);
      setStatus(statusData);
      setCfg(cfgData);
      if (!sessionId && sessionData.sessions[0]) setSessionId(sessionData.sessions[0].id);
      setReady(true);
    } catch (err) {
      setError(String(err));
      setReady(true);
    }
  }, [sessionId]);

  const refreshInspect = useCallback(async () => {
    try {
      const [fileData, gitData, approvalData, inst, skillData] = await Promise.all([
        api.files(dir),
        api.git(),
        api.approvals(),
        api.instructions(),
        api.skills(),
      ]);
      setFiles(fileData.entries);
      setGit({
        status: gitData.status,
        log: gitData.log,
        diff: gitData.diff,
        files: gitData.files || [],
        repo: Boolean(gitData.repo),
      });
      setApprovals(approvalData.approvals);
      setInstructions(inst.content);
      setSkills(skillData.skills);
    } catch {
      /* empty folders are fine */
    }
  }, [dir]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const theme = String((cfg.ui as { theme?: string } | undefined)?.theme || "dark");
    document.documentElement.dataset.theme = theme;
  }, [cfg]);

  useEffect(() => {
    refreshInspect();
    const id = setInterval(refreshInspect, 4000);
    return () => clearInterval(id);
  }, [refreshInspect]);

  useEffect(() => {
    function onKey(ev: KeyboardEvent) {
      const key = ev.key.toLowerCase();
      if (ev.key === "Escape") {
        setOverlay("");
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === "k") {
        ev.preventDefault();
        setOverlay("palette");
        setPaletteQ("");
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === "enter") {
        ev.preventDefault();
        void runTask();
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === ".") {
        ev.preventDefault();
        void stopAgent();
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === ",") {
        ev.preventDefault();
        setOverlay("settings");
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === "p") {
        ev.preventDefault();
        setOverlay("project");
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === "l") {
        ev.preventDefault();
        promptRef.current?.focus();
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && key === "n") {
        ev.preventDefault();
        void newSession();
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && ev.shiftKey && key === "e") {
        ev.preventDefault();
        exportTranscript();
        return;
      }
      if (key === "?" && !["INPUT", "TEXTAREA"].includes((ev.target as HTMLElement)?.tagName || "")) {
        ev.preventDefault();
        setOverlay("help");
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const commands = useMemo(
    () => [
      { id: "run", label: "Run current task", run: () => void runTask() },
      { id: "stop", label: "Stop agent", run: () => void stopAgent() },
      { id: "settings", label: "Open settings", run: () => setOverlay("settings") },
      { id: "help", label: "Keyboard cheat sheet", run: () => setOverlay("help") },
      { id: "project", label: "Open project…", run: () => setOverlay("project") },
      { id: "session", label: "New session", run: () => void newSession() },
      { id: "resume", label: "Resume last task", run: () => void resumeLast() },
      { id: "export", label: "Export transcript", run: () => exportTranscript() },
      { id: "undo", label: "Undo last agent file changes", run: () => void undoChanges() },
      { id: "files", label: "Show file tree", run: () => setRight("files") },
      { id: "git", label: "Show git panel", run: () => setRight("git") },
      { id: "health", label: "Show health check", run: () => setRight("health") },
      { id: "skills", label: "Edit project skills", run: () => setRight("skills") },
    ],
    [task, workspace, sessionId, jobId, sessions],
  );

  function ingestEvent(row: EventRow) {
    setEvents((prev) => [...prev.slice(-300), row]);
    const payload = row.payload || {};
    if (row.type === "model.delta" && payload.text) {
      setChat((items) => {
        const last = items[items.length - 1];
        if (last && last.kind === "agent") return [...items.slice(0, -1), { kind: "agent", text: String(payload.text) }];
        return [...items, { kind: "agent", text: String(payload.text) }];
      });
    }
    if (row.type === "tool.started") {
      setChat((items) => [...items, { kind: "tool", tool: String(payload.tool || "tool"), text: JSON.stringify(payload.arguments || {}), live: true }]);
    }
    if (row.type === "tool.completed") {
      setChat((items) => {
        const next = [...items];
        const idx = [...next].reverse().findIndex((item) => item.kind === "tool" && item.tool === payload.tool && item.live);
        const real = idx >= 0 ? next.length - 1 - idx : -1;
        const text = String(payload.output_preview || payload.error || "");
        if (real >= 0 && next[real].kind === "tool") next[real] = { kind: "tool", tool: String(payload.tool), ok: Boolean(payload.success), text, live: false };
        else next.push({ kind: "tool", tool: String(payload.tool || "tool"), ok: Boolean(payload.success), text });
        return next;
      });
    }
    if (row.type === "plan.updated" || row.type === "agent.planning") {
      const plan = payload.plan as { steps?: { status: string; id: string; title: string }[] } | undefined;
      if (plan?.steps) {
        setPlanMd(plan.steps.map((s) => `${s.status === "done" ? "[x]" : s.status === "failed" ? "[!]" : "[ ]"} ${s.id} ${s.title}`).join("\n"));
      }
      if (Array.isArray(payload.todos)) setTodos(payload.todos as { title: string; status?: string }[]);
    }
    if (row.type === "todos.updated" && Array.isArray(payload.todos)) setTodos(payload.todos as { title: string }[]);
    if (row.type === "approval.requested") void api.approvals().then((data) => setApprovals(data.approvals));
    if (row.type === "agent.completed") {
      setSummary(String(payload.summary || ""));
      setUsage((payload.usage as Record<string, number>) || {});
      setBusy(false);
    }
  }

  async function runTask() {
    const text = composeTask();
    if (!text || busy) return;
    setError("");
    setBusy(true);
    setSummary("");
    setChat((items) => [...items, { kind: "user", text }]);
    try {
      const job = await api.startJob(text, workspace || undefined, sessionId || undefined);
      setJobId(job.id);
      setSessionId(job.session_id);
      sourceRef.current?.close();
      const source = new EventSource(`/api/jobs/${job.id}/events`);
      sourceRef.current = source;
      source.onmessage = (ev) => {
        try {
          const row = JSON.parse(ev.data) as EventRow & { type: string };
          if (row.type === "job.done") {
            const payload = row.payload as { summary?: string; usage?: Record<string, number>; result?: { summary: string; plan?: { steps: { status: string; id: string; title: string }[] } } };
            setSummary(payload.result?.summary || payload.summary || "");
            if (payload.usage) setUsage(payload.usage);
            if (payload.result?.plan?.steps) {
              setPlanMd(payload.result.plan.steps.map((s) => `${s.status === "done" ? "[x]" : "[ ]"} ${s.id} ${s.title}`).join("\n"));
            }
            setBusy(false);
            source.close();
            void refresh();
            void refreshInspect();
            return;
          }
          ingestEvent(row);
        } catch {
          /* keepalive */
        }
      };
      source.onerror = () => {
        source.close();
        void api.job(job.id).then((done) => {
          setSummary(done.summary || done.result?.summary || "");
          setBusy(false);
        });
      };
    } catch (err) {
      setError(String(err));
      setChat((items) => [...items, { kind: "agent", text: String(err) }]);
      setBusy(false);
    }
  }

  async function stopAgent() {
    try {
      if (jobId) await api.cancelJob(jobId);
      else await api.cancelCurrent(sessionId || undefined);
    } finally {
      setBusy(false);
      sourceRef.current?.close();
    }
  }

  function composeTask() {
    const extra = chips.length ? `\n\nAttached paths: ${chips.join(", ")}` : "";
    return (task.trim() + extra).trim();
  }

  async function newSession() {
    if (!workspace) return;
    const created = await api.createSession(workspace, "New session");
    setSessionId(created.id);
    setChat([]);
    setEvents([]);
    setSummary("");
    setTask("");
  }

  async function resumeLast() {
    const last = sessions.find((s) => s.id === sessionId) || sessions[0];
    if (!last) return;
    const detail = await api.session(last.id);
    const prompt = detail.tasks[0]?.prompt || "";
    setSessionId(last.id);
    setEvents(detail.events);
    setTask(prompt);
    if (prompt) {
      setTask(prompt);
      setTimeout(() => void runTask(), 0);
    }
  }

  async function openSession(id: string) {
    const detail = await api.session(id);
    setSessionId(id);
    setWorkspace(detail.workspace);
    setEvents(detail.events);
    setChat(
      detail.events
        .filter((e) => e.type === "agent.started" || e.type === "agent.completed" || e.type === "tool.completed")
        .map((e) => {
          if (e.type === "agent.started") return { kind: "user" as const, text: String(e.payload.task || "") };
          if (e.type === "tool.completed") return { kind: "tool" as const, tool: String(e.payload.tool || "tool"), ok: Boolean(e.payload.success), text: String(e.payload.output_preview || e.payload.error || "") };
          return { kind: "agent" as const, text: String(e.payload.summary || "") };
        }),
    );
    setSummary(detail.tasks[0]?.summary || "");
  }

  async function openFile(path: string, type: string) {
    if (type === "dir") {
      setDir(path);
      return;
    }
    const file = await api.file(path);
    setFileView(file.content);
    setFilePath(path);
    const diff = await api.gitDiff(path);
    setHunks(diff.hunks);
    setRight(diff.hunks.length ? "diff" : "files");
  }

  async function pickProject(path: string) {
    const opened = await api.openProject(path);
    setWorkspace(opened.path);
    setSessionId(opened.session_id);
    setOverlay("");
    setDir(".");
    await refresh();
    await refreshInspect();
  }

  function exportTranscript() {
    if (!sessionId) return;
    window.open(api.exportUrl(sessionId, "md"), "_blank");
  }

  async function undoChanges() {
    try {
      const result = await api.undo();
      setError("");
      setChat((items) => [...items, { kind: "agent", text: `Restored ${result.restored.length} file(s).` }]);
      await refreshInspect();
    } catch (err) {
      setError(String(err));
    }
  }

  async function onDrop(ev: React.DragEvent) {
    ev.preventDefault();
    const next: string[] = [];
    for (const file of Array.from(ev.dataTransfer.files)) {
      const text = await file.text().catch(() => "");
      if (text) {
        const saved = await api.attach(file.name, text);
        next.push(saved.path);
      } else {
        next.push(file.name);
      }
    }
    const text = ev.dataTransfer.getData("text/plain").trim();
    if (text && !text.includes("\n") && (text.startsWith("/") || text.startsWith("."))) next.push(text);
    setChips((prev) => [...prev, ...next]);
  }

  const filteredCommands = commands.filter((c) => c.label.toLowerCase().includes(paletteQ.toLowerCase()));
  const tokens = usage.total_tokens || 0;

  if (!ready) return <div className="boot">SHADOW AGENT</div>;

  if (needsOnboard) {
    return <Onboarding onDone={() => { setNeedsOnboard(false); void refresh(); }} />;
  }

  return (
    <div className="app" onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <header className="top">
        <div className="brand">
          <img className="mark" src="/icon.svg" alt="" width={30} height={30} />
          <div>
            <p className="kicker">SHADOWFETCH SUITE</p>
            <h1>Shadow Agent</h1>
          </div>
        </div>
        <div className="meta">
          <button className="chip" onClick={() => setOverlay("project")} title="Switch project">
            {workspace || "Pick a project"}
          </button>
          <span className="chip">{status.model.provider}/{status.model.default}</span>
          <span className="chip">{status.permissions.level}</span>
          {tokens > 0 && <span className="chip ok">{tokens} tok</span>}
        </div>
        <div className="top-actions">
          <div className={`live ${busy ? "on" : ""}`}><i />{busy ? "RUNNING" : "IDLE"}</div>
          <button className="icon-btn" onClick={() => setOverlay("palette")}>⌘K</button>
          <button className="icon-btn" onClick={() => setOverlay("settings")}>Settings</button>
          <button className="icon-btn" onClick={() => setOverlay("help")}>?</button>
        </div>
      </header>

      <aside className="left">
        <div className="panel-h">
          <span>SESSIONS</span>
          <button className="icon-btn" onClick={() => void newSession()}>New</button>
        </div>
        <div className="scroll">
          {sessions.length === 0 && <Empty title="No sessions yet" body="Run a task. It will show up here so you can resume it." />}
          {sessions.map((s) => (
            <div key={s.id} className={`item ${s.id === sessionId ? "active" : ""}`} onClick={() => void openSession(s.id)}>
              <strong>{s.title || "Untitled"}</strong>
              <span>{s.status} · {s.workspace.split("/").pop()}</span>
            </div>
          ))}
        </div>
        <div className="panel-h">RECENT FOLDERS</div>
        <div className="scroll">
          {projects.length === 0 && <div className="item">Open a folder to pin it here.</div>}
          {projects.map((p) => (
            <div key={p.id} className={`item ${p.path === workspace ? "active" : ""}`} onClick={() => void pickProject(p.path)}>
              <strong>{p.name}</strong>
              <span>{p.path}</span>
            </div>
          ))}
        </div>
        <div className="panel-h">MODELS</div>
        <div className="scroll">
          {models.map((m) => (
            <div key={m.id} className={`item ${m.id === status.model.default ? "active" : ""}`}>
              <strong>{m.name}</strong>
              <span>{m.provider}</span>
            </div>
          ))}
        </div>
      </aside>

      <main className="center">
        <div className="panel-h">
          <span>WORKSPACE</span>
          <div className="tabs">
            {(["conversation", "plan", "tools"] as CenterTab[]).map((tab) => (
              <button key={tab} className={center === tab ? "on" : ""} onClick={() => setCenter(tab)}>{tab}</button>
            ))}
          </div>
        </div>
        {approvals.map((a) => (
          <div className="approval" key={a.id}>
            <h3>Approve a dangerous command</h3>
            <pre className="code">{a.command || a.reason}</pre>
            <p className="hint">{a.reason}</p>
            <div className="row">
              <button className="primary" onClick={() => void api.decide(a.id, "approve").then(() => refreshInspect())}>Approve</button>
              <button className="danger" onClick={() => void api.decide(a.id, "deny").then(() => refreshInspect())}>Deny</button>
            </div>
          </div>
        ))}
        {error && <div className="approval"><strong>Could not do that.</strong><p>{error}</p></div>}
        <div className="scroll">
          {center === "conversation" && (
            <>
              {chat.length === 0 && !summary && (
                <Empty
                  title="Describe a coding task"
                  body="The harness will inspect, plan, edit, and verify. Try “Create a Python hello-world project” — it works offline with Mock."
                />
              )}
              {chat.map((item, i) =>
                item.kind === "tool" ? (
                  <div key={i} className={`tool-card ${item.ok === false ? "bad" : ""}`}>
                    <header><span>{item.tool}{item.live ? " · running" : ""}</span><span>{item.ok === false ? "failed" : item.ok ? "ok" : ""}</span></header>
                    <pre>{item.text.slice(0, 1200)}</pre>
                  </div>
                ) : (
                  <div key={i} className={`msg ${item.kind === "user" ? "user" : ""}`}>
                    <div className="who">{item.kind === "user" ? "YOU" : "AGENT"}</div>
                    <pre>{item.text}</pre>
                  </div>
                ),
              )}
              {summary && (
                <div className="msg">
                  <div className="who">RESULT</div>
                  <pre>{summary}</pre>
                </div>
              )}
            </>
          )}
          {center === "plan" && (
            <>
              {todos.length > 0 && (
                <div className="msg">
                  <div className="who">TODOS</div>
                  <pre>{todos.map((t) => `${t.status === "done" ? "[x]" : "[ ]"} ${t.title}`).join("\n")}</pre>
                </div>
              )}
              <div className="plan">{planMd || "No plan yet. Run a task and the harness will publish steps here."}</div>
            </>
          )}
          {center === "tools" &&
            (events.filter((e) => e.type.startsWith("tool.") || e.type.startsWith("test.")).length === 0 ? (
              <Empty title="No tool calls yet" body="When the agent reads, edits, or runs commands, live cards appear here." />
            ) : (
              events.filter((e) => e.type.startsWith("tool.") || e.type.startsWith("test.")).slice(-50).map((e, i) => (
                <div key={i} className={`event ${String(e.payload.success) === "false" || e.type.includes("fail") ? "bad" : "ok"}`}>
                  {e.type} · {JSON.stringify(e.payload).slice(0, 200)}
                </div>
              ))
            ))}
        </div>
        <form className="composer" onSubmit={(ev) => { ev.preventDefault(); void runTask(); }}>
          {chips.length > 0 && (
            <div className="chips">
              {chips.map((c) => (
                <button type="button" className="path-chip" key={c} onClick={() => setChips(chips.filter((x) => x !== c))}>{c} ×</button>
              ))}
            </div>
          )}
          <div className="composer-row">
            <textarea
              ref={promptRef}
              value={task}
              onChange={(ev) => setTask(ev.target.value)}
              onKeyDown={(ev) => {
                if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
                  ev.preventDefault();
                  void runTask();
                }
              }}
              placeholder="Task for the agent loop… Drop files or paths here. Ctrl+Enter to run."
            />
            {busy ? (
              <button type="button" className="danger" onClick={() => void stopAgent()}>Stop</button>
            ) : (
              <button type="submit" className="primary">Run</button>
            )}
          </div>
          <div className="row">
            <button type="button" className="ghost" onClick={() => void resumeLast()}>Resume last</button>
            <button type="button" className="ghost" onClick={() => void undoChanges()}>Undo files</button>
            <button type="button" className="ghost" onClick={exportTranscript}>Export</button>
            <span className="hint">Shift+Enter for a new line · ? for shortcuts</span>
          </div>
        </form>
      </main>

      <aside className="right">
        <div className="panel-h">
          <span>INSPECT</span>
          <div className="tabs">
            {(["files", "diff", "git", "skills", "health"] as RightTab[]).map((tab) => (
              <button key={tab} className={right === tab ? "on" : ""} onClick={() => setRight(tab)}>{tab}</button>
            ))}
          </div>
        </div>
        <div className="scroll">
          {right === "files" && (
            <>
              <div className="item" onClick={() => setDir(dir === "." ? "." : dir.split("/").slice(0, -1).join("/") || ".")}>
                <strong>{dir}</strong>
                <span>click to go up</span>
              </div>
              {files.length === 0 && <Empty title="Empty folder" body="This directory has no visible files." />}
              {files.map((f) => (
                <div key={f.path} className="file" onClick={() => void openFile(f.path, f.type)}>
                  <strong>{f.type === "dir" ? "▸" : "·"} {f.name}</strong>
                </div>
              ))}
              {fileView && <pre className="plan">{fileView.slice(0, 8000)}</pre>}
            </>
          )}
          {right === "diff" && (
            hunks.length === 0 ? (
              <Empty title="No diff" body="Open a changed file or run the agent. Git hunks will land here." />
            ) : (
              hunks.map((h, i) => (
                <div key={i} className="tool-card">
                  <header>{h.header}</header>
                  {h.lines.map((line, j) => (
                    <div key={j} className={`diff-line ${line.kind === "add" ? "diff-add" : line.kind === "del" ? "diff-del" : "diff-ctx"}`}>
                      {(line.kind === "add" ? "+" : line.kind === "del" ? "-" : " ") + line.text}
                    </div>
                  ))}
                </div>
              ))
            )
          )}
          {right === "git" && (
            git.repo ? (
              <>
                <pre className="plan">{git.status || "clean"}</pre>
                {git.files.map((f) => (
                  <div key={f.path} className="file" onClick={() => void openFile(f.path, "file")}>
                    <strong>{f.label} {f.path}</strong>
                  </div>
                ))}
                <pre className="plan">{git.log}</pre>
                <div className="field">
                  <label>Commit message</label>
                  <input value={commitMsg} onChange={(e) => setCommitMsg(e.target.value)} placeholder="Describe the change" />
                </div>
                <div className="row">
                  <button className="ghost" onClick={() => void api.gitAdd(["."]).then(refreshInspect)}>Stage all</button>
                  <button className="primary" onClick={() => commitMsg && void api.gitCommit(commitMsg).then(() => { setCommitMsg(""); void refreshInspect(); })}>Commit</button>
                </div>
              </>
            ) : (
              <Empty title="Not a git repo" body="Initialize git in this folder to use status, diff, and commit." />
            )
          )}
          {right === "skills" && (
            <>
              <p className="hint">Project instructions and skills live in .shadow/ and are injected into every run.</p>
              <div className="field">
                <label>.shadow/instructions.md</label>
                <textarea rows={8} value={instructions} onChange={(e) => setInstructions(e.target.value)} />
                <button className="ghost" onClick={() => void api.saveInstructions(instructions)}>Save instructions</button>
              </div>
              <div className="field">
                <label>Skill name</label>
                <input value={skillName} onChange={(e) => setSkillName(e.target.value)} />
                <textarea rows={6} value={skillBody} onChange={(e) => setSkillBody(e.target.value)} placeholder="# How to run tests…" />
                <button className="ghost" onClick={() => void api.saveSkill(skillName, skillBody).then(() => refreshInspect())}>Save skill</button>
              </div>
              {skills.map((s) => (
                <div key={s.name} className="item" onClick={() => { setSkillName(s.name); setSkillBody(s.content); }}>
                  <strong>{s.name}</strong>
                </div>
              ))}
            </>
          )}
          {right === "health" && health && (
            <>
              <div className="status-row"><span>version</span><code>{health.version}</code></div>
              <div className="status-row"><span>workspace</span><code>{health.workspace}</code></div>
              <div className="status-row"><span>provider</span><code className={health.provider?.ok ? "health-ok" : "health-bad"}>{health.provider?.name} — {health.provider?.detail}</code></div>
              {Object.entries(health.tools || {}).map(([name, info]) => (
                <div className="status-row" key={name}>
                  <span>{name}</span>
                  <code className={info.ok ? "health-ok" : "health-bad"}>{info.ok ? info.detail || "yes" : "not found"}</code>
                </div>
              ))}
            </>
          )}
        </div>
      </aside>

      {overlay === "settings" && (
        <Settings
          cfg={cfg}
          apiKey={settingsKey}
          onKey={setSettingsKey}
          onClose={() => setOverlay("")}
          onSave={async (values, key) => {
            await api.saveConfig(values, key, String((values.model as { api_key_env?: string } | undefined)?.api_key_env || ""));
            setOverlay("");
            await refresh();
          }}
        />
      )}
      {overlay === "help" && <Help onClose={() => setOverlay("")} />}
      {overlay === "palette" && (
        <Palette query={paletteQ} onQuery={setPaletteQ} items={filteredCommands} onClose={() => setOverlay("")} />
      )}
      {overlay === "project" && (
        <ProjectPicker projects={projects} current={workspace} onClose={() => setOverlay("")} onPick={(path) => void pickProject(path)} />
      )}
    </div>
  );
}

function Empty({ title, body }: { title: string; body: string }) {
  return (
    <div className="empty">
      <h3>{title}</h3>
      <p>{body}</p>
    </div>
  );
}

function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [workspace, setWorkspace] = useState("");
  const [provider, setProvider] = useState("mock");
  const [apiKey, setApiKey] = useState("");
  const [level, setLevel] = useState("workspace");
  const [error, setError] = useState("");
  const [providers, setProviders] = useState<{ id: string; needs_key?: boolean; api_key_env?: string }[]>([]);

  useEffect(() => {
    api.onboarding().then((data) => {
      setWorkspace(data.suggested_workspace);
      setProviders(data.providers);
    });
  }, []);

  async function finish() {
    try {
      await api.completeOnboarding({ workspace, provider, api_key: apiKey, permission_level: level, theme: "dark" });
      onDone();
    } catch (err) {
      setError(String(err));
    }
  }

  const needsKey = providers.find((p) => p.id === provider)?.needs_key;
  return (
    <div className="modal-back">
      <div className="wizard">
        <p className="kicker">SHADOW AGENT</p>
        <h2>Get running in under a minute</h2>
        <div className="steps">{[0, 1, 2, 3].map((n) => <span key={n} className={n <= step ? "on" : ""} />)}</div>
        {step === 0 && (
          <div className="field">
            <label>Project folder</label>
            <input value={workspace} onChange={(e) => setWorkspace(e.target.value)} placeholder="/home/you/src/my-app" />
            <p className="hint">This is the sandbox. The agent cannot write outside it.</p>
          </div>
        )}
        {step === 1 && (
          <div className="field">
            <label>Model provider</label>
            <select value={provider} onChange={(e) => setProvider(e.target.value)}>
              {providers.map((p) => <option key={p.id} value={p.id}>{p.id}</option>)}
            </select>
            <p className="hint">Mock works offline with no API key. Switch later in Settings.</p>
          </div>
        )}
        {step === 2 && (
          <div className="field">
            <label>API key {needsKey ? "" : "(optional)"}</label>
            <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder={needsKey ? "Paste key, or leave blank if already in the environment" : "Not needed for Mock"} />
            <p className="hint">Saved only in ~/.config/shadow-agent/secrets.env (mode 600). Never written into YAML or git.</p>
          </div>
        )}
        {step === 3 && (
          <div className="field">
            <label>Permission level</label>
            <select value={level} onChange={(e) => setLevel(e.target.value)}>
              <option value="read_only">Read only — inspect only</option>
              <option value="workspace">Workspace — edit and run (recommended)</option>
              <option value="elevated">Elevated — dangerous commands after you approve</option>
            </select>
          </div>
        )}
        {error && <p className="health-bad">{error}</p>}
        <div className="row">
          {step > 0 && <button className="ghost" onClick={() => setStep(step - 1)}>Back</button>}
          {step < 3 && <button className="primary" onClick={() => setStep(step + 1)}>Next</button>}
          {step === 3 && <button className="primary" onClick={() => void finish()}>Start Shadow Agent</button>}
        </div>
      </div>
    </div>
  );
}

function Settings({
  cfg,
  apiKey,
  onKey,
  onClose,
  onSave,
}: {
  cfg: Record<string, unknown>;
  apiKey: string;
  onKey: (v: string) => void;
  onClose: () => void;
  onSave: (values: Record<string, unknown>, key: string) => Promise<void>;
}) {
  const model = (cfg.model || {}) as Record<string, string>;
  const permissions = (cfg.permissions || {}) as Record<string, string | boolean>;
  const ui = (cfg.ui || {}) as Record<string, string | number>;
  const [defaultModel, setDefaultModel] = useState(model.default || "mock");
  const [provider, setProvider] = useState(model.provider || "mock");
  const [endpoint, setEndpoint] = useState(model.endpoint || "");
  const [name, setName] = useState(model.name || "");
  const [keyEnv, setKeyEnv] = useState(model.api_key_env || "OPENAI_API_KEY");
  const [level, setLevel] = useState(String(permissions.level || "workspace"));
  const [theme, setTheme] = useState(String(ui.theme || "dark"));
  return (
    <div className="modal-back" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Settings</h2>
        <p className="hint">These write ~/.config/shadow-agent/config.yaml. Keys stay in secrets.env.</p>
        <div className="field"><label>Default model</label><input value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} /></div>
        <div className="field"><label>Provider</label>
          <select value={provider} onChange={(e) => setProvider(e.target.value)}>
            {["mock", "openai_compatible", "ollama", "local", "llamacpp", "vllm"].map((p) => <option key={p}>{p}</option>)}
          </select>
        </div>
        <div className="field"><label>Endpoint</label><input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://api.x.ai/v1" /></div>
        <div className="field"><label>Model name</label><input value={name} onChange={(e) => setName(e.target.value)} /></div>
        <div className="field"><label>API key env var</label><input value={keyEnv} onChange={(e) => setKeyEnv(e.target.value)} /></div>
        <div className="field"><label>Paste API key</label><input type="password" value={apiKey} onChange={(e) => onKey(e.target.value)} placeholder="leave blank to keep the current secret" /></div>
        <div className="field"><label>Permissions</label>
          <select value={level} onChange={(e) => setLevel(e.target.value)}>
            <option value="read_only">read_only</option>
            <option value="workspace">workspace</option>
            <option value="elevated">elevated</option>
          </select>
        </div>
        <div className="field"><label>Theme</label>
          <select value={theme} onChange={(e) => setTheme(e.target.value)}>
            <option value="dark">dark</option>
            <option value="dim">dim</option>
          </select>
        </div>
        <div className="row">
          <button className="primary" onClick={() => void onSave({
            model: { default: defaultModel, provider, endpoint, name, api_key_env: keyEnv },
            permissions: { level },
            ui: { theme },
          }, apiKey)}>Save</button>
          <button className="ghost" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}

function Help({ onClose }: { onClose: () => void }) {
  return (
    <div className="modal-back" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Keyboard cheat sheet</h2>
        <div className="help-grid">
          {SHORTCUTS.map(([k, label]) => (
            <><span key={k}>{label}</span><span className="kbd">{k}</span></>
          ))}
        </div>
        <p className="hint">Shadow Agent is a harness: the model is replaceable. Mock works with no credits.</p>
        <button className="ghost" onClick={onClose}>Close</button>
      </div>
    </div>
  );
}

function Palette({ query, onQuery, items, onClose }: { query: string; onQuery: (v: string) => void; items: { id: string; label: string; run: () => void }[]; onClose: () => void }) {
  return (
    <div className="modal-back" onClick={onClose}>
      <div className="palette" onClick={(e) => e.stopPropagation()}>
        <input autoFocus value={query} onChange={(e) => onQuery(e.target.value)} placeholder="Type a command…" />
        {items.map((item, i) => (
          <button key={item.id} className={`hit ${i === 0 ? "on" : ""}`} onClick={() => { item.run(); onClose(); }}>{item.label}</button>
        ))}
        {items.length === 0 && <div className="item">No matching commands.</div>}
      </div>
    </div>
  );
}

function ProjectPicker({ projects, current, onClose, onPick }: { projects: Project[]; current: string; onClose: () => void; onPick: (path: string) => void }) {
  const [path, setPath] = useState(current);
  return (
    <div className="modal-back" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Open a project</h2>
        <div className="field">
          <label>Folder path</label>
          <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="/home/you/src/app" />
        </div>
        <div className="row">
          <button className="primary" onClick={() => onPick(path)}>Open</button>
          <button className="ghost" onClick={onClose}>Cancel</button>
        </div>
        <div className="panel-h">RECENT</div>
        {projects.map((p) => (
          <div key={p.id} className="item" onClick={() => onPick(p.path)}>
            <strong>{p.name}</strong>
            <span>{p.path}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
