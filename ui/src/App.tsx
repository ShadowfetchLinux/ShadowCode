import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Approval,
  type CommandResult,
  type EventRow,
  type Health,
  type ModelInfo,
  type Project,
  type ProviderInfo,
  type Session,
  type UpdateInfo,
} from "./api";
import { ApprovalCard, CommandCardView, Empty, OpCard, type ChatItem } from "./components/cards";
import { Drawer, type DrawerTab } from "./components/Drawer";
import { Onboarding } from "./components/Onboarding";
import { CustomModelDialog, Help, Palette, ProjectPicker, TrustDialog, type PaletteItem } from "./components/overlays";
import { Settings } from "./components/Settings";

type Overlay = "" | "settings" | "help" | "palette" | "project" | "custom-model";
type Toast = { id: number; text: string; kind: "ok" | "err" | "info" };

let toastSeq = 1;
const MODES = ["coder", "researcher", "reviewer", "tester"];

function fmtTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k`;
  return String(n);
}

export default function App() {
  // --- data ---------------------------------------------------------------
  const [ready, setReady] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [health, setHealth] = useState<Health | null>(null);
  const [status, setStatus] = useState<{ workspace: string; model: { default: string; provider: string; name?: string; context_limit?: number }; permissions: { level: string } }>({ workspace: "", model: { default: "mock", provider: "mock", name: "", context_limit: 128000 }, permissions: { level: "workspace" } });
  const [cfg, setCfg] = useState<Record<string, unknown>>({});
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [commandList, setCommandList] = useState<{ name: string; description: string; arg_spec: string }[]>([]);

  // --- conversation --------------------------------------------------------
  const [task, setTask] = useState("");
  const [chips, setChips] = useState<string[]>([]);
  const [chat, setChat] = useState<ChatItem[]>([]);
  const [commandCards, setCommandCards] = useState<CommandResult[]>([]);
  const [busy, setBusy] = useState(false);
  const [jobId, setJobId] = useState("");
  const [summary, setSummary] = useState("");
  const [usage, setUsage] = useState<Record<string, number>>({});
  const [stage, setStage] = useState("IDLE");
  const [fixRetries, setFixRetries] = useState(0);
  const [maxFixRetries, setMaxFixRetries] = useState(3);
  const [approvals, setApprovals] = useState<Approval[]>([]);
  const [modelChoice, setModelChoice] = useState("");
  const [mode, setMode] = useState("coder");
  const [slashMenu, setSlashMenu] = useState<{ open: boolean; q: string; index: number }>({ open: false, q: "", index: 0 });

  // --- chrome --------------------------------------------------------------
  // The drawer is closed by default: the default view is top bar, transcript,
  // composer, and the thin status line. Everything else lives behind Ctrl+B / ⌘K.
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [drawerTab, setDrawerTab] = useState<DrawerTab>("sessions");
  const [diffPath, setDiffPath] = useState("");
  const [overlay, setOverlay] = useState<Overlay>("");
  const [trustReq, setTrustReq] = useState<{ path: string; name?: string; permissions?: Record<string, unknown> } | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [error, setError] = useState("");
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const streamRef = useRef<HTMLDivElement>(null);
  const sourceRef = useRef<EventSource | null>(null);

  const pushToast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = toastSeq++;
    setToasts((prev) => [...prev.slice(-3), { id, text, kind }]);
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 5000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [healthData, onboard, modelData, projectData, sessionData, statusData, cfgData, providerData] = await Promise.all([
        api.health(),
        api.onboarding(),
        api.models(),
        api.projects(),
        api.sessions(),
        api.status(),
        api.config(),
        api.providers(),
      ]);
      setHealth(healthData);
      setNeedsOnboard(!onboard.completed);
      setWorkspace(healthData.workspace || statusData.workspace);
      setModels(modelData.models);
      setProviders(providerData.providers);
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

  const refreshApprovals = useCallback(async () => {
    try {
      setApprovals((await api.approvals()).approvals);
    } catch { /* offline */ }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    api.commands().then((d) => setCommandList(d.commands)).catch(() => setCommandList([]));
    api.updateCheck().then(setUpdate).catch(() => setUpdate(null));
  }, []);
  useEffect(() => {
    void refreshApprovals();
    const id = setInterval(() => void refreshApprovals(), busy ? 1500 : 6000);
    return () => clearInterval(id);
  }, [refreshApprovals, busy]);

  // Theme: light unless config says dark.
  useEffect(() => {
    const theme = String((cfg.ui as { theme?: string } | undefined)?.theme || "light");
    document.documentElement.dataset.theme = theme;
  }, [cfg]);

  // Title badge while working / when a task finishes in the background.
  useEffect(() => {
    document.title = busy ? "● ShadowCode" : "ShadowCode";
  }, [busy]);

  // Auto-scroll the transcript.
  useEffect(() => {
    const el = streamRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [chat, summary, busy]);

  // --- keyboard ------------------------------------------------------------
  useEffect(() => {
    function onKey(ev: KeyboardEvent) {
      const key = ev.key.toLowerCase();
      const inField = ["INPUT", "TEXTAREA", "SELECT"].includes((ev.target as HTMLElement)?.tagName || "");
      if (ev.key === "Escape") {
        if (overlay) { setOverlay(""); return; }
        if (trustReq) { setTrustReq(null); return; }
        if (approvals[0]) { void decide(approvals[0].id, "deny"); return; }
        if (drawerOpen) { setDrawerOpen(false); return; }
        return;
      }
      if (ev.key === "Enter" && !inField && approvals[0] && !overlay) {
        ev.preventDefault();
        void decide(approvals[0].id, "approve");
        return;
      }
      const mod = ev.ctrlKey || ev.metaKey;
      if (mod && key === "k") { ev.preventDefault(); setOverlay("palette"); return; }
      if (mod && key === "b") { ev.preventDefault(); setDrawerOpen((v) => !v); return; }
      if (mod && key === ",") { ev.preventDefault(); setOverlay("settings"); return; }
      if (mod && key === "p") { ev.preventDefault(); setOverlay("project"); return; }
      if (mod && key === "l") { ev.preventDefault(); promptRef.current?.focus(); return; }
      if (mod && key === "n") { ev.preventDefault(); void newSession(); return; }
      if (mod && key === ".") { ev.preventDefault(); void stopAgent(); return; }
      if (mod && ev.shiftKey && key === "e") { ev.preventDefault(); exportSession(); return; }
      if (key === "?" && !inField) { ev.preventDefault(); setOverlay("help"); }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // --- events → transcript ---------------------------------------------------
  function ingestEvent(row: EventRow) {
    const payload = row.payload || {};
    const taskId = String(row.task_id || "");
    if (row.type === "model.delta" && payload.text) {
      setChat((items) => {
        const last = items[items.length - 1];
        if (last && last.kind === "agent" && !last.who) return [...items.slice(0, -1), { kind: "agent", text: String(payload.text) }];
        return [...items, { kind: "agent", text: String(payload.text) }];
      });
    }
    if (row.type === "model.retry") pushToast(`Model busy — retry ${payload.attempt}/${payload.max_attempts} in ${payload.wait_sec}s`, "info");
    if (row.type === "tool.started") {
      setChat((items) => [
        ...items,
        { kind: "tool", tool: String(payload.tool || "tool"), text: "", live: true, icon: "●", headline: String(payload.tool || "tool"), fullOutput: "", collapsed: true, taskId },
      ]);
    }
    if (row.type === "tool.completed") {
      setChat((items) => {
        const next = [...items];
        const rev = [...next].reverse().findIndex((item) => item.kind === "tool" && item.tool === payload.tool && item.live);
        const real = rev >= 0 ? next.length - 1 - rev : -1;
        const args = (payload.arguments || {}) as Record<string, unknown>;
        const card: ChatItem = {
          kind: "tool",
          tool: String(payload.tool || "tool"),
          ok: Boolean(payload.success),
          text: String(payload.output_preview || payload.error || ""),
          live: false,
          icon: String(payload.icon || (payload.success ? "✓" : "✗")),
          headline: String(payload.headline || payload.tool || ""),
          fullOutput: String(payload.output_full || payload.output_preview || payload.error || ""),
          collapsed: real >= 0 && next[real].kind === "tool" ? (next[real] as Extract<ChatItem, { kind: "tool" }>).collapsed ?? true : true,
          taskId,
          path: String(args.path || args.dest || ""),
        };
        if (real >= 0) next[real] = card;
        else next.push(card);
        return next;
      });
    }
    if (row.type.startsWith("agent.") && payload.stage) {
      setStage(String(payload.stage));
      if (typeof payload.fix_retries === "number") setFixRetries(payload.fix_retries);
      if (typeof payload.max_fix_retries === "number") setMaxFixRetries(payload.max_fix_retries);
    }
    if (row.type === "approval.requested") void refreshApprovals();
    if (row.type === "agent.completed") {
      setSummary(String(payload.summary || ""));
      setUsage((payload.usage as Record<string, number>) || {});
      setStage(String(payload.stage || (payload.success ? "DONE" : "IDLE")));
      setBusy(false);
    }
  }

  function notifyDesktop(ok: boolean, text: string) {
    if (!document.hidden || typeof Notification === "undefined") return;
    if (Notification.permission === "granted") {
      try { new Notification(ok ? "ShadowCode — task complete" : "ShadowCode — task stopped", { body: text.slice(0, 140), icon: "/icon.svg" }); } catch { /* ignore */ }
    }
  }

  // --- actions -------------------------------------------------------------
  function composeTask() {
    const extra = chips.length ? `\n\nAttached paths: ${chips.join(", ")}` : "";
    return (task.trim() + extra).trim();
  }

  async function runTask() {
    const text = composeTask();
    if (!text || busy) return;
    setError("");
    setBusy(true);
    setSummary("");
    setStage("UNDERSTAND");
    setFixRetries(0);
    setChat((items) => [...items, { kind: "user", text }]);
    // Clear the composer immediately: Enter must never leave the prompt behind.
    setTask("");
    setChips([]);
    if (typeof Notification !== "undefined" && Notification.permission === "default") void Notification.requestPermission().catch(() => undefined);
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
            const payload = row.payload as { summary?: string; usage?: Record<string, number>; status?: string; result?: { summary: string } };
            const finalSummary = payload.result?.summary || payload.summary || "";
            setSummary(finalSummary);
            if (payload.usage) setUsage(payload.usage);
            setBusy(false);
            source.close();
            const ok = payload.status === "completed";
            pushToast(ok ? "Task complete" : `Task ${payload.status || "failed"}`, ok ? "ok" : "err");
            notifyDesktop(ok, finalSummary || text);
            void refresh();
            return;
          }
          ingestEvent(row);
        } catch { /* keepalive */ }
      };
      source.onerror = () => {
        source.close();
        void api.job(job.id).then((done) => { setSummary(done.summary || done.result?.summary || ""); setBusy(false); });
      };
    } catch (err) {
      setError(String(err));
      pushToast(String(err), "err");
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

  async function decide(id: string, decision: "approve" | "deny") {
    try {
      await api.decide(id, decision);
    } catch (err) {
      pushToast(String(err), "err");
    }
    await refreshApprovals();
  }

  async function newSession() {
    if (!workspace) return;
    const created = await api.createSession(workspace, "New session");
    setSessionId(created.id);
    setChat([]);
    setCommandCards([]);
    setSummary("");
    setUsage({});
    setStage("IDLE");
    await refresh();
    promptRef.current?.focus();
  }

  async function openSession(id: string) {
    const detail = await api.session(id);
    setSessionId(id);
    setWorkspace(detail.workspace);
    setChat(
      detail.events
        .filter((e) => e.type === "agent.started" || e.type === "agent.completed" || e.type === "tool.completed")
        .map((e) => {
          if (e.type === "agent.started") return { kind: "user" as const, text: String(e.payload.task || "") };
          if (e.type === "tool.completed") {
            const p = e.payload;
            const args = (p.arguments || {}) as Record<string, unknown>;
            return {
              kind: "tool" as const,
              tool: String(p.tool || "tool"),
              ok: Boolean(p.success),
              text: String(p.output_preview || p.error || ""),
              icon: String(p.icon || (p.success ? "✓" : "✗")),
              headline: String(p.headline || p.tool || ""),
              fullOutput: String(p.output_full || p.output_preview || p.error || ""),
              collapsed: true,
              taskId: String(e.task_id || ""),
              path: String(args.path || ""),
            };
          }
          return { kind: "agent" as const, text: String(e.payload.summary || ""), who: "Result" };
        }),
    );
    setSummary("");
    setDrawerOpen(false);
  }

  async function pickProject(path: string) {
    try {
      const opened = await api.openProject(path);
      if (opened.needs_trust) {
        setTrustReq({ path: opened.path, name: opened.name, permissions: opened.permissions });
        setOverlay("");
        return;
      }
      setWorkspace(opened.path);
      setSessionId(opened.session_id);
      setChat([]);
      setOverlay("");
      await refresh();
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  async function confirmTrust() {
    if (!trustReq) return;
    try {
      const opened = await api.trustProject(trustReq.path);
      setTrustReq(null);
      setWorkspace(opened.path);
      setSessionId(opened.session_id);
      setChat([]);
      pushToast(`Trusted ${opened.path}`, "ok");
      await refresh();
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  function exportSession() {
    if (!sessionId) return;
    window.open(api.exportUrl(sessionId, "md"), "_blank");
  }

  async function undoLast() {
    try {
      const result = await api.undo();
      pushToast(`Restored ${result.restored.length} file(s)`, "ok");
      setChat((items) => [...items, { kind: "agent", text: `Restored ${result.restored.length} file(s): ${result.restored.join(", ") || "—"}`, who: "Undo" }]);
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  async function rewindTask(taskId: string) {
    try {
      const result = await api.rewindTask(taskId);
      pushToast(`Rewound ${result.restored.length} file(s)`, "ok");
      setChat((items) => [...items, { kind: "agent", text: `Rewound ${result.restored.length} file(s): ${result.restored.join(", ") || "—"}`, who: "Rewind" }]);
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  function reviewDiff(path: string) {
    setDiffPath(path);
    setDrawerTab("changes");
    setDrawerOpen(true);
  }

  function openDrawer(tab: DrawerTab) {
    setDrawerTab(tab);
    setDrawerOpen(true);
  }

  async function runSlashCommand(name: string, args: string) {
    try {
      const result = await api.runCommand(name, args, sessionId || undefined);
      if (result.kind === "overlay") setOverlay((result.overlay as Overlay) || "settings");
      else setCommandCards((prev) => [...prev.slice(-5), result]);
      if (result.kind === "error") pushToast(result.headline || name, "err");
      await refresh();
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  function submitSlash(line: string) {
    const rest = line.slice(1);
    const space = rest.indexOf(" ");
    const name = space >= 0 ? rest.slice(0, space) : rest;
    const args = space >= 0 ? rest.slice(space + 1) : "";
    setSlashMenu({ open: false, q: "", index: 0 });
    void runSlashCommand(name, args);
  }

  async function applyCustomModel(id: string, provider: string, endpoint: string) {
    try {
      await api.selectModel(id, { provider, endpoint, name: id });
      pushToast(`Default model → ${id}`, "ok");
      setOverlay("");
      setModelChoice("");
      await refresh();
    } catch (err) {
      pushToast(String(err), "err");
    }
  }

  async function onFiles(files: FileList | File[]) {
    const next: string[] = [];
    for (const file of Array.from(files)) {
      const text = await file.text().catch(() => "");
      if (text) {
        const saved = await api.attach(file.name, text);
        next.push(saved.path);
      }
    }
    if (next.length) setChips((prev) => [...prev, ...next]);
  }

  async function onDrop(ev: React.DragEvent) {
    ev.preventDefault();
    await onFiles(ev.dataTransfer.files);
    const text = ev.dataTransfer.getData("text/plain").trim();
    if (text && !text.includes("\n") && (text.startsWith("/") || text.startsWith("."))) setChips((prev) => [...prev, text]);
  }

  // --- derived -------------------------------------------------------------
  const palette: PaletteItem[] = useMemo(
    () => [
      { id: "stop", label: "Stop the agent", hint: "Ctrl+.", run: () => void stopAgent() },
      { id: "new", label: "New session", hint: "Ctrl+N", run: () => void newSession() },
      { id: "project", label: "Open project…", hint: "Ctrl+P", run: () => setOverlay("project") },
      { id: "sessions", label: "Sessions", run: () => openDrawer("sessions") },
      { id: "files", label: "Files", run: () => openDrawer("files") },
      { id: "changes", label: "Changes (git · diff)", run: () => openDrawer("changes") },
      { id: "skills", label: "Skills and instructions", run: () => openDrawer("skills") },
      { id: "goals", label: "Goals", run: () => openDrawer("goals") },
      { id: "health", label: "Health · doctor · router", run: () => openDrawer("health") },
      { id: "background", label: "Background processes", run: () => openDrawer("background") },
      { id: "undo", label: "Undo last task's file changes", run: () => void undoLast() },
      { id: "export", label: "Export session as Markdown", hint: "Ctrl+Shift+E", run: () => exportSession() },
      { id: "custom-model", label: "Use a custom model…", run: () => setOverlay("custom-model") },
      { id: "settings", label: "Settings", hint: "Ctrl+,", run: () => setOverlay("settings") },
      { id: "theme", label: "Toggle dark mode", run: () => void api.saveConfig({ ui: { theme: String((cfg.ui as { theme?: string })?.theme) === "dark" ? "light" : "dark" } }).then(refresh) },
      { id: "help", label: "Keyboard cheat sheet", hint: "?", run: () => setOverlay("help") },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [workspace, sessionId, jobId, cfg],
  );

  const modelGroups = useMemo(() => {
    const groups = new Map<string, ModelInfo[]>();
    for (const m of models) {
      const list = groups.get(m.provider) || [];
      list.push(m);
      groups.set(m.provider, list);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [models]);

  const currentModel = status.model.name || status.model.default;
  const tokens = usage.total_tokens || 0;
  const ctxPct = status.model.context_limit ? Math.min(100, Math.round(((usage.prompt_tokens || 0) / status.model.context_limit) * 100)) : 0;
  const currentSession = sessions.find((s) => s.id === sessionId);
  const title = currentSession?.title || (chat.find((c) => c.kind === "user") as { text: string } | undefined)?.text?.split("\n")[0] || "ShadowCode";
  const slashMatches = slashMenu.open ? commandList.filter((c) => c.name.startsWith(slashMenu.q)).slice(0, 10) : [];
  // The streamed agent text usually *is* the result; only show a separate
  // Result block when the final summary adds something new.
  const lastItem = chat[chat.length - 1];
  const showSummary = Boolean(summary) && !(lastItem && lastItem.kind === "agent" && lastItem.text.trim() === summary.trim());
  const versionLabel = health?.version || "";

  if (!ready) {
    return <div className="boot"><span className="skel" /><span className="skel short" /></div>;
  }
  if (needsOnboard) {
    return <Onboarding onDone={() => { setNeedsOnboard(false); void refresh(); }} />;
  }

  return (
    <div className={`app ${drawerOpen ? "drawer-open" : "drawer-closed"}`} onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <header className="top">
        <div className="top-left">
          <button type="button" className={`icon-btn ${drawerOpen ? "on" : ""}`} title="Drawer (Ctrl+B)" onClick={() => setDrawerOpen((v) => !v)}>☰</button>
          <button type="button" className="icon-btn" title="New session (Ctrl+N)" onClick={() => void newSession()}>＋</button>
        </div>
        <div className="top-title" title={workspace}>{title}</div>
        <div className="top-right">
          {update?.update_available && <button type="button" className="pill update" title={`Run: shadow update`} onClick={() => openDrawer("health")}>v{update.latest}</button>}
          <button type="button" className="icon-btn" title="Command palette (Ctrl+K)" onClick={() => setOverlay("palette")}>⌘K</button>
          <button type="button" className="icon-btn" title="Settings (Ctrl+,)" onClick={() => setOverlay("settings")}>⚙</button>
        </div>
      </header>

      <main className="stage">
        <div className="chat-stream" ref={streamRef}>
          <div className="chat-inner">
            {error && <div className="notice bad">{error}</div>}
            {chat.length === 0 && commandCards.length === 0 && !showSummary && (
              <Empty title="What should we build?" body="Describe a change. ShadowCode inspects, plans, edits, and verifies. Type / for commands, ? for keys." />
            )}
            {chat.map((item, i) =>
              item.kind === "tool" ? (
                <OpCard
                  key={i}
                  item={item}
                  onToggle={() => setChat((items) => { const next = [...items]; const it = next[i]; if (it && it.kind === "tool") next[i] = { ...it, collapsed: it.collapsed === false }; return next; })}
                  onRewind={(tid) => void rewindTask(tid)}
                  onReviewDiff={reviewDiff}
                />
              ) : item.kind === "user" ? (
                <div key={i} className="msg-user"><div className="user-pill"><div className="bubble">{item.text}</div></div></div>
              ) : (
                <div key={i} className="msg-agent">{item.who && <div className="who">{item.who}</div>}{item.text}</div>
              ),
            )}
            {commandCards.map((card, i) => <CommandCardView key={`c${i}`} card={card} />)}
            {approvals.map((a) => <ApprovalCard key={a.id} approval={a} onDecide={(id, d) => void decide(id, d)} />)}
            {busy && <div className="msg-agent"><div className="skel-rows"><span className="skel" /><span className="skel short" /></div></div>}
            {showSummary && <div className="msg-agent"><div className="who">Result</div>{summary}</div>}
          </div>
        </div>

        <div className="composer-wrap">
          {slashMenu.open && (
            <div className="slash-menu">
              {slashMatches.map((c, i) => (
                <button type="button" key={c.name} className={`slash-hit ${i === slashMenu.index ? "on" : ""}`} onClick={() => { setTask(`/${c.name}${c.arg_spec ? " " : ""}`); setSlashMenu({ open: false, q: "", index: 0 }); promptRef.current?.focus(); }}>
                  <strong>/{c.name}</strong>
                  {c.arg_spec && <span className="slash-arg">{c.arg_spec}</span>}
                  <span className="slash-desc">{c.description}</span>
                </button>
              ))}
              {slashMatches.length === 0 && <div className="slash-empty">No matching commands.</div>}
            </div>
          )}
          <form className="composer" onSubmit={(ev) => { ev.preventDefault(); void runTask(); }}>
            {chips.length > 0 && (
              <div className="chips">
                {chips.map((c) => <button type="button" className="path-chip" key={c} onClick={() => setChips(chips.filter((x) => x !== c))}>{c} ×</button>)}
              </div>
            )}
            <div className="composer-row">
              <button type="button" className="plus-btn" title="Attach a file or path" onClick={() => fileRef.current?.click()}>＋</button>
              <input ref={fileRef} type="file" multiple hidden onChange={(e) => { if (e.target.files) void onFiles(e.target.files); e.target.value = ""; }} />
              <textarea
                ref={promptRef}
                value={task}
                rows={1}
                onChange={(ev) => {
                  const text = ev.target.value;
                  setTask(text);
                  if (text.startsWith("/")) {
                    const rest = text.slice(1);
                    const space = rest.indexOf(" ");
                    setSlashMenu({ open: space < 0, q: space >= 0 ? rest.slice(0, space) : rest, index: 0 });
                  } else if (slashMenu.open) {
                    setSlashMenu({ open: false, q: "", index: 0 });
                  }
                  ev.target.style.height = "auto";
                  ev.target.style.height = `${Math.min(ev.target.scrollHeight, 220)}px`;
                }}
                onKeyDown={(ev) => {
                  if (slashMenu.open && slashMatches.length) {
                    if (ev.key === "ArrowDown") { ev.preventDefault(); setSlashMenu((s) => ({ ...s, index: Math.min(s.index + 1, slashMatches.length - 1) })); return; }
                    if (ev.key === "ArrowUp") { ev.preventDefault(); setSlashMenu((s) => ({ ...s, index: Math.max(s.index - 1, 0) })); return; }
                    if (ev.key === "Tab" || (ev.key === "Enter" && slashMatches[slashMenu.index] && `/${slashMatches[slashMenu.index].name}` !== task.trim())) {
                      ev.preventDefault();
                      const picked = slashMatches[slashMenu.index];
                      setTask(`/${picked.name}${picked.arg_spec ? " " : ""}`);
                      setSlashMenu({ open: false, q: "", index: 0 });
                      return;
                    }
                    if (ev.key === "Escape") { ev.preventDefault(); setSlashMenu({ open: false, q: "", index: 0 }); return; }
                  }
                  if (ev.key === "Enter" && !ev.shiftKey && !ev.nativeEvent.isComposing) {
                    ev.preventDefault();
                    if (task.startsWith("/")) {
                      submitSlash(task);
                      setTask("");
                    } else {
                      void runTask();
                    }
                    if (ev.currentTarget) ev.currentTarget.style.height = "auto";
                  }
                }}
                placeholder="Ask for follow-up changes…  (/ for commands)"
              />
              <div className="composer-controls">
                <select
                  className="model-select"
                  value={modelChoice}
                  title="Model for this task (blank = default). Pick “Custom…” to use any model id."
                  onChange={(e) => {
                    if (e.target.value === "__custom__") { setOverlay("custom-model"); return; }
                    setModelChoice(e.target.value);
                  }}
                >
                  <option value="">{currentModel}</option>
                  {modelGroups.map(([provider, list]) => (
                    <optgroup key={provider} label={provider}>
                      {list.map((m) => <option key={m.id} value={m.id}>{m.id}{m.detected ? " ●" : ""}</option>)}
                    </optgroup>
                  ))}
                  <option value="__custom__">Custom model…</option>
                </select>
                <select className="mode-select" value={mode} onChange={(e) => setMode(e.target.value)} title="Agent mode">
                  {MODES.map((m) => <option key={m} value={m}>{m}</option>)}
                </select>
                <button type="button" className="mic-btn" title="Voice input (coming soon)" disabled>🎙</button>
                {busy ? (
                  <button type="button" className="submit-btn stop" title="Stop (Ctrl+.)" onClick={() => void stopAgent()}>■</button>
                ) : (
                  <button type="submit" className="submit-btn" title="Send (Enter)" disabled={!task.trim() && chips.length === 0}>↑</button>
                )}
              </div>
            </div>
          </form>
        </div>

        <div className="statusline">
          <span title="Model">{status.model.provider}/{currentModel}</span>
          <span className="sep">·</span>
          <span title="Context used">ctx {ctxPct}%</span>
          <span className="sep">·</span>
          <span title="Tokens this task">{fmtTokens(tokens)} tok</span>
          <span className="sep">·</span>
          <span className={`stage stage-${stage.toLowerCase()}`}>{stage === "FIX" ? `FIX ${fixRetries}/${maxFixRetries}` : stage}{busy ? " ●" : ""}</span>
          <span className="grow" />
          <span title={workspace}>{workspace ? workspace.split("/").pop() : "no workspace"}</span>
          <span className="sep">·</span>
          <span title="Permission level">{status.permissions.level}</span>
          {versionLabel && <><span className="sep">·</span><span className="dim">v{versionLabel}</span></>}
        </div>
      </main>

      {drawerOpen && (
        <Drawer
          tab={drawerTab}
          onTab={setDrawerTab}
          onClose={() => setDrawerOpen(false)}
          workspace={workspace}
          sessions={sessions}
          sessionId={sessionId}
          onOpenSession={(id) => void openSession(id)}
          onNewSession={() => void newSession()}
          onRefreshSessions={refresh}
          diffPath={diffPath}
          onDiffPath={setDiffPath}
          health={health}
          busy={busy}
          toast={pushToast}
        />
      )}

      <div className="toasts">
        {toasts.map((t) => <div key={t.id} className={`toast ${t.kind}`} onClick={() => setToasts((prev) => prev.filter((x) => x.id !== t.id))}>{t.text}</div>)}
      </div>

      {trustReq && <TrustDialog req={trustReq} onCancel={() => setTrustReq(null)} onConfirm={() => void confirmTrust()} />}
      {overlay === "settings" && (
        <Settings
          cfg={cfg}
          onClose={() => setOverlay("")}
          onToast={pushToast}
          onSave={async (values, key, keyEnv) => {
            await api.saveConfig(values, key, keyEnv);
            setOverlay("");
            pushToast("Settings saved", "ok");
            await refresh();
          }}
        />
      )}
      {overlay === "help" && <Help onClose={() => setOverlay("")} version={versionLabel} />}
      {overlay === "palette" && <Palette items={palette} onClose={() => setOverlay("")} />}
      {overlay === "project" && <ProjectPicker projects={projects} current={workspace} onClose={() => setOverlay("")} onPick={(path) => void pickProject(path)} />}
      {overlay === "custom-model" && <CustomModelDialog providers={providers} onClose={() => setOverlay("")} onSubmit={applyCustomModel} />}
    </div>
  );
}
