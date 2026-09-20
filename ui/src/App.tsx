import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpRight,
  Check,
  ChevronRight,
  Code2,
  FileCode2,
  FolderOpen,
  GitBranch,
  GitPullRequest,
  ListChecks,
  LoaderCircle,
  PanelLeft,
  Paperclip,
  Search,
  ShieldCheck,
  Sparkles,
  Square,
  TerminalSquare,
  X,
} from "lucide-react";
import {
  api,
  type Approval,
  type CommandResult,
  type Health,
  type Job,
  type ModelInfo,
  type Project,
  type ProviderInfo,
  type Session,
} from "./api";
import { ApprovalCard, CommandCardView, OpCard } from "./components/cards";
import { Drawer, type DrawerTab } from "./components/Drawer";
import { Sidebar } from "./components/Sidebar";
import { Markdown } from "./components/Markdown";
import { Onboarding } from "./components/Onboarding";
import {
  CustomModelDialog,
  Help,
  Palette,
  ProjectPicker,
  TrustDialog,
  type PaletteItem,
} from "./components/overlays";
import { Settings } from "./components/Settings";
import { useConversation } from "./hooks/useConversation";

type Overlay =
  "" | "settings" | "help" | "palette" | "project" | "custom-model";
type Toast = { id: number; text: string; kind: "ok" | "err" | "info" };
const formatTokens = (n: number) =>
  n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
const draftKey = (id: string, workspace: string) =>
  `shadow:draft:${id || workspace}`;

