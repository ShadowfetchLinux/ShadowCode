import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Approval,
  type DetectedProvider,
  type DiffHunk,
  type EventRow,
  type ExecResult,
  type FileEntry,
  type Health,
  type ModelInfo,
  type Project,
  type Session,
} from "./api";

type CenterTab = "conversation" | "plan" | "tools";
type RightTab = "files" | "diff" | "git" | "skills" | "health";
type Overlay = "" | "settings" | "help" | "palette" | "project";
type Toast = { id: number; text: string; kind: "ok" | "err" | "info" };

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

let toastSeq = 1;

export default function App() {
  const [ready, setReady] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
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
  const [status, setStatus] = useState({ workspace: "", model: { default: "mock", provider: "mock", name: "" }, permissions: { level: "workspace" } });
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
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [modelChoice, setModelChoice] = useState("");
  const [mode, setMode] = useState("coder");
  const [workLocal, setWorkLocal] = useState(false);
  const [trustReq, setTrustReq] = useState<{ path: string; name?: string; permissions?: Record<string, unknown> } | null>(null);
  const [testing, setTesting] = useState<Record<string, boolean>>({});
  const [execResult, setExecResult] = useState<ExecResult | null>(null);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const sourceRef = useRef<EventSource | null>(null);

  const pushToast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = toastSeq++;
    setToasts((prev) => [...prev.slice(-3), { id, text, kind }]);
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 6000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [healthData, onboard, modelData, projectData, sessionData, statusData, cfgData, detectData] = await Promise.all([
        api.health(),
        api.onboarding(),
        api.models(),
        api.projects(),
        api.sessions(),
        api.status(),
        api.config(),
        api.detectProviders(),
      ]);
      setHealth(healthData);
      setNeedsOnboard(!onboard.completed);
      setWorkspace(healthData.workspace || statusData.workspace);
      setModels(modelData.models);
      setDetected(detectData.providers);
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

  const refreshDiff = useCallback(async () => {
    if (!filePath) return;
    try {
      const diff = await api.gitDiff(filePath);
      setHunks(diff.hunks);
    } catch {
      /* not a repo */
    }
  }, [filePath]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const theme = String((cfg.ui as { theme?: string } | undefined)?.theme || "light");
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
        setTrustReq(null);
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
    if (row.type === "model.retry") {
      pushToast(`Model busy — retry ${payload.attempt}/${payload.max_attempts} in ${payload.wait_sec}s`, "info");
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
    // Clear the composer immediately so Enter does not leave the submitted
    // prompt behind. The captured `text` is already in flight; clearing here
    // does not affect the running job. Shift+Enter newlines are unaffected
    // because they never enter this branch.
    setTask("");
    setChips([]);
    try {
      const job = await api.startJob(text, workspace || undefined, sessionId || undefined, modelChoice || undefined, mode);
      setJobId(job.id);
      setSessionId(job.session_id);
      sourceRef.current?.close();
      const source = new EventSource(`/api/jobs/${job.id}/events`);
      sourceRef.current = source;
      source.onmessage = (ev) => {
        try {
          const row = JSON.parse(ev.data) as EventRow & { type: string };
          if (row.type === "job.done") {
            const payload = row.payload as { summary?: string; usage?: Record<string, number>; status?: string; result?: { summary: string; plan?: { steps: { status: string; id: string; title: string }[] } } };
            setSummary(payload.result?.summary || payload.summary || "");
            if (payload.usage) setUsage(payload.usage);
            if (payload.result?.plan?.steps) {
              setPlanMd(payload.result.plan.steps.map((s) => `${s.status === "done" ? "[x]" : "[ ]"} ${s.id} ${s.title}`).join("\n"));
            }
            setBusy(false);
            source.close();
            pushToast(payload.status === "completed" ? "Task complete" : `Task ended: ${payload.status || "failed"}`, payload.status === "completed" ? "ok" : "err");
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
      pushToast(String(err), "err");
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
    if (opened.needs_trust) {
      setTrustReq({ path: opened.path, name: opened.name, permissions: opened.permissions });
      setOverlay("");
      return;
    }
    setWorkspace(opened.path);
    setSessionId(opened.session_id);
    setOverlay("");
    setDir(".");
    await refresh();
    await refreshInspect();
  }

  async function confirmTrust() {
    if (!trustReq) return;
    try {
      const opened = await api.trustProject(trustReq.path);
      setTrustReq(null);
      setWorkspace(opened.path);
      setSessionId(opened.session_id);
      setDir(".");
      pushToast(`Trusted ${opened.path}`, "ok");
      await refresh();
      await refreshInspect();
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  async function useModel(m: ModelInfo) {
    try {
      await api.selectModel(m.id);
      pushToast(`Default model → ${m.id}`, "ok");
      await refresh();
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  async function testModel(m: ModelInfo) {
    setTesting((prev) => ({ ...prev, [m.id]: true }));
    try {
      const result = await api.testModel({
        provider: m.provider,
        name: String(m.metadata?.model || m.id),
        endpoint: m.endpoint,
        api_key_env: String(m.metadata?.api_key_env || ""),
      });
      if (result.ok) pushToast(`${m.id}: OK in ${result.latency_ms}ms — “${(result.reply || "").slice(0, 60)}”`, "ok");
      else pushToast(`${m.id}: ${result.error || "test failed"}`, "err");
    } catch (err) {
      pushToast(String(err), "err");
    } finally {
      setTesting((prev) => ({ ...prev, [m.id]: false }));
    }
  }

  async function rerunCommand(command: string) {
    pushToast(`Running: ${command}`, "info");
    try {
      const result = await api.exec(command);
      setExecResult(result);
      setRight("files");
      setCenter("tools");
      pushToast(result.ok ? `exit ${result.exit_code}: ${command}` : `failed (${result.exit_code}): ${command}`, result.ok ? "ok" : "err");
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  async function hunkAction(hunk: DiffHunk, action: "accept" | "reject") {
    if (!filePath) return;
    try {
      await api.hunkAction(filePath, hunk, action);
      pushToast(action === "accept" ? `Staged hunk in ${filePath}` : `Reverted hunk in ${filePath}`, "ok");
      await refreshDiff();
      await refreshInspect();
    } catch (err) {
      pushToast(String(err), "err");
    }
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
      pushToast(String(err), "err");
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
  const localProvider = ["mock", "ollama", "local", "llamacpp", "vllm"].includes(status.model.provider);
  const runningProviders = new Set(detected.filter((d) => d.running).map((d) => d.provider));

  if (!ready) {
    return (
      <div className="boot">
        <div>SHADOW AGENT</div>
        <div className="skel-rows"><span className="skel" /><span className="skel" /><span className="skel" /></div>
      </div>
    );
  }

  if (needsOnboard) {
    return <Onboarding onDone={() => { setNeedsOnboard(false); void refresh(); }} />;
  }

  return (
    <div className="app" onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <header className="top">
        <div className="top-nav">
          <button className="icon-btn" title="Back" onClick={() => void resumeLast()}>‹</button>
          <button className="icon-btn" title="Forward" onClick={() => void newSession()}>›</button>
          <button className="icon-btn" title="New message" onClick={() => { setTask(""); promptRef.current?.focus(); }}>＋</button>
        </div>
        <div className="top-title">{task.trim() || summary || "Shadow Agent"}</div>
        <div className="top-actions">
          <button className="icon-btn" onClick={() => setOverlay("palette")} title="Command palette (Ctrl+K)">⌘K</button>
          <button className="icon-btn" onClick={() => setOverlay("settings")} title="Settings (Ctrl+,)">⚙</button>
          <button className="icon-btn" onClick={() => setOverlay("help")} title="Keyboard cheat sheet (?)">?</button>
        </div>
      </header>

      <div className="statusline">
        <span>{status.model.provider}/{status.model.name || status.model.default}</span>
        <span className="sep">·</span>
        <span title="Workspace">{workspace ? workspace.split("/").pop() : "no workspace"}</span>
        <span className="sep">·</span>
        <span title="Permission level">{status.permissions.level}</span>
        {tokens > 0 && (<><span className="sep">·</span><span title="Tokens this task">{tokens} tok{localProvider ? " · $0 local" : ""}</span></>)}
        <span className="sep">·</span>
        <span className={`live ${busy ? "on" : ""}`}><i />{busy ? "working" : "idle"}</span>
        <span style={{ marginLeft: "auto" }} className="hint">Enter to run · Shift+Enter for a new line · ? for shortcuts</span>
      </div>

      <aside className="left">
        <div className="panel-h">
          <span>SESSIONS</span>
          <button className="icon-btn" onClick={() => void newSession()}>New</button>
        </div>
        <div className="scroll">
          {sessions.length === 0 && <Empty title="No sessions yet" body="Run a task. It will show up here so you can resume it." />}
          {sessions.map((s) => (
            <div key={s.id} className={`item ${s.id === sessionId ? "active" : ""}`} onClick={() => void openSession(s.id)}>
              <strong>{s.title || "Untitled"}{s.parent_id ? " ↳" : ""}</strong>
              <span>
                {s.status} · {s.workspace.split("/").pop()}
                <button className="mini" title="Fork this session to try a path" onClick={(e) => { e.stopPropagation(); void api.branchSession(s.id).then((b) => { pushToast(`Branched → ${b.id.slice(0,8)}`, "ok"); void refresh(); }); }}>Branch</button>
              </span>
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
        <div className="panel-h">
          <span>MODELS</span>
          <button className="icon-btn" title="Re-detect local servers" onClick={() => void api.models(true).then((d) => setModels(d.models)).then(() => api.detectProviders(true)).then((d) => setDetected(d.providers))}>↻</button>
        </div>
        <div className="scroll">
          {models.length === 0 && <div className="skel-rows"><span className="skel" /><span className="skel" /></div>}
          {models.map((m) => {
            const caps = (m.metadata?.capabilities || {}) as Record<string, boolean>;
            const isActive = m.id === status.model.default || m.id === status.model.name;
            const live = Boolean(m.detected) && (m.metadata?.detected === true || runningProviders.has(m.provider));
            return (
              <div key={m.id} className={`item model-item ${isActive ? "active" : ""}`} onClick={() => void useModel(m)} title="Click to make default">
                <strong>
                  {live && <span className="dot-live" />}
                  {m.name}
                </strong>
                <span>
                  {m.provider}
                  {caps.tools ? " · tools" : ""}
                  {caps.thinking ? " · thinking" : ""}
                  {m.metadata?.detail ? ` · ${m.metadata.detail}` : ""}
                </span>
                <span className="model-actions">
                  <button
                    className="mini"
                    disabled={Boolean(testing[m.id])}
                    onClick={(e) => { e.stopPropagation(); void testModel(m); }}
                  >
                    {testing[m.id] ? "…" : "Test"}
                  </button>
                  {!isActive && <button className="mini" onClick={(e) => { e.stopPropagation(); void useModel(m); }}>Use</button>}
                </span>
              </div>
            );
          })}
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
            <div className="cat"><i />{a.tool || "Permission"}</div>
            <h3>Allow Shadow to run this command?</h3>
            <pre className="code">{a.command || a.reason}</pre>
            <p className="hint">{a.reason}</p>
            <div className="row">
              <button className="ghost" onClick={() => void api.decide(a.id, "deny").then(() => refreshInspect())}>Cancel <span className="kbd">Esc</span></button>
              <button className="primary" onClick={() => void api.decide(a.id, "approve").then(() => refreshInspect())}>Allow <span className="kbd-hint">↵</span></button>
            </div>
          </div>
        ))}
        {error && <div className="approval"><strong>Could not do that.</strong><p>{error}</p></div>}
        <div className="chat-stream">
          <div className="chat-inner">
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
                ) : item.kind === "user" ? (
                  <div key={i} className="msg-user">
                    <div className="user-pill">
                      <div className="tag"><i />Task</div>
                      <div className="bubble">{item.text}</div>
                    </div>
                  </div>
                ) : (
                  <div key={i} className="msg-agent">
                    <div className="who">Agent</div>
                    {item.text}
                  </div>
                ),
              )}
              {busy && (
                <div className="msg-agent">
                  <div className="who">Agent</div>
                  <div className="skel-rows"><span className="skel" /><span className="skel short" /></div>
                </div>
              )}
              {summary && (
                <div className="msg-agent">
                  <div className="who">Result</div>
                  {summary}
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
          {center === "tools" && (
            <>
              {execResult && (
                <div className={`tool-card ${execResult.ok ? "" : "bad"}`}>
                  <header><span>rerun · {execResult.command}</span><span>exit {execResult.exit_code}</span></header>
                  <pre>{(execResult.stdout + (execResult.stderr ? "\n" + execResult.stderr : "")).slice(0, 2000) || "(no output)"}</pre>
                </div>
              )}
              {events.filter((e) => e.type.startsWith("tool.") || e.type.startsWith("test.")).length === 0 ? (
                <Empty title="No tool calls yet" body="When the agent reads, edits, or runs commands, live cards appear here." />
              ) : (
                events.filter((e) => e.type.startsWith("tool.") || e.type.startsWith("test.")).slice(-50).map((e, i) => {
                  const cmd = e.type === "tool.completed" && e.payload.tool === "exec" ? String((e.payload as { command?: string }).command || "") : "";
                  return (
                    <div key={i} className={`event ${String(e.payload.success) === "false" || e.type.includes("fail") ? "bad" : "ok"}`}>
                      <span>{e.type} · {JSON.stringify(e.payload).slice(0, 200)}</span>
                      {cmd && <button className="mini" onClick={() => void rerunCommand(cmd)}>Rerun</button>}
                    </div>
                  );
                })
              )}
            </>
          )}
          </div>
        </div>
        <div className="composer-wrap">
        <form className="composer" onSubmit={(ev) => { ev.preventDefault(); void runTask(); }}>
          {chips.length > 0 && (
            <div className="chips">
              {chips.map((c) => (
                <button type="button" className="path-chip" key={c} onClick={() => setChips(chips.filter((x) => x !== c))}>{c} ×</button>
              ))}
            </div>
          )}
          <button type="button" className="plus-btn" title="Attach file or path (drop files here)" onClick={() => promptRef.current?.focus()}>＋</button>
          <textarea
            ref={promptRef}
            value={task}
            onChange={(ev) => setTask(ev.target.value)}
            onKeyDown={(ev) => {
              if (ev.key === "Enter" && !ev.shiftKey && !ev.nativeEvent.isComposing) {
                ev.preventDefault();
                void runTask();
              }
            }}
            placeholder="Ask for follow-up changes…"
          />
          <div className="composer-controls">
            <select
              className="model-select"
              value={modelChoice}
              onChange={(e) => setModelChoice(e.target.value)}
              title="Model for this task only"
            >
              <option value="">model: {status.model.name || status.model.default}</option>
              {models.map((m) => (
                <option key={m.id} value={m.id}>{m.id}</option>
              ))}
            </select>
            <select
              className="mode-select"
              value={mode}
              onChange={(e) => setMode(e.target.value)}
              title="Agent mode (purpose)"
            >
              <option value="coder">coder</option>
              <option value="researcher">researcher</option>
              <option value="reviewer">reviewer</option>
              <option value="tester">tester</option>
            </select>
            <button type="button" className="mic-btn" title="Voice input (not wired)">🎙</button>
            {busy ? (
              <button type="button" className="submit-btn stop" title="Stop (Ctrl+.)" onClick={() => void stopAgent()}>■</button>
            ) : (
              <button type="submit" className="submit-btn" title="Run (Enter)">↑</button>
            )}
          </div>
        </form>
        <div className="composer-foot">
          <label title="Prefer the local model / keep work on this machine">
            <input type="checkbox" checked={workLocal} onChange={(e) => setWorkLocal(e.target.checked)} />
            Work locally
          </label>
          <button type="button" className="more-btn" onClick={() => void resumeLast()} title="Resume last task">↻ Resume</button>
          <button type="button" className="more-btn" onClick={() => void undoChanges()} title="Undo last agent file changes">↶ Undo files</button>
          <button type="button" className="more-btn" onClick={exportTranscript} title="Export transcript">⤓ Export</button>
          <span style={{ marginLeft: "auto" }}>Enter to run · Shift+Enter for a new line</span>
        </div>
        </div>
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
                  <header>
                    <span>{h.header}</span>
                    <span className="model-actions">
                      <button className="mini" title="Stage just this hunk" onClick={() => void hunkAction(h, "accept")}>Accept</button>
                      <button className="mini danger-text" title="Revert this hunk in the worktree" onClick={() => void hunkAction(h, "reject")}>Reject</button>
                    </span>
                  </header>
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
              {detected.filter((d) => d.running).map((d) => (
                <div className="status-row" key={d.provider}>
                  <span>{d.label}</span>
                  <code className="health-ok">{d.models.length} models · {d.latency_ms}ms</code>
                </div>
              ))}
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

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`} onClick={() => setToasts((prev) => prev.filter((x) => x.id !== t.id))}>
            {t.text}
          </div>
        ))}
      </div>

      {trustReq && (
        <div className="modal-back" onClick={() => setTrustReq(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>Trust this folder?</h2>
            <p className="hint">
              <code>{trustReq.path}</code> becomes the agent sandbox. The agent can read, edit, and run commands inside it
              at permission level <code>{String(trustReq.permissions?.level || "workspace")}</code>
              {trustReq.permissions?.network ? " with network access" : " with network access disabled"}.
              Dangerous commands still need your approval.
            </p>
            <div className="row">
              <button className="primary" onClick={() => void confirmTrust()}>Trust and open</button>
              <button className="ghost" onClick={() => setTrustReq(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}

      {overlay === "settings" && (
        <Settings
          cfg={cfg}
          apiKey={settingsKey}
          onKey={setSettingsKey}
          onClose={() => setOverlay("")}
          onToast={pushToast}
          onSave={async (values, key) => {
            await api.saveConfig(values, key, String((values.model as { api_key_env?: string } | undefined)?.api_key_env || ""));
            setOverlay("");
            pushToast("Settings saved", "ok");
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
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [level, setLevel] = useState("workspace");
  const [error, setError] = useState("");
  const [providers, setProviders] = useState<{ id: string; needs_key?: boolean; api_key_env?: string; endpoint?: string }[]>([]);
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
  const [testState, setTestState] = useState<{ busy: boolean; text: string; ok?: boolean }>({ busy: false, text: "" });

  useEffect(() => {
    api.onboarding().then((data) => {
      setWorkspace(data.suggested_workspace);
      setProviders(data.providers);
      setDetected(data.detected || []);
      const preferred = data.defaults?.provider || "mock";
      setProvider(preferred);
      const hit = (data.detected || []).find((d) => d.provider === preferred && d.running && d.models.length);
      if (hit) setModel(hit.models[0].id);
    });
  }, []);

  const detectedFor = detected.find((d) => d.provider === provider && d.running);
  const needsKey = providers.find((p) => p.id === provider)?.needs_key;

  async function testConnection() {
    setTestState({ busy: true, text: "" });
    try {
      const preset = providers.find((p) => p.id === provider);
      const result = await api.testModel({
        provider,
        name: model || String(preset?.id || ""),
        endpoint: String(preset?.endpoint || ""),
        api_key_env: String(preset?.api_key_env || ""),
      });
      if (result.ok) setTestState({ busy: false, ok: true, text: `Connected in ${result.latency_ms}ms — “${(result.reply || "").slice(0, 80)}”` });
      else setTestState({ busy: false, ok: false, text: result.error || "test failed" });
    } catch (err) {
      setTestState({ busy: false, ok: false, text: String(err) });
    }
  }

  async function finish() {
    try {
      await api.completeOnboarding({ workspace, provider, model: model || provider, name: model, api_key: apiKey, permission_level: level, theme: "light" });
      onDone();
    } catch (err) {
      setError(String(err));
    }
  }

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
            {detected.some((d) => d.running) && (
              <p className="hint detect-banner">
                Detected: {detected.filter((d) => d.running).map((d) => `${d.label} (${d.models.length} model${d.models.length === 1 ? "" : "s"})`).join(" · ")}
              </p>
            )}
            <select value={provider} onChange={(e) => {
              const next = e.target.value;
              setProvider(next);
              const hit = detected.find((d) => d.provider === next && d.running && d.models.length);
              setModel(hit ? hit.models[0].id : "");
            }}>
              {providers.map((p) => <option key={p.id} value={p.id}>{p.id}{detected.find((d) => d.provider === p.id && d.running) ? " — detected" : ""}</option>)}
            </select>
            {detectedFor && detectedFor.models.length > 0 && (
              <>
                <label>Installed model</label>
                <select value={model} onChange={(e) => setModel(e.target.value)}>
                  {detectedFor.models.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.id}{m.detail ? ` (${m.detail})` : ""}{m.capabilities?.tools ? " · tools" : ""}
                    </option>
                  ))}
                </select>
              </>
            )}
            <p className="hint">Mock works offline with no API key. Local servers are auto-detected. Switch later in Settings.</p>
          </div>
        )}
        {step === 2 && (
          <div className="field">
            <label>API key {needsKey ? "" : "(optional)"}</label>
            <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder={needsKey ? "Paste key, or leave blank if already in the environment" : "Not needed for Mock/local"} />
            <p className="hint">Saved only in ~/.config/shadow-agent/secrets.env (mode 600). Never written into YAML or git.</p>
            <div className="row">
              <button type="button" className="ghost" disabled={testState.busy} onClick={() => void testConnection()}>
                {testState.busy ? "Testing…" : "Test connection"}
              </button>
            </div>
            {testState.text && <p className={testState.ok ? "health-ok" : "health-bad"}>{testState.text}</p>}
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
  onToast,
}: {
  cfg: Record<string, unknown>;
  apiKey: string;
  onKey: (v: string) => void;
  onClose: () => void;
  onSave: (values: Record<string, unknown>, key: string) => Promise<void>;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
}) {
  const model = (cfg.model || {}) as Record<string, string>;
  const permissions = (cfg.permissions || {}) as Record<string, string | boolean>;
  const ui = (cfg.ui || {}) as Record<string, string | number | boolean>;
  const [defaultModel, setDefaultModel] = useState(model.default || "mock");
  const [provider, setProvider] = useState(model.provider || "mock");
  const [endpoint, setEndpoint] = useState(model.endpoint || "");
  const [name, setName] = useState(model.name || "");
  const [keyEnv, setKeyEnv] = useState(model.api_key_env || "OPENAI_API_KEY");
  const [level, setLevel] = useState(String(permissions.level || "workspace"));
  const [theme, setTheme] = useState(String(ui.theme || "light"));
  const [ability, setAbility] = useState(String(ui.ability || "none"));
  const [notify, setNotify] = useState(ui.notify !== false);
  const [presets, setPresets] = useState<{ id: string; provider: string; endpoint?: string; api_key_env?: string; name?: string }[]>([]);
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
  const [testState, setTestState] = useState<{ busy: boolean; text: string; ok?: boolean }>({ busy: false, text: "" });

  useEffect(() => {
    void api.onboarding().then((data) => setPresets(data.providers as { id: string; provider: string; endpoint?: string; api_key_env?: string; name?: string }[]));
    void api.detectProviders().then((data) => setDetected(data.providers));
  }, []);

  const detectedFor = detected.find((d) => d.provider === provider && d.running);

  function applyPreset(id: string) {
    const preset = presets.find((p) => p.id === id);
    if (!preset) return;
    setProvider(preset.provider || id);
    setDefaultModel(id);
    setEndpoint(preset.endpoint || "");
    setName(preset.name || "");
    setKeyEnv(preset.api_key_env || "OPENAI_API_KEY");
    const hit = detected.find((d) => d.provider === (preset.provider || id) && d.running && d.models.length);
    if (hit) {
      setName(hit.models[0].id);
      setDefaultModel(hit.models[0].id);
    }
  }

  async function testConnection() {
    setTestState({ busy: true, text: "" });
    try {
      const result = await api.testModel({ provider, name, endpoint, api_key_env: keyEnv });
      if (result.ok) setTestState({ busy: false, ok: true, text: `OK in ${result.latency_ms}ms — “${(result.reply || "").slice(0, 80)}”` });
      else setTestState({ busy: false, ok: false, text: result.error || "test failed" });
    } catch (err) {
      setTestState({ busy: false, ok: false, text: String(err) });
    }
  }

  return (
    <div className="modal-back" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Settings</h2>
        <p className="hint">These write ~/.config/shadow-agent/config.yaml. Keys stay in secrets.env.</p>
        <div className="field">
          <label>Provider preset</label>
          <select value="" onChange={(e) => e.target.value && applyPreset(e.target.value)}>
            <option value="">Pick to auto-fill…</option>
            {presets.map((p) => (
              <option key={p.id} value={p.id}>
                {p.id}{detected.find((d) => d.provider === p.id && d.running) ? " — detected" : ""}
              </option>
            ))}
          </select>
        </div>
        {detectedFor && detectedFor.models.length > 0 && (
          <div className="field">
            <label>Detected models on this machine</label>
            <select
              value={detectedFor.models.some((m) => m.id === name) ? name : ""}
              onChange={(e) => {
                if (!e.target.value) return;
                setName(e.target.value);
                setDefaultModel(e.target.value);
                setEndpoint(detectedFor.endpoint);
              }}
            >
              <option value="">Pick an installed model…</option>
              {detectedFor.models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id}{m.detail ? ` (${m.detail})` : ""}{m.capabilities?.tools ? " · tools" : ""}{m.capabilities?.thinking ? " · thinking" : ""}
                </option>
              ))}
            </select>
          </div>
        )}
        <div className="field"><label>Default model</label><input value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} /></div>
        <div className="field"><label>Provider</label>
          <select value={provider} onChange={(e) => setProvider(e.target.value)}>
            {["mock", "openai_compatible", "ollama", "local", "llamacpp", "vllm"].map((p) => <option key={p}>{p}</option>)}
          </select>
        </div>
        <div className="field"><label>Endpoint</label><input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://api.x.ai/v1" /></div>
        <div className="field"><label>Model name</label><input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. gpt-4.1, qwen3:14b, llama3.2 — any id your provider accepts" /></div>
        <div className="field"><label>API key env var</label><input value={keyEnv} onChange={(e) => setKeyEnv(e.target.value)} /></div>
        <div className="field"><label>Paste API key</label><input type="password" value={apiKey} onChange={(e) => onKey(e.target.value)} placeholder="leave blank to keep the current secret" /></div>
        <div className="field">
          <label>Register this model so it appears in the per-task dropdown</label>
          <button className="ghost" onClick={() => {
            if (!defaultModel) { onToast("Enter a default model id first", "err"); return; }
            void api.registerModel(defaultModel, provider, name, endpoint).then(() => {
              onToast(`Registered ${defaultModel} for ${provider || "openai_compatible"}`, "ok");
            }).catch((err) => onToast(String(err), "err"));
          }}>Register custom model</button>
        </div>
        <div className="field"><label>Permissions</label>
          <select value={level} onChange={(e) => setLevel(e.target.value)}>
            <option value="read_only">read_only</option>
            <option value="workspace">workspace</option>
            <option value="elevated">elevated</option>
          </select>
        </div>
        <div className="field"><label>Theme</label>
          <select value={theme} onChange={(e) => setTheme(e.target.value)}>
            <option value="light">light (Codex default)</option>
            <option value="dark">dark (Codex)</option>
            <option value="dim">dim</option>
          </select>
        </div>
        <div className="field"><label>Abilities</label>
          <select value={ability} onChange={(e) => setAbility(e.target.value)} title="Computer Use / Custom live in Settings, not the composer">
            <option value="none">none</option>
            <option value="computer_use">Computer Use</option>
            <option value="custom">Custom</option>
          </select>
          <p className="hint">Abilities are configured here so the composer stays clean: + icon, input, model, mode, mic, submit.</p>
        </div>
        <div className="field">
          <label className="row" style={{ alignItems: "center", gap: 8 }}>
            <input type="checkbox" checked={notify} onChange={(e) => setNotify(e.target.checked)} style={{ width: "auto" }} />
            Desktop notification when a long task finishes
          </label>
        </div>
        <div className="row">
          <button className="ghost" disabled={testState.busy} onClick={() => void testConnection()}>
            {testState.busy ? "Testing…" : "Test connection"}
          </button>
          {testState.text && <span className={testState.ok ? "health-ok" : "health-bad"}>{testState.text}</span>}
        </div>
        <div className="row">
          <button className="primary" onClick={() => {
            if (testState.text && !testState.ok) onToast("Saving anyway — last test failed", "info");
            void onSave({
              model: { default: defaultModel, provider, endpoint, name, api_key_env: keyEnv },
              permissions: { level },
              ui: { theme, ability, notify },
            }, apiKey);
          }}>Save</button>
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