export default function App() {
  const [ready, setReady] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [health, setHealth] = useState<Health | null>(null);
  const [cfg, setCfg] = useState<Record<string, unknown>>({});
  const [status, setStatus] = useState<Awaited<
    ReturnType<typeof api.status>
  > | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [commands, setCommands] = useState<
    { name: string; description: string; arg_spec: string }[]
  >([]);
  const [task, setTask] = useState("");
  const [chips, setChips] = useState<string[]>([]);
  const [commandCards, setCommandCards] = useState<CommandResult[]>([]);
  const [approvals, setApprovals] = useState<Approval[]>([]);
  const [modelChoice, setModelChoice] = useState("");
  const [mode, setMode] = useState("coder");
  const [sidebar, setSidebar] = useState(
    () =>
      localStorage.getItem("shadow:sidebar") !== "closed" &&
      window.innerWidth > 760,
  );
  const [panel, setPanel] = useState<DrawerTab | null>(null);
  const [diffPath, setDiffPath] = useState("");
  const [overlay, setOverlay] = useState<Overlay>("");
  const [trust, setTrust] = useState<{
    path: string;
    name?: string;
    permissions?: Record<string, unknown>;
  } | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [slashIndex, setSlashIndex] = useState(0);
  const [slashOpen, setSlashOpen] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  const [git, setGit] = useState<{ branch: string; count: number }>({
    branch: "",
    count: 0,
  });
  const [elapsed, setElapsed] = useState(0);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const streamRef = useRef<HTMLDivElement>(null);
  const selectedRef = useRef("");
  const selection = useRef(0);
  const submittingRef = useRef(false);
  const toastSeq = useRef(0);
  const booted = useRef(false);
  const activationQueue = useRef<Promise<unknown>>(Promise.resolve());
  const stick = useRef(true);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = ++toastSeq.current;
    setToasts((prev) => [...prev.slice(-3), { id, text, kind }]);
    setTimeout(
      () => setToasts((prev) => prev.filter((t) => t.id !== id)),
      5000,
    );
  }, []);

  const refresh = useCallback(async () => {
    const [s, p, active, state] = await Promise.all([
      api.sessions(),
      api.projects(),
      api.jobs(),
      api.status(),
    ]);
    setSessions(s.sessions);
    setProjects(p.projects);
    setJobs(active.jobs);
    setStatus(state);
    try {
      const g = await api.git();
      setGit({
        branch: g.repo
          ? g.status
              .split("\n")[0]
              .replace(/^##\s*/, "")
              .split("...")[0]
          : "",
        count: g.files?.length || 0,
      });
    } catch {
      /* status is optional outside git */
    }
  }, []);
  const conversation = useConversation(() => {
    void refresh().catch(() => undefined);
  });
  const { transcript, setTranscript, job, busy, connection } = conversation;
  const locked = busy || submitting || switching;

  async function reloadConfig() {
    const [config, state, modelData, providerData] = await Promise.all([
      api.config(),
      api.status(),
      api.models(),
      api.providers(),
    ]);
    setCfg(config);
    setStatus(state);
    setModels(modelData.models);
    setProviders(providerData.providers);
  }

  async function openSession(id: string) {
    if (submittingRef.current) return;
    if (selectedRef.current) {
      if (task)
        localStorage.setItem(draftKey(selectedRef.current, workspace), task);
      else localStorage.removeItem(draftKey(selectedRef.current, workspace));
    }
    const ticket = ++selection.current;
    setSwitching(true);
    setError("");
    try {
      const activation = activationQueue.current.then(() =>
        api.activateSession(id),
      );
      activationQueue.current = activation.catch(() => undefined);
      const detail = await activation;
      const active = await api.currentJob(id);
      if (ticket !== selection.current) return;
      selectedRef.current = id;
      setSessionId(id);
      setWorkspace(detail.workspace);
      localStorage.setItem("shadow:selected", id);
      conversation.load(detail, active.job);
      setTask(localStorage.getItem(draftKey(id, detail.workspace)) || "");
      setChips([]);
      setCommandCards([]);
      setApprovals([]);
      setSlashOpen(false);
      stick.current = true;
      setAtBottom(true);
      await refresh();
    } catch (e) {
      if (ticket === selection.current) {
        setError(String(e));
        toast(String(e), "err");
      }
    } finally {
      if (ticket === selection.current) setSwitching(false);
    }
  }

  async function boot() {
    setError("");
    try {
      const [h, onboard, sessionData] = await Promise.all([
        api.health(),
        api.onboarding(),
        api.sessions(),
      ]);
      setHealth(h);
      setWorkspace(h.workspace);
      setNeedsOnboard(!onboard.completed);
      await reloadConfig();
      await refresh();
      const saved = localStorage.getItem("shadow:selected");
      const initial =
        sessionData.sessions.find((s) => s.id === saved) ||
        sessionData.sessions.find((s) => s.workspace === h.workspace);
      if (onboard.completed && initial) await openSession(initial.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setReady(true);
    }
  }
  useEffect(() => {
    if (!booted.current) {
      booted.current = true;
      void boot();
    }
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = String(
      (cfg.ui as { theme?: string })?.theme || "light",
    );
  }, [cfg]);
  useEffect(() => {
    localStorage.setItem("shadow:sidebar", sidebar ? "open" : "closed");
  }, [sidebar]);
  useEffect(() => {
    document.title = `${busy ? "● " : ""}ShadowCode`;
  }, [busy]);
  useEffect(() => {
    const key = draftKey(sessionId, workspace);
    const timer = setTimeout(() => {
      if (task) localStorage.setItem(key, task);
      else localStorage.removeItem(key);
    }, 200);
    return () => clearTimeout(timer);
  }, [task, sessionId, workspace]);
  useEffect(() => {
    const el = promptRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
    }
  }, [task]);
  useEffect(() => {
    if (stick.current)
      streamRef.current?.scrollTo({ top: streamRef.current.scrollHeight });
  }, [transcript.items, commandCards, busy, approvals]);
  useEffect(() => {
    let live = true;
    async function poll() {
      try {
        const data = await api.approvals(sessionId);
        if (live) setApprovals(data.approvals);
      } catch {
        /* reconnect banner covers outages */
      }
      try {
        const data = await api.jobs();
        if (live) setJobs(data.jobs);
      } catch {
        /* preserve the last known state */
      }
    }
    void poll();
    const timer = setInterval(poll, busy ? 1200 : 5000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [sessionId, busy]);
  useEffect(() => {
    if (!job || !busy) return;
    const tick = () =>
      setElapsed(Math.max(0, Math.floor(Date.now() / 1000 - job.started_at)));
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [job?.id, busy]);

  async function newSession() {
    if (submitting || switching) return;
    if (!workspace) {
      setOverlay("project");
      return;
    }
    try {
      const created = await api.createSession(workspace, "New task");
      await openSession(created.id);
      promptRef.current?.focus();
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function pickProject(path?: string) {
    if (!path) {
      setOverlay("project");
      return;
    }
    try {
      const opened = await api.openProject(path);
      setOverlay("");
      if (opened.needs_trust) {
        setTrust({
          path: opened.path,
          name: opened.name,
          permissions: opened.permissions,
        });
        return;
      }
      await openSession(opened.session_id);
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function confirmTrust() {
    if (!trust) return;
    try {
      const opened = await api.trustProject(trust.path);
      setTrust(null);
      await openSession(opened.session_id);
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function stop() {
    if (!job || !busy) return;
    try {
      const next = await api.cancelJob(job.id);
      conversation.setJob(next);
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function decide(id: string, decision: "approve" | "deny") {
    try {
      await api.decide(id, decision);
      setApprovals((await api.approvals(sessionId)).approvals);
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function runSlash(text: string) {
    const [name, ...rest] = text.slice(1).split(" ");
    const args = rest.join(" ");
    if (name === "new" || name === "clear") {
      await newSession();
      return;
    }
    const panels: Record<string, DrawerTab> = {
      sessions: "sessions",
      diff: "changes",
      goals: "goals",
      skills: "skills",
      health: "health",
      doctor: "health",
    };
    if (panels[name] && !args) {
      setPanel(panels[name]);
      return;
    }
    if (name === "settings") {
      setOverlay("settings");
      return;
    }
    const result = await api.runCommand(name, args, sessionId || undefined);
    if (result.kind === "overlay")
      setOverlay((result.overlay as Overlay) || "settings");
    else setCommandCards((prev) => [...prev, result]);
    await refresh();
  }
  async function submit() {
    if (locked || submittingRef.current || (!task.trim() && !chips.length))
      return;
    if (["/new", "/clear"].includes(task.trim())) {
      setTask("");
      await newSession();
      return;
    }
    const original = task;
    const attached = [...chips];
    const text = (
      task.trim() +
      (chips.length ? `\n\nAttached paths: ${chips.join(", ")}` : "")
    ).trim();
    submittingRef.current = true;
    setSubmitting(true);
    setError("");
    setTask("");
    setChips([]);
    setSlashOpen(false);
    localStorage.removeItem(draftKey(sessionId, workspace));
    stick.current = true;
    setAtBottom(true);
    try {
      if (text.startsWith("/")) {
        await runSlash(text);
        return;
      }
      const started = await api.startJob(
        text,
        workspace || undefined,
        sessionId || undefined,
        modelChoice || undefined,
        mode,
      );
      if (started.session_id !== selectedRef.current) {
        selectedRef.current = started.session_id;
        setSessionId(started.session_id);
        localStorage.setItem("shadow:selected", started.session_id);
      }
      conversation.start(started);
      await refresh().catch(() => undefined);
    } catch (e) {
      setTask(original);
      setChips(attached);
      setError(String(e));
      toast(String(e), "err");
    } finally {
      setSubmitting(false);
      submittingRef.current = false;
    }
  }
  async function attach(files: FileList | File[]) {
    for (const file of Array.from(files)) {
      if (file.size > 1_000_000) {
        toast(
          `${file.name}: text attachments must be smaller than 1 MB`,
          "err",
        );
        continue;
      }
      try {
        const text = await file.text();
        if (
          text.includes("\0") ||
          (file.type &&
            !file.type.startsWith("text/") &&
            !/json|javascript|xml|yaml/.test(file.type))
        ) {
          toast(`${file.name}: attach a text or source file`, "err");
          continue;
        }
        const saved = await api.attach(file.name, text);
        setChips((prev) => [...new Set([...prev, saved.path])]);
      } catch (e) {
        toast(String(e), "err");
      }
    }
  }
  async function rewind(taskId: string) {
    if (busy) {
      toast("Stop the task before rewinding its files.", "info");
      return;
    }
    try {
      const result = await api.rewindTask(taskId);
      toast(`Restored ${result.restored.length} files`, "ok");
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }
  function exportSession() {
    if (sessionId) window.open(api.exportUrl(sessionId), "_blank", "noopener");
  }
  const palette: PaletteItem[] = [
    {
      id: "new",
      label: "New task",
      hint: "Ctrl+N",
      run: () => void newSession(),
    },
    {
      id: "project",
      label: "Open project",
      hint: "Ctrl+P",
      run: () => setOverlay("project"),
    },
    { id: "files", label: "Browse files", run: () => setPanel("files") },
    { id: "changes", label: "Review changes", run: () => setPanel("changes") },
    {
      id: "terminal",
      label: "Run a terminal command",
      run: () => setPanel("terminal"),
    },
    {
      id: "sessions",
      label: "Manage tasks · rename, branch, export, delete",
      run: () => setPanel("sessions"),
    },
    {
      id: "goals",
      label: "Goals and milestones",
      run: () => setPanel("goals"),
    },
    {
      id: "skills",
      label: "Skills and instructions",
      run: () => setPanel("skills"),
    },
    { id: "health", label: "Workspace health", run: () => setPanel("health") },
    {
      id: "export",
      label: "Export this task as Markdown",
      hint: "Ctrl+Shift+E",
      run: exportSession,
    },
    {
      id: "stop",
      label: "Stop the agent",
      hint: "Ctrl+.",
      run: () => void stop(),
    },
    {
      id: "settings",
      label: "Settings",
      hint: "Ctrl+,",
      run: () => setOverlay("settings"),
    },
    {
      id: "theme",
      label: "Toggle light / dark appearance",
      run: () =>
        void api
          .saveConfig({
            ui: {
              theme:
                (cfg.ui as { theme?: string })?.theme === "dark"
                  ? "light"
                  : "dark",
            },
          })
          .then(reloadConfig)
          .catch((e) => toast(String(e), "err")),
    },
    {
      id: "help",
      label: "Keyboard shortcuts",
      hint: "?",
      run: () => setOverlay("help"),
    },
  ];
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const key = e.key.toLowerCase();
      const mod = e.ctrlKey || e.metaKey;
      const inField = /INPUT|TEXTAREA|SELECT/.test(
        (e.target as HTMLElement)?.tagName || "",
      );
      if (e.key === "Escape") {
        if (overlay) setOverlay("");
        else if (trust) setTrust(null);
        else if (slashOpen) setSlashOpen(false);
        else if (panel) setPanel(null);
        return;
      }
      if (mod && key === "k") {
        e.preventDefault();
        setOverlay("palette");
      }
      if (mod && key === "b") {
        e.preventDefault();
        setSidebar((v) => !v);
      }
      if (mod && key === ",") {
        e.preventDefault();
        setOverlay("settings");
      }
      if (mod && key === "p") {
        e.preventDefault();
        setOverlay("project");
      }
      if (mod && key === "n") {
        e.preventDefault();
        void newSession();
      }
      if (mod && key === "l") {
        e.preventDefault();
        promptRef.current?.focus();
      }
      if (mod && key === ".") {
        e.preventDefault();
        void stop();
      }
      if (mod && e.shiftKey && key === "e") {
        e.preventDefault();
        exportSession();
      }
      if (key === "?" && !inField) {
        e.preventDefault();
        setOverlay("help");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  useEffect(() => {
    api
      .commands()
      .then((d) => setCommands(d.commands))
      .catch(() => undefined);
  }, []);

  const current = sessions.find((s) => s.id === sessionId);
  const title = current?.title || "New task";
  const model =
    modelChoice ||
    status?.model.name ||
    status?.model.default ||
    "Choose model";
  const ctx = status?.model.context_limit
    ? Math.min(
        100,
        Math.round(
          ((transcript.usage.prompt_tokens || 0) / status.model.context_limit) *
            100,
        ),
      )
    : 0;
  const slashHits = slashOpen
    ? commands.filter((c) => c.name.startsWith(task.slice(1))).slice(0, 8)
    : [];
  const empty =
    !transcript.items.length && !commandCards.length && !busy && !submitting;
  const completedSteps = transcript.plan.filter(
    (p) => p.status === "done",
  ).length;
  if (!ready)
    return (
      <div className="boot">
        <img src="/icon.svg" alt="" />
        <span>Opening your workspace…</span>
        <LoaderCircle className="spin" size={18} />
      </div>
    );
  if (needsOnboard)
    return (
      <Onboarding
        onDone={() => {
          setNeedsOnboard(false);
          void boot();
        }}
      />
    );

  return (
    <div
      className={`app ${sidebar ? "with-sidebar" : ""} ${panel ? "drawer-open" : ""}`}
    >
      {sidebar && (
        <Sidebar
          sessions={sessions}
          projects={projects}
          selected={sessionId}
          workspace={workspace}
          jobs={jobs}
          onSelect={(id) => void openSession(id)}
          onNew={() => void newSession()}
          onProject={(path) => void pickProject(path)}
          onPanel={setPanel}
          onSettings={() => setOverlay("settings")}
          onHide={() => setSidebar(false)}
        />
      )}
      <header className="top">
        {!sidebar && (
          <button
            type="button"
            className="icon-btn"
            aria-label="Show sidebar"
            title="Show sidebar (Ctrl+B)"
            onClick={() => setSidebar(true)}
          >
            <PanelLeft size={18} />
          </button>
        )}
        <button
          type="button"
          className="project-crumb"
          title={workspace || "Open project"}
          onClick={() => setOverlay("project")}
        >
          <FolderOpen size={15} />
          <span>{workspace.split("/").pop() || "Open project"}</span>
        </button>
        <ChevronRight size={13} className="dim" />
        <span className="top-title" title={title}>
          {title}
        </span>
        <div className="top-right">
          <button
            type="button"
            className={`top-action ${panel === "changes" ? "on" : ""}`}
            onClick={() => setPanel(panel === "changes" ? null : "changes")}
          >
            <GitPullRequest size={15} />
            <span>Review</span>
            {git.count > 0 && <span className="count">{git.count}</span>}
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Browse files"
            aria-label="Browse files"
            onClick={() => setPanel(panel === "files" ? null : "files")}
          >
            <FileCode2 size={17} />
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Terminal"
            aria-label="Terminal"
            onClick={() => setPanel(panel === "terminal" ? null : "terminal")}
          >
            <TerminalSquare size={17} />
          </button>
          <span className="top-divider" />
          <button
            type="button"
            className="icon-btn"
            title="Command palette (Ctrl+K)"
            aria-label="Command palette"
            onClick={() => setOverlay("palette")}
          >
            <Search size={16} />
          </button>
        </div>
      </header>
      <main className="stage">
        {job?.status === "interrupted" && (
          <div className="connection-banner" role="status">
            <span>
              This task was interrupted. Its recorded work is available below.
            </span>
            <button
              type="button"
              className="mini"
              onClick={() => {
                setTask(
                  `Continue the interrupted task. Inspect the current files and verify the remaining work. Original request: ${job.task}`,
                );
                promptRef.current?.focus();
              }}
            >
              Continue task
            </button>
          </div>
        )}
        {connection === "reconnecting" && (
          <div className="connection-banner" role="status">
            <LoaderCircle size={14} className="spin" />
            Reconnecting… Your task continues in the background.
          </div>
        )}
        <div
          className="chat-stream"
          ref={streamRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            stick.current =
              el.scrollHeight - el.scrollTop - el.clientHeight < 100;
            setAtBottom(stick.current);
          }}
        >
          <div className={`chat-inner ${empty ? "is-empty" : ""}`}>
            {error && (
              <div className="notice bad" role="alert">
                <span>{error}</span>
                <button
                  type="button"
                  className="mini"
                  onClick={() => void boot()}
                >
                  Reconnect
                </button>
                <button
                  type="button"
                  className="icon-btn"
                  aria-label="Dismiss error"
                  onClick={() => setError("")}
                >
                  <X size={14} />
                </button>
              </div>
            )}
            {switching ? (
              <div className="loading-task">
                <LoaderCircle size={20} className="spin" />
                Opening task…
              </div>
            ) : empty ? (
              <div className="welcome">
                <div className="welcome-symbol">
                  <img src="/icon.svg" alt="" />
                </div>
                <div className="eyebrow">
                  YOUR IDEAS. YOUR MODELS. YOUR MACHINE.
                </div>
                <h1>What are we building?</h1>
                <p>A focused space to turn intent into working code.</p>
                <button
                  type="button"
                  className="workspace-badge"
                  title={workspace}
                  onClick={() => setOverlay("project")}
                >
                  <FolderOpen size={14} />
                  {workspace.split("/").pop() || "Choose a project"}
                  <ChevronRight size={13} />
                </button>
                <div className="suggestions">
                  {[
                    {
                      icon: Code2,
                      title: "Build something",
                      body: "Turn an idea into a first version",
                      prompt: "Help me build ",
                    },
                    {
                      icon: GitPullRequest,
                      title: "Review this project",
                      body: "Find bugs and practical improvements",
                      prompt:
                        "Review this project for bugs and reliability issues. Explain your findings before changing files.",
                    },
                    {
                      icon: Sparkles,
                      title: "Understand the code",
                      body: "Find your way around the workspace",
                      prompt:
                        "Explore this workspace and explain its architecture, key entry points, and how to run it.",
                    },
                  ].map((s) => (
                    <button
                      type="button"
                      className="suggestion"
                      key={s.title}
                      onClick={() => {
                        setTask(s.prompt);
                        promptRef.current?.focus();
                      }}
                    >
                      <s.icon size={18} />
                      <strong>
                        {s.title}
                        <ArrowUpRight size={13} />
                      </strong>
                      <span>{s.body}</span>
                    </button>
                  ))}
                </div>
              </div>
            ) : (
              <>
                {transcript.items.map((item, i) =>
                  item.kind === "tool" ? (
                    <OpCard
                      key={i}
                      item={item}
                      onToggle={() =>
                        setTranscript((s) => ({
                          ...s,
                          items: s.items.map((it, n) =>
                            n === i && it.kind === "tool"
                              ? { ...it, collapsed: it.collapsed === false }
                              : it,
                          ),
                        }))
                      }
                      onRewind={(tid) => void rewind(tid)}
                      onReviewDiff={(path) => {
                        setDiffPath(path);
                        setPanel("changes");
                      }}
                    />
                  ) : item.kind === "user" ? (
                    <div key={i} className="msg-user">
                      <div className="user-pill">
                        <div className="bubble">{item.text}</div>
                      </div>
                    </div>
                  ) : (
                    <div key={i} className="msg-agent">
                      {item.who && <div className="who">{item.who}</div>}
                      <Markdown>{item.text}</Markdown>
                    </div>
                  ),
                )}
                {commandCards.map((card, i) => (
                  <CommandCardView key={`c${i}`} card={card} />
                ))}
              </>
            )}
            {approvals.map((a) => (
              <ApprovalCard
                key={a.id}
                approval={a}
                onDecide={(id, decision) => void decide(id, decision)}
              />
            ))}
            {(busy || submitting) && !switching && (
              <div className="working" role="status">
                <LoaderCircle size={15} className="spin" />
                <span>
                  {submitting
                    ? "Starting task"
                    : job?.status === "cancelling"
                      ? "Stopping safely"
                      : transcript.stage === "UNDERSTAND"
                        ? "Exploring your request"
                        : transcript.stage.toLowerCase().replaceAll("_", " ")}
                </span>
                <span className="dim">
                  {elapsed >= 60
                    ? `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`
                    : `${elapsed}s`}
                </span>
              </div>
            )}
          </div>
        </div>
        {!atBottom && (
          <button
            type="button"
            className="jump-latest"
            onClick={() => {
              stick.current = true;
              streamRef.current?.scrollTo({
                top: streamRef.current.scrollHeight,
                behavior: "smooth",
              });
            }}
          >
            <ArrowDown size={14} />
            Latest activity
          </button>
        )}
        <div
          className="composer-wrap"
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            void attach(e.dataTransfer.files);
          }}
        >
          {transcript.plan.length > 0 && (
            <details className="task-plan">
              <summary>
                <ListChecks size={15} />
                <span>Task plan</span>
                <span className="dim">
                  {completedSteps} of {transcript.plan.length}
                </span>
                <div className="plan-track">
                  <span
                    style={{
                      width: `${(completedSteps / transcript.plan.length) * 100}%`,
                    }}
                  />
                </div>
              </summary>
              <ol>
                {transcript.plan.map((p) => (
                  <li key={p.id} className={`plan-${p.status}`}>
                    {p.status === "done" ? (
                      <Check size={14} />
                    ) : (
                      <span className="plan-circle" />
                    )}
                    <span>{p.title}</span>
                    <small>{p.status}</small>
                  </li>
                ))}
              </ol>
            </details>
          )}
          {slashOpen && (
            <div
              className="slash-menu"
              role="listbox"
              aria-label="Slash commands"
            >
              {slashHits.length ? (
                slashHits.map((c, i) => (
                  <button
                    type="button"
                    role="option"
                    aria-selected={i === slashIndex}
                    className={`slash-hit ${i === slashIndex ? "on" : ""}`}
                    key={c.name}
                    onClick={() => {
                      setTask(`/${c.name}${c.arg_spec ? " " : ""}`);
                      setSlashOpen(false);
                      promptRef.current?.focus();
                    }}
                  >
                    <strong>/{c.name}</strong>
                    <span>{c.description}</span>
                  </button>
                ))
              ) : (
                <div className="slash-empty">No matching commands</div>
              )}
            </div>
          )}
          <form
            className={`composer ${locked ? "is-working" : ""}`}
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            {chips.length > 0 && (
              <div className="chips">
                {chips.map((c) => (
                  <button
                    type="button"
                    className="path-chip"
                    key={c}
                    onClick={() => setChips(chips.filter((x) => x !== c))}
                  >
                    <FileCode2 size={12} />
                    {c.split("/").pop()}
                    <X size={12} />
                  </button>
                ))}
              </div>
            )}
            <textarea
              ref={promptRef}
              aria-label="Message ShadowCode"
              value={task}
              rows={2}
              placeholder={
                busy
                  ? "Draft your next step while ShadowCode works…"
                  : empty
                    ? "Describe what you want to build…"
                    : "Ask for a follow-up change…"
              }
              onChange={(e) => {
                setTask(e.target.value);
                setSlashIndex(0);
                setSlashOpen(/^\/\S*$/.test(e.target.value));
              }}
              onKeyDown={(e) => {
                if (slashOpen && slashHits.length) {
                  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                    e.preventDefault();
                    setSlashIndex(
                      (i) =>
                        (i +
                          (e.key === "ArrowDown" ? 1 : slashHits.length - 1)) %
                        slashHits.length,
                    );
                    return;
                  }
                  if (e.key === "Tab") {
                    e.preventDefault();
                    setTask(`/${slashHits[slashIndex].name} `);
                    setSlashOpen(false);
                    return;
                  }
                }
                if (
                  e.key === "Enter" &&
                  !e.shiftKey &&
                  !e.nativeEvent.isComposing
                ) {
                  e.preventDefault();
                  void submit();
                }
              }}
            />
            <div className="composer-footer">
              <button
                type="button"
                className="icon-btn attach-btn"
                aria-label="Attach text files"
                title="Attach text files"
                onClick={() => fileRef.current?.click()}
              >
                <Paperclip size={17} />
              </button>
              <input
                ref={fileRef}
                type="file"
                multiple
                hidden
                onChange={(e) => {
                  if (e.target.files) void attach(e.target.files);
                  e.target.value = "";
                }}
              />
              <select
                className="model-select"
                aria-label="Model for this task"
                value={modelChoice}
                onChange={(e) => {
                  if (e.target.value === "__custom__")
                    setOverlay("custom-model");
                  else setModelChoice(e.target.value);
                }}
              >
                <option value="">
                  {status?.model.name ||
                    status?.model.default ||
                    "Choose model"}
                </option>
                {[...new Set(models.map((m) => m.provider))].map((p) => (
                  <optgroup key={p} label={p}>
                    {models
                      .filter((m) => m.provider === p)
                      .map((m) => (
                        <option key={m.id} value={m.id}>
                          {m.name || m.id}
                          {m.detected ? " · local" : ""}
                        </option>
                      ))}
                  </optgroup>
                ))}
                <option value="__custom__">Custom model…</option>
              </select>
              <span className="control-divider" />
              <select
                className="mode-select"
                aria-label="Agent mode"
                value={mode}
                onChange={(e) => setMode(e.target.value)}
              >
                <option value="coder">Build</option>
                <option value="researcher">Research</option>
                <option value="reviewer">Review</option>
                <option value="tester">Test</option>
              </select>
              <span className="grow" />
              <span className="composer-hint">
                {task ? "↵ Send" : "/ for commands"}
              </span>
              {busy ? (
                <button
                  type="button"
                  className="submit-btn stop"
                  aria-label="Stop task"
                  title="Stop task (Ctrl+.)"
                  disabled={job?.status === "cancelling"}
                  onClick={() => void stop()}
                >
                  <Square size={14} fill="currentColor" />
                </button>
              ) : (
                <button
                  type="submit"
                  className="submit-btn"
                  aria-label="Send task"
                  title="Send task"
                  disabled={locked || (!task.trim() && !chips.length)}
                >
                  {submitting ? (
                    <LoaderCircle size={17} className="spin" />
                  ) : (
                    <ArrowUp size={19} />
                  )}
                </button>
              )}
            </div>
          </form>
          <div className="composer-note">
            <ShieldCheck size={12} />
            <span>
              {status?.permissions.level === "read_only"
                ? "Read-only workspace"
                : "Workspace access"}
              <span className="note-sep">·</span>Review changes as you go
            </span>
            <button type="button" onClick={() => setOverlay("help")}>
              Keyboard shortcuts
            </button>
          </div>
        </div>
        <footer className="statusline">
          <span className={`status-dot ${busy ? "active" : ""}`} />
          <span>
            {busy
              ? "Working"
              : connection === "reconnecting"
                ? "Reconnecting"
                : "Ready"}
          </span>
          <span className="sep">/</span>
          <span title={model}>{model}</span>
          <span className="grow" />
          <span title="Input tokens as a percentage of the configured context limit">
            Context {ctx}%
          </span>
          <span className="sep">·</span>
          <span>{formatTokens(transcript.usage.total_tokens || 0)} tokens</span>
          {git.branch && (
            <button
              type="button"
              title="Review git changes"
              onClick={() => setPanel("changes")}
            >
              <GitBranch size={12} />
              {git.branch}
            </button>
          )}
          <span className="version">v{health?.version || "0.19.0"}</span>
        </footer>
      </main>
      {panel && (
        <Drawer
          key={`${workspace}:${panel}`}
          tab={panel}
          onTab={setPanel}
          onClose={() => setPanel(null)}
          workspace={workspace}
          sessions={sessions}
          sessionId={sessionId}
          onOpenSession={(id) => void openSession(id)}
          onNewSession={() => void newSession()}
          onRefreshSessions={async () => {
            await refresh();
            const rows = (await api.sessions()).sessions;
            if (
              selectedRef.current &&
              !rows.some((row) => row.id === selectedRef.current)
            ) {
              const next = rows.find((row) => row.workspace === workspace);
              if (next) await openSession(next.id);
              else await newSession();
            }
          }}
          diffPath={diffPath}
          onDiffPath={setDiffPath}
          health={health}
          busy={busy}
          toast={toast}
        />
      )}
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            <span>{t.text}</span>
            <button
              type="button"
              className="icon-btn"
              aria-label="Dismiss notification"
              onClick={() =>
                setToasts((prev) => prev.filter((x) => x.id !== t.id))
              }
            >
              <X size={13} />
            </button>
          </div>
        ))}
      </div>
      {trust && (
        <TrustDialog
          req={trust}
          onCancel={() => setTrust(null)}
          onConfirm={() => void confirmTrust()}
        />
      )}
      {overlay === "settings" && (
        <Settings
          cfg={cfg}
          onClose={() => setOverlay("")}
          onToast={toast}
          onSave={async (values, key, env) => {
            await api.saveConfig(values, key, env);
            await reloadConfig();
            setOverlay("");
            toast("Settings saved", "ok");
          }}
        />
      )}
      {overlay === "help" && (
        <Help onClose={() => setOverlay("")} version={health?.version || ""} />
      )}
      {overlay === "palette" && (
        <Palette items={palette} onClose={() => setOverlay("")} />
      )}
      {overlay === "project" && (
        <ProjectPicker
          projects={projects}
          current={workspace}
          onClose={() => setOverlay("")}
          onPick={(path) => void pickProject(path)}
        />
      )}
      {overlay === "custom-model" && (
        <CustomModelDialog
          providers={providers}
          onClose={() => setOverlay("")}
          onSubmit={async (id, provider, endpoint) => {
            try {
              await api.selectModel(id, { provider, endpoint, name: id });
              await reloadConfig();
              setModelChoice("");
              setOverlay("");
              toast("Model updated", "ok");
            } catch (e) {
              toast(String(e), "err");
            }
          }}
        />
      )}
    </div>
  );
}
