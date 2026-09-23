import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowDown,
  Check,
  ChevronRight,
  FolderOpen,
  GitBranch,
  GitPullRequest,
  ListChecks,
  LoaderCircle,
  PanelLeft,
  Search,
  X,
} from "lucide-react";
import {
  api,
  type Approval,
  type CommandResult,
  type ConsentRequest,
  type Health,
  type Job,
  type Project,
  type Session,
  type StartJobRequest,
} from "./api";
import { ApprovalCard, CommandCardView, OpCard } from "./components/cards";
import { Drawer, type DrawerTab } from "./components/Drawer";
import { Sidebar } from "./components/Sidebar";
import { Markdown } from "./components/Markdown";
import { Onboarding } from "./components/Onboarding";
import { WelcomeBanner } from "./components/WelcomeBanner";
import { ActivityTimeline } from "./components/ActivityTimeline";
import { TaskSummary, type DiffStat } from "./components/TaskSummary";
import { ConsentDialog } from "./components/ConsentDialog";
import { Composer } from "./components/Composer";
import {
  NetworkPill,
  PermissionControl,
  WebToggle,
  type PermissionMode,
} from "./components/ComposerControls";
import {
  Help,
  Palette,
  ProjectPicker,
  TrustDialog,
  type PaletteItem,
} from "./components/overlays";
import {
  Settings,
  type AdvancedTab,
  type SettingsSection,
} from "./components/Settings";
import { QueuedTasks } from "./components/QueuedTasks";
import { TaskSteerBar } from "./components/TaskSteerBar";
import { UnifiedPicker } from "./components/UnifiedPicker";
import { useConversation } from "./hooks/useConversation";
import {
  isLocal,
  isReady,
  rememberRecent,
  vendorKey,
  type PickerTarget,
} from "./lib/picker";
import {
  checkAttachment,
  sendBlockedByImages,
  toBase64,
  type Attachment,
} from "./lib/attachments";
import { formatDuration } from "./lib/activity";
import { conversationJob } from "./lib/jobs";
import {
  isProjectTrustError,
  sameWorkspacePath,
  trustErrorHint,
  trustPromptFor,
  trustRequestFor,
} from "./lib/trust";
import {
  exportSession as saveExport,
  invoke,
  isNative,
  listen,
  openExternal,
} from "./lib/transport";

type Overlay = "" | "settings" | "help" | "palette" | "project";
type Toast = { id: number; text: string; kind: "ok" | "err" | "info" };
type Consent = {
  request: ConsentRequest;
  body: StartJobRequest;
  original: { task: string; attachments: Attachment[] };
};
const formatTokens = (n: number) =>
  n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
const draftKey = (id: string, workspace: string) =>
  `shadow:draft:${id || workspace}`;
const targetKey = (workspace: string) => `shadow:model:${workspace}`;
const isSessionCommand = (text: string) =>
  ["/new", "/clear"].includes(text.trim());
const readStore = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const writeStore = (key: string, value: string | null) => {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* Browser storage only holds conveniences. */
  }
};
const ADVANCED_PANELS: Record<string, AdvancedTab> = {
  goals: "goals",
  skills: "skills",
  health: "health",
  doctor: "health",
  background: "background",
};

export default function App() {
  const [ready, setReady] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [pickerTargets, setPickerTargets] = useState<PickerTarget[]>([]);
  const [pickerLoaded, setPickerLoaded] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [forking, setForking] = useState(false);
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
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [settings, setSettings] = useState<{
    section: SettingsSection;
    advanced?: AdvancedTab;
    vendor?: string;
  }>({ section: "accounts" });
  const [commandCards, setCommandCards] = useState<CommandResult[]>([]);
  const [approvals, setApprovals] = useState<Approval[]>([]);
  const [modelChoice, setModelChoice] = useState("");
  const [runningChoice, setRunningChoice] = useState("");
  const [webEnabled, setWebEnabled] = useState(
    () => readStore("shadow:web") === "on",
  );
  const [consent, setConsent] = useState<Consent | null>(null);
  const [sidebar, setSidebar] = useState(
    () => readStore("shadow:sidebar") !== "closed" && window.innerWidth > 760,
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
  const [shutdown, setShutdown] = useState<{
    status: string;
    message?: string;
  } | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [cancellingQueued, setCancellingQueued] = useState<string[]>([]);
  const [switching, setSwitching] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  const [git, setGit] = useState<{ branch: string; count: number }>({
    branch: "",
    count: 0,
  });
  const [elapsed, setElapsed] = useState(0);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const streamRef = useRef<HTMLDivElement>(null);
  const selectedRef = useRef("");
  const selection = useRef(0);
  const submittingRef = useRef(false);
  const taskRef = useRef("");
  const toastSeq = useRef(0);
  const booted = useRef(false);
  const activationQueue = useRef<Promise<unknown>>(Promise.resolve());
  const stick = useRef(true);
  const browsingHistory = useRef(false);
  const pickerFetched = useRef(0);
  const pickerSeq = useRef(0);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = ++toastSeq.current;
    setToasts((prev) => [...prev.slice(-3), { id, text, kind }]);
    const duration = kind === "err" ? 8000 : 5000;
    setTimeout(
      () => setToasts((prev) => prev.filter((t) => t.id !== id)),
      duration,
    );
  }, []);

  function openSettings(
    section: SettingsSection = "accounts",
    extra: { advanced?: AdvancedTab; vendor?: string } = {},
  ) {
    setSettings({ section, ...extra });
    setOverlay("settings");
  }

  useEffect(() => {
    let stopped = false;
    let unsubscribe: (() => void) | undefined;
    if (isNative())
      void listen("shadowcode:shutdown", (payload) =>
        setShutdown(payload as { status: string; message?: string }),
      )
        .then((stop) => {
          if (stopped) stop();
          else unsubscribe = stop;
        })
        .catch((error) => {
          if (!stopped) toast(String(error), "err");
        });
    const external = (event: MouseEvent) => {
      const anchor = (
        event.target as Element | null
      )?.closest<HTMLAnchorElement>("a[href]");
      if (!anchor || anchor.getAttribute("href")?.startsWith("#")) return;
      event.preventDefault();
      void openExternal(anchor.href).catch((error) =>
        toast(String(error), "err"),
      );
    };
    document.addEventListener("click", external, true);
    return () => {
      stopped = true;
      unsubscribe?.();
      document.removeEventListener("click", external, true);
    };
  }, [toast]);

  const reloadPicker = useCallback(
    async (refresh = false) => {
      pickerFetched.current = Date.now();
      // Requests can overlap (vendor checks are slow); only the newest one
      // may replace the rows, or an older answer would hide a model that was
      // just added.
      const seq = ++pickerSeq.current;
      try {
        const result = await api.picker(refresh);
        if (seq !== pickerSeq.current) return;
        setPickerTargets(Array.isArray(result.targets) ? result.targets : []);
      } catch (e) {
        if (seq === pickerSeq.current)
          toast(`Could not load models: ${String(e)}`, "err");
      } finally {
        if (seq === pickerSeq.current) setPickerLoaded(true);
      }
    },
    [toast],
  );

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
    void reloadPicker();
  });
  const { transcript, setTranscript, job, busy, connection } = conversation;
  const locked = busy || submitting || switching || Boolean(shutdown);
  const projectBusy =
    busy ||
    jobs.some(
      (item) =>
        item.workspace === workspace &&
        ["queued", "running", "cancelling"].includes(item.status),
    );
  const queueing = projectBusy;
  const composerLocked = submitting || switching || Boolean(shutdown);
  const queuedJobs = jobs
    .filter((item) => item.workspace === workspace && item.status === "queued")
    .slice()
    .reverse();
  const commandWaiting = queueing && task.trim().startsWith("/");
  taskRef.current = task;

  async function reloadConfig() {
    const [config, state] = await Promise.all([api.config(), api.status()]);
    setCfg(config);
    setStatus(state);
  }

  async function openSession(id: string) {
    if (submittingRef.current) return;
    if (selectedRef.current) {
      const draft = taskRef.current;
      if (draft && !isSessionCommand(draft))
        writeStore(draftKey(selectedRef.current, workspace), draft);
      else if (!draft)
        writeStore(draftKey(selectedRef.current, workspace), null);
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
      writeStore("shadow:selected", id);
      conversation.load(detail, active.job);
      // The conversation's own target wins; a new conversation starts on the
      // project's last choice. Nothing falls back to a configured default.
      setModelChoice(
        detail.execution_target || readStore(targetKey(detail.workspace)) || "",
      );
      setRunningChoice(
        active.job?.model || active.job?.routing?.requested || "",
      );
      setTask(readStore(draftKey(id, detail.workspace)) || "");
      setAttachments([]);
      setCommandCards([]);
      setApprovals([]);
      stick.current = true;
      setAtBottom(true);
      await refresh();
    } catch (e) {
      if (ticket === selection.current) {
        setError(String(e));
        toast(String(e), "err");
      }
    } finally {
      if (ticket === selection.current) {
        stick.current = true;
        setAtBottom(true);
        setSwitching(false);
      }
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
      void reloadPicker();
      await refresh();
      const saved = readStore("shadow:selected");
      const initial =
        sessionData.sessions.find((s) => s.id === saved) ||
        sessionData.sessions.find((s) => s.workspace === h.workspace);
      if (onboard.completed && initial) await openSession(initial.id);
      else setModelChoice(readStore(targetKey(h.workspace)) || "");
      const latest = await api.status();
      setStatus(latest);
      const prompt = trustPromptFor(
        latest.workspace || h.workspace,
        latest.trusted ?? h.trusted,
        latest.permissions || h.permissions,
      );
      // First run trusts the folder in onboarding; asking again behind it
      // would show a second trust prompt once onboarding closes.
      setTrust(onboard.completed ? prompt : null);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // Theme: explicit light/dark, or follow the system when unset or "system".
  useEffect(() => {
    const preference = String(
      (cfg.ui as { theme?: string })?.theme || "system",
    );
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.dataset.theme =
        preference === "light" || preference === "dark"
          ? preference
          : media?.matches
            ? "dark"
            : "light";
    };
    apply();
    if (preference === "light" || preference === "dark" || !media) return;
    media.addEventListener?.("change", apply);
    return () => media.removeEventListener?.("change", apply);
  }, [cfg]);
  useEffect(() => {
    writeStore("shadow:sidebar", sidebar ? "open" : "closed");
  }, [sidebar]);
  useEffect(() => {
    const compact = window.matchMedia("(max-width: 760px)");
    const resize = (event: MediaQueryListEvent) => {
      if (event.matches) setSidebar(false);
    };
    compact.addEventListener("change", resize);
    return () => compact.removeEventListener("change", resize);
  }, []);
  // Readiness and usage change outside the app (sign-in in a browser, plan
  // resets): refresh rows when the window regains focus.
  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - pickerFetched.current > 15000) void reloadPicker();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reloadPicker]);
  useEffect(() => {
    if (transcript.usageVersion) void reloadPicker();
  }, [transcript.usageVersion, reloadPicker]);
  useEffect(() => {
    document.title = `${busy ? "● " : ""}ShadowCode`;
  }, [busy]);
  useEffect(() => {
    const key = draftKey(sessionId, workspace);
    const timer = setTimeout(() => {
      if (isSessionCommand(task)) return;
      writeStore(key, task || null);
    }, 200);
    return () => clearTimeout(timer);
  }, [task, sessionId, workspace]);
  useLayoutEffect(() => {
    if (ready && !switching && stick.current)
      streamRef.current?.scrollTo({ top: streamRef.current.scrollHeight });
  }, [
    transcript.items,
    transcript.activity,
    commandCards,
    busy,
    submitting,
    approvals,
    ready,
    switching,
    queuedJobs.length,
  ]);
  useLayoutEffect(() => {
    if (conversation.history.viewing) {
      stick.current = false;
      streamRef.current?.scrollTo({ top: 0 });
    } else if (browsingHistory.current) {
      stick.current = true;
      setAtBottom(true);
      streamRef.current?.scrollTo({ top: streamRef.current.scrollHeight });
    }
    browsingHistory.current = conversation.history.viewing;
  }, [conversation.history.firstCursor, conversation.history.viewing]);
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
    const timer = setInterval(poll, busy ? 1200 : 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [sessionId, busy]);
  // Queued follow-ups can start after the visible job has finished. Reload
  // before attaching their stream so intervening milestones are retained.
  useEffect(() => {
    if (!sessionId || busy || submitting || switching) return;
    const sessionJobs = jobs.filter((item) => item.session_id === sessionId);
    const next = conversationJob(sessionJobs, job);
    if (!next || next.id === job?.id) return;
    let live = true;
    void Promise.all([api.session(sessionId), api.job(next.id)])
      .then(([detail, fullJob]) => {
        if (
          live &&
          selectedRef.current === sessionId &&
          !submittingRef.current
        ) {
          conversation.load(detail, fullJob, true);
          setRunningChoice(fullJob.model || fullJob.routing?.requested || "");
        }
      })
      .catch(() => {
        /* The next poll retries a failed snapshot. */
      });
    return () => {
      live = false;
    };
  }, [jobs, sessionId, busy, submitting, switching, job, conversation.load]);
  useEffect(() => {
    if (!job || !busy) return;
    const tick = () =>
      setElapsed(Math.max(0, Math.floor(Date.now() / 1000 - job.started_at)));
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [job?.id, busy]);

  const selectedTarget = pickerTargets.find((t) => t.id === modelChoice);
  const canAttachImages = selectedTarget?.vision === true;
  const permissions = (cfg.permissions || {}) as Record<string, unknown>;
  const permissionMode: PermissionMode =
    permissions.mode === "allow_edits" ? "allow_edits" : "ask";
  const readOnly =
    (status?.permissions.level || permissions.level) === "read_only";
  const networkMode = String(
    ((cfg.network || {}) as { mode?: string }).mode || "online",
  );
  const vendorNotes = (permissions.vendor_notes || {}) as Record<
    string,
    string
  >;
  const vendorNote =
    selectedTarget && !isLocal(selectedTarget)
      ? vendorNotes[vendorKey(selectedTarget)] ||
        vendorNotes[`cli-${vendorKey(selectedTarget)}`] ||
        `${selectedTarget.name.split(" · ")[0]} runs its own tools, sandbox and web access; ShadowCode passes this choice to it where the tool supports it.`
      : undefined;
  const webAllowed =
    Boolean(selectedTarget && isLocal(selectedTarget)) &&
    networkMode === "online";

  async function selectTarget(id: string) {
    setModelChoice(id);
    rememberRecent(id);
    if (workspace) writeStore(targetKey(workspace), id);
    if (sessionId)
      await api.setSessionTarget(sessionId, id).catch(() => {
        /* The choice still applies: every job carries its target. */
      });
  }

  async function newSession(opts?: { force?: boolean }) {
    if (!opts?.force && (submitting || switching)) return;
    if (!workspace) {
      setOverlay("project");
      return;
    }
    try {
      const created = await api.createSession(workspace);
      writeStore("shadow:selected", created.id);
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
      const [latest, h] = await Promise.all([api.status(), api.health()]);
      setStatus(latest);
      setHealth(h);
      if (latest.trusted === false) {
        toast(
          "Trust did not persist. Click Trust and open again, then send the task.",
          "err",
        );
        return;
      }
      setTrust(null);
      await reloadConfig();
      if (
        sessionId &&
        sameWorkspacePath(workspace, opened.path || latest.workspace)
      ) {
        toast("Project trusted. You can send a task.", "ok");
      } else if (opened.session_id) {
        await openSession(opened.session_id);
      } else {
        toast("Project trusted. You can send a task.", "ok");
      }
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function stop() {
    if (!job || !busy) return;
    try {
      const next = await api.cancelJob(job.id);
      if (selectedRef.current === next.session_id) {
        const detail = await api.session(next.session_id);
        if (selectedRef.current === next.session_id)
          conversation.load(detail, next);
      }
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function cancelQueued(queued: Job) {
    if (cancellingQueued.includes(queued.id)) return;
    setCancellingQueued((ids) => [...ids, queued.id]);
    try {
      const updated = await api.cancelJob(queued.id, true);
      setJobs((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
      toast("Queued task cancelled.", "info");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setCancellingQueued((ids) => ids.filter((id) => id !== queued.id));
      void refresh().catch(() => undefined);
    }
  }
  async function decide(
    id: string,
    decision: "approve" | "deny",
    approvalSession?: string,
  ) {
    try {
      await api.decide(id, decision, approvalSession);
      const selected = selectedRef.current;
      const next = await api.approvals(selected);
      if (selectedRef.current === selected) setApprovals(next.approvals);
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function runSlash(text: string) {
    const ticket = selection.current;
    const originSession = selectedRef.current;
    const [name, ...rest] = text.slice(1).split(" ");
    const args = rest.join(" ");
    if (name === "new" || name === "clear") {
      await newSession();
      return;
    }
    if (name === "model") {
      // The picker is the only place a model is chosen.
      setPickerOpen(true);
      return;
    }
    const drawers: Record<string, DrawerTab> = {
      sessions: "sessions",
      diff: "changes",
      changes: "changes",
      files: "files",
      terminal: "terminal",
    };
    if (drawers[name] && !args) {
      setPanel(drawers[name]);
      return;
    }
    if (ADVANCED_PANELS[name] && !args) {
      openSettings("advanced", { advanced: ADVANCED_PANELS[name] });
      return;
    }
    if (name === "settings" && !args) {
      openSettings();
      return;
    }
    const result = await api.runCommand(name, args, sessionId || undefined, {
      model: modelChoice || undefined,
    });
    if (ticket !== selection.current) {
      await refresh();
      return;
    }
    const metadata = result.metadata || {};
    const started = metadata.job as Job | undefined;
    if (started?.id) {
      if (started.session_id !== selectedRef.current) {
        selectedRef.current = started.session_id;
        setSessionId(started.session_id);
        writeStore("shadow:selected", started.session_id);
      }
      conversation.start(started);
    } else if (typeof metadata.session_id === "string") {
      await openSession(metadata.session_id);
    } else if (result.kind === "overlay") {
      if (["model", "picker"].includes(result.overlay)) setPickerOpen(true);
      else openSettings();
    } else if (metadata.action === "quit" || result.quit) {
      await invoke("desktop_quit");
    } else if (!metadata.panel) {
      if (originSession) {
        const [detail, current] = await Promise.all([
          api.session(originSession),
          api.currentJob(originSession),
        ]);
        if (ticket === selection.current)
          conversation.load(detail, current.job);
      } else setCommandCards((prev) => [...prev, result]);
    }
    if (typeof metadata.panel === "string") {
      const panelName = metadata.panel;
      if (drawers[panelName]) setPanel(drawers[panelName]);
      else if (["sessions", "files", "terminal", "changes"].includes(panelName))
        setPanel(panelName as DrawerTab);
      else if (ADVANCED_PANELS[panelName])
        openSettings("advanced", { advanced: ADVANCED_PANELS[panelName] });
    }
    if (metadata.reload_config) await reloadConfig();
    await refresh();
  }

  const sendBlocked = useMemo(() => {
    if (task.trim().startsWith("/")) return null;
    if (!pickerLoaded) return null;
    if (!selectedTarget)
      return modelChoice
        ? "The saved model is no longer available. Choose a model to send."
        : "Choose a model to send.";
    if (!isReady(selectedTarget))
      return `${selectedTarget.name}: ${selectedTarget.availability_label || "Unavailable"}${selectedTarget.reason ? ` · ${selectedTarget.reason}` : ""}`;
    return sendBlockedByImages(
      attachments,
      canAttachImages,
      selectedTarget.name,
    );
  }, [
    task,
    pickerLoaded,
    selectedTarget,
    modelChoice,
    attachments,
    canAttachImages,
  ]);
  const hasContent = Boolean(task.trim() || attachments.length);
  const canSend =
    !composerLocked &&
    !commandWaiting &&
    hasContent &&
    !sendBlocked &&
    (task.trim().startsWith("/") || Boolean(selectedTarget));

  async function startTask(
    body: StartJobRequest,
    original: { task: string; attachments: Attachment[] },
  ) {
    const submitTicket = selection.current;
    submittingRef.current = true;
    setSubmitting(true);
    setError("");
    try {
      const result = await api.startJob(body);
      if ("consent" in result) {
        if (submitTicket === selection.current)
          setConsent({ request: result.consent, body, original });
        return;
      }
      const started = result.job;
      if (submitTicket !== selection.current) {
        await refresh();
        return;
      }
      if (started.session_id !== selectedRef.current) {
        selectedRef.current = started.session_id;
        setSessionId(started.session_id);
        writeStore("shadow:selected", started.session_id);
      }
      for (const a of original.attachments)
        if (a.preview) URL.revokeObjectURL(a.preview);
      // Keep streaming the current task while a follow-up waits.
      if (!busy) {
        conversation.start(started);
        setRunningChoice(body.model || "");
      }
      if (body.queue)
        toast(
          "Follow-up queued. It will run after earlier project tasks.",
          "ok",
        );
      await refresh().catch(() => undefined);
    } catch (e) {
      if (submitTicket !== selection.current) {
        toast(String(e), "err");
        return;
      }
      setTask(original.task);
      setAttachments(original.attachments);
      setError(String(e));
      toast(String(e), "err");
      if (isProjectTrustError(e) && workspace)
        setTrust(trustRequestFor(workspace, status?.permissions));
    } finally {
      setSubmitting(false);
      submittingRef.current = false;
    }
  }

  async function submit() {
    if (composerLocked || submittingRef.current || !hasContent) return;
    if (commandWaiting) {
      setError(
        "Wait for this project's active work to finish before running a slash command. You can queue a message now.",
      );
      return;
    }
    if (isSessionCommand(task)) {
      setTask("");
      await newSession({ force: true });
      return;
    }
    const prompt = trustPromptFor(
      workspace || health?.workspace,
      status?.trusted ?? health?.trusted,
      status?.permissions || health?.permissions,
    );
    if (prompt) {
      setTrust(prompt);
      setError("");
      return;
    }
    const original = { task, attachments };
    if (task.trim().startsWith("/")) {
      setTask("");
      stick.current = true;
      submittingRef.current = true;
      setSubmitting(true);
      try {
        await runSlash(task.trim());
      } catch (e) {
        setTask(original.task);
        toast(String(e), "err");
      } finally {
        setSubmitting(false);
        submittingRef.current = false;
      }
      return;
    }
    // Re-check at send time: the row may have changed after attaching.
    if (!selectedTarget || !isReady(selectedTarget) || sendBlocked) {
      if (sendBlocked) toast(sendBlocked, "err");
      return;
    }
    const images = attachments
      .filter((a) => a.kind === "image")
      .map((a) => a.path);
    const texts = attachments
      .filter((a) => a.kind === "text")
      .map((a) => a.path);
    const text = (
      task.trim() +
      (texts.length ? `\n\nAttached paths: ${texts.join(", ")}` : "") +
      (images.length ? `\n\nAttached images: ${images.join(", ")}` : "")
    ).trim();
    setTask("");
    setAttachments([]);
    writeStore(draftKey(sessionId, workspace), null);
    stick.current = true;
    setAtBottom(true);
    await startTask(
      {
        task: text || "Describe the attached image(s).",
        workspace: workspace || undefined,
        session_id: sessionId || undefined,
        model: selectedTarget.id,
        purpose: "coder",
        queue: queueing,
        images,
        web: webAllowed && webEnabled,
      },
      original,
    );
  }

  async function attach(files: File[]) {
    if (composerLocked) return;
    let images = attachments.filter((a) => a.kind === "image").length;
    for (const file of files) {
      const check = checkAttachment(file, {
        vision: canAttachImages,
        images,
        modelName: selectedTarget?.name,
      });
      if (!check.ok) {
        toast(check.error, "err");
        continue;
      }
      try {
        if (check.kind === "image") {
          images += 1;
          const saved = await api.attachImage(file.name, await toBase64(file));
          const preview = URL.createObjectURL(file);
          setAttachments((prev) =>
            prev.some((a) => a.path === saved.path)
              ? prev
              : [
                  ...prev,
                  { path: saved.path, name: file.name, kind: "image", preview },
                ],
          );
        } else {
          const text = await file.text();
          if (text.includes("\0")) {
            toast(`${file.name}: attach a text, source, or image file.`, "err");
            continue;
          }
          const saved = await api.attach(file.name, text);
          setAttachments((prev) =>
            prev.some((a) => a.path === saved.path)
              ? prev
              : [...prev, { path: saved.path, name: file.name, kind: "text" }],
          );
        }
      } catch (e) {
        toast(String(e), "err");
      }
    }
  }
  function removeAttachment(path: string) {
    setAttachments((prev) => {
      const gone = prev.find((a) => a.path === path);
      if (gone?.preview) URL.revokeObjectURL(gone.preview);
      return prev.filter((a) => a.path !== path);
    });
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
  const diffStat = useCallback(async (path: string): Promise<DiffStat> => {
    const diff = await api.gitDiff(path);
    let add = 0;
    let del = 0;
    for (const hunk of [...(diff.hunks || []), ...(diff.staged_hunks || [])])
      for (const line of hunk.lines) {
        if (line.kind === "add") add++;
        else if (line.kind === "del") del++;
      }
    return { add, del };
  }, []);
  function reviewChanges(path?: string) {
    setDiffPath(path || "");
    setPanel("changes");
  }
  function exportSession(format: "md" | "json" = "md") {
    if (sessionId)
      void saveExport(sessionId, format).catch((e) => toast(String(e), "err"));
  }
  async function setPermissionMode(mode: PermissionMode) {
    try {
      await api.saveConfig({ permissions: { mode } });
      await reloadConfig();
    } catch (e) {
      toast(String(e), "err");
    }
  }
  const palette: PaletteItem[] = [
    {
      id: "new",
      label: "New task",
      hint: "Ctrl+N",
      run: () => void newSession(),
    },
    {
      id: "model",
      label: "Choose a model",
      hint: "Ctrl+M",
      run: () => setPickerOpen(true),
    },
    {
      id: "project",
      label: "Open project",
      hint: "Ctrl+P",
      run: () => setOverlay("project"),
    },
    { id: "changes", label: "Review changes", run: () => setPanel("changes") },
    { id: "files", label: "Browse files", run: () => setPanel("files") },
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
    { id: "accounts", label: "Accounts", run: () => openSettings("accounts") },
    { id: "local", label: "Local models", run: () => openSettings("local") },
    {
      id: "goals",
      label: "Goals and milestones",
      run: () => openSettings("advanced", { advanced: "goals" }),
    },
    {
      id: "skills",
      label: "Skills and instructions",
      run: () => openSettings("advanced", { advanced: "skills" }),
    },
    {
      id: "health",
      label: "Workspace health",
      run: () => openSettings("advanced", { advanced: "health" }),
    },
    {
      id: "export",
      label: "Export this task as Markdown",
      hint: "Ctrl+Shift+E",
      run: exportSession,
    },
    {
      id: "export-json",
      label: "Export this task as JSON",
      hint: "Complete event records",
      run: () => exportSession("json"),
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
      run: () => openSettings(),
    },
    {
      id: "theme",
      label: "Toggle light / dark appearance",
      run: () =>
        void api
          .saveConfig({
            ui: {
              theme:
                document.documentElement.dataset.theme === "dark"
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
        if (consent) return;
        if (overlay) setOverlay("");
        else if (trust) setTrust(null);
        else if (pickerOpen) setPickerOpen(false);
        else if (panel) setPanel(null);
        return;
      }
      // Dialogs own the keyboard while they are open.
      if (overlay || trust || consent || needsOnboard) return;
      if (mod && key === "k") {
        e.preventDefault();
        setOverlay("palette");
      } else if (mod && key === "b" && !e.shiftKey) {
        e.preventDefault();
        setSidebar((v) => !v);
      } else if (mod && e.shiftKey && key === "b") {
        e.preventDefault();
        setPanel((p) => (p ? null : "changes"));
      } else if (mod && key === ",") {
        e.preventDefault();
        openSettings();
      } else if (mod && key === "p") {
        e.preventDefault();
        setOverlay("project");
      } else if (mod && key === "n") {
        e.preventDefault();
        void newSession();
      } else if (mod && key === "m") {
        e.preventDefault();
        setPickerOpen(true);
      } else if (mod && key === "l") {
        e.preventDefault();
        promptRef.current?.focus();
      } else if (mod && key === ".") {
        e.preventDefault();
        void stop();
      } else if (mod && e.shiftKey && key === "e") {
        e.preventDefault();
        exportSession();
      } else if (key === "?" && !inField) {
        e.preventDefault();
        setOverlay("help");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  const reloadCommands = useCallback(async () => {
    const result = await api.commands();
    setCommands(result.commands);
  }, []);
  useEffect(() => {
    let current = true;
    api
      .commands()
      .then((result) => {
        if (current) setCommands(result.commands);
      })
      .catch(() => {
        if (current) setCommands([]);
      });
    return () => {
      current = false;
    };
  }, [workspace]);

  const currentSession = sessions.find((s) => s.id === sessionId);
  const title = currentSession?.title || "New task";
  const activeModel = job?.routing || transcript.routing;
  const contextLimit =
    activeModel && !activeModel.provider?.startsWith("cli:")
      ? activeModel.context_limit
      : 0;
  const ctx = contextLimit
    ? Math.min(
        100,
        Math.round(
          ((transcript.usage.prompt_tokens || 0) / contextLimit) * 100,
        ),
      )
    : 0;
  const empty =
    !transcript.items.length &&
    !commandCards.length &&
    !busy &&
    !submitting &&
    !switching;
  const completedSteps = transcript.plan.filter(
    (p) => p.status === "done",
  ).length;
  const activeTaskId = transcript.activeTaskId || job?.task_id || "";
  const lastIndexByTask = useMemo(() => {
    const map: Record<string, number> = {};
    transcript.items.forEach((item, index) => {
      if (item.taskId) map[item.taskId] = index;
    });
    return map;
  }, [transcript.items]);
  // A finished task reads top to bottom: what it did (timeline), what it
  // says (answer), what changed (summary card).
  const timelineBefore = useMemo(() => {
    const summarized = new Set(
      transcript.items.flatMap((item) =>
        item.kind === "summary" ? [item.taskId] : [],
      ),
    );
    const map: Record<string, number> = {};
    transcript.items.forEach((item, index) => {
      if (
        item.kind === "agent" &&
        item.taskId &&
        summarized.has(item.taskId) &&
        !(item.taskId in map)
      )
        map[item.taskId] = index;
    });
    return map;
  }, [transcript.items]);
  const pendingNote =
    busy && modelChoice && runningChoice && modelChoice !== runningChoice
      ? "Applies to your next message"
      : undefined;
  const [bootTimeout, setBootTimeout] = useState(false);
  useEffect(() => {
    if (!ready) {
      const timer = setTimeout(() => setBootTimeout(true), 10000);
      return () => clearTimeout(timer);
    }
  }, [ready]);
  if (!ready)
    return (
      <div className="boot">
        <img src="/icon.svg" alt="" />
        <span>Opening your workspace…</span>
        <LoaderCircle className="spin" size={18} aria-hidden="true" />
        {bootTimeout && (
          <div className="boot-timeout">
            <p>
              Taking longer than expected. The engine may still be starting.
            </p>
            {error && <p className="health-bad">{error}</p>}
            <button
              type="button"
              onClick={() => {
                setBootTimeout(false);
                void boot();
              }}
            >
              Retry connection
            </button>
          </div>
        )}
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

  const picker = (
    <UnifiedPicker
      targets={pickerTargets}
      value={modelChoice}
      open={pickerOpen}
      onOpenChange={setPickerOpen}
      loading={!pickerLoaded}
      note={pendingNote}
      onSelect={(id) => {
        void selectTarget(id);
        promptRef.current?.focus();
      }}
      onConnect={(vendor) => openSettings("accounts", { vendor })}
      onSetup={(target) =>
        isLocal(target)
          ? openSettings("local")
          : openSettings("accounts", { vendor: vendorKey(target) })
      }
      onAddLocal={() => openSettings("local")}
    />
  );
  const controls = (
    <>
      <PermissionControl
        mode={permissionMode}
        readOnly={readOnly}
        vendorNote={vendorNote}
        onChange={(mode) => void setPermissionMode(mode)}
        onOpenSettings={() => openSettings("permissions")}
      />
      {networkMode === "offline" ? (
        <NetworkPill mode="offline" />
      ) : selectedTarget && isLocal(selectedTarget) ? (
        networkMode === "web_off" ? (
          <NetworkPill mode="web_off" />
        ) : (
          <WebToggle
            enabled={webEnabled}
            onChange={(next) => {
              setWebEnabled(next);
              writeStore("shadow:web", next ? "on" : "off");
            }}
          />
        )
      ) : null}
    </>
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
          onSettings={() => openSettings()}
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
            <PanelLeft size={18} aria-hidden="true" />
          </button>
        )}
        <button
          type="button"
          className="project-crumb"
          title={workspace || "Open project"}
          onClick={() => setOverlay("project")}
        >
          <FolderOpen size={15} aria-hidden="true" />
          <span>{workspace.split("/").pop() || "Open project"}</span>
        </button>
        <ChevronRight size={13} className="dim" aria-hidden="true" />
        <span className="top-title" title={title}>
          {title}
        </span>
        <div className="top-right">
          <button
            type="button"
            className={`top-action ${panel === "changes" ? "on" : ""}`}
            aria-label="Review changes"
            title="Changes (Ctrl+Shift+B)"
            onClick={() => setPanel(panel === "changes" ? null : "changes")}
          >
            <GitPullRequest size={15} aria-hidden="true" />
            <span>Changes</span>
            {git.count > 0 && <span className="count">{git.count}</span>}
          </button>
          <span className="top-divider" />
          <button
            type="button"
            className="icon-btn"
            title="Command palette (Ctrl+K)"
            aria-label="Command palette"
            onClick={() => setOverlay("palette")}
          >
            <Search size={16} aria-hidden="true" />
          </button>
        </div>
      </header>
      <main className="stage">
        {health?.desktop_attached && (
          <div className="connection-banner" role="status">
            Connected to your running engine. Closing this window leaves its
            work running.
          </div>
        )}
        {transcript.limit && (
          <div className="connection-banner limit-banner" role="alert">
            <span>
              Plan limit reached on {transcript.limit.vendor} · choose another
              model
            </span>
            <button
              type="button"
              className="mini"
              onClick={() => setPickerOpen(true)}
            >
              Choose model
            </button>
          </div>
        )}
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
            <LoaderCircle size={14} className="spin" aria-hidden="true" />
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
            {shutdown && (
              <div className="notice" role="status">
                <span>
                  {shutdown.message ||
                    "Stopping active work and saving the session before closing…"}
                </span>
                {shutdown.status === "error" && (
                  <button
                    type="button"
                    className="mini"
                    onClick={() => void invoke("desktop_quit")}
                  >
                    Retry closing
                  </button>
                )}
              </div>
            )}
            {error && (
              <div className="notice bad" role="alert">
                <span>
                  {error}
                  {trustErrorHint(error) ? ` ${trustErrorHint(error)}` : ""}
                </span>
                {isProjectTrustError(error) && workspace && (
                  <button
                    type="button"
                    className="mini"
                    onClick={() =>
                      setTrust(trustRequestFor(workspace, status?.permissions))
                    }
                  >
                    Trust this folder
                  </button>
                )}
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
                  <X size={14} aria-hidden="true" />
                </button>
              </div>
            )}
            {!switching &&
              conversation.history.enabled &&
              (conversation.history.hasOlder ||
                conversation.history.viewing ||
                conversation.history.error) && (
                <nav
                  className="history-navigation"
                  aria-label="Conversation history"
                >
                  <div className="row">
                    <button
                      type="button"
                      className="ghost"
                      disabled={
                        conversation.history.loading ||
                        !conversation.history.hasOlder
                      }
                      onClick={() => {
                        stick.current = false;
                        void conversation.history.older();
                      }}
                    >
                      Older messages
                    </button>
                    {conversation.history.viewing && (
                      <>
                        <button
                          type="button"
                          className="ghost"
                          disabled={conversation.history.loading}
                          onClick={() => {
                            stick.current = false;
                            void conversation.history.newer();
                          }}
                        >
                          Newer messages
                        </button>
                        <button
                          type="button"
                          className="ghost"
                          onClick={() => {
                            conversation.history.latest();
                            stick.current = true;
                            setAtBottom(true);
                          }}
                        >
                          Latest messages
                        </button>
                      </>
                    )}
                  </div>
                  {conversation.history.loading && (
                    <p role="status">Loading saved messages…</p>
                  )}
                  {conversation.history.viewing && (
                    <p className="hint">
                      Browsing saved history. Current work continues. Pages may
                      begin partway through a task.
                    </p>
                  )}
                  {conversation.history.error && (
                    <p role="alert" className="error">
                      {conversation.history.error}
                    </p>
                  )}
                </nav>
              )}
            {switching ? (
              <div className="loading-task">
                <LoaderCircle size={20} className="spin" aria-hidden="true" />
                Opening task…
              </div>
            ) : empty && !conversation.history.viewing ? (
              <WelcomeBanner
                onSelect={(prompt) => {
                  setTask(prompt);
                  promptRef.current?.focus();
                }}
              />
            ) : (
              <>
                {transcript.items.map((item, i) => {
                  const node =
                    item.kind === "user" &&
                    transcript.activeTaskId !== item.taskId &&
                    queuedJobs.some(
                      (queued) => queued.task_id === item.taskId,
                    ) ? null : item.kind === "tool" ? (
                      // Tool calls live in the activity timeline; lifecycle
                      // hooks have no tool event and stay as cards.
                      item.tool === "hook" ? (
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
                        />
                      ) : null
                    ) : item.kind === "command" ? (
                      <CommandCardView key={i} card={item.card} />
                    ) : item.kind === "note" ? (
                      <div
                        key={i}
                        className={`msg-note ${item.warning ? "warning" : ""}`}
                      >
                        {item.text}
                      </div>
                    ) : item.kind === "divider" ? (
                      <div key={i} className="msg-divider" role="separator">
                        <span>{item.text}</span>
                      </div>
                    ) : item.kind === "summary" ? (
                      transcript.activity[item.taskId] ? (
                        <div key={i} className="msg-summary">
                          {!(item.taskId in timelineBefore) && (
                            <ActivityTimeline
                              activity={transcript.activity[item.taskId]}
                              withSummary
                            />
                          )}
                          <TaskSummary
                            activity={transcript.activity[item.taskId]}
                            diffStat={diffStat}
                            onReview={reviewChanges}
                            onRewind={
                              transcript.activity[item.taskId].verification
                                ?.status === "vendor_owned"
                                ? undefined
                                : () => void rewind(item.taskId)
                            }
                          />
                        </div>
                      ) : null
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
                        {item.eventId && !item.live && (
                          <button
                            type="button"
                            className="ghost fork-action"
                            disabled={
                              busy || submitting || switching || forking
                            }
                            onClick={() =>
                              void (async () => {
                                setForking(true);
                                try {
                                  const branch = await api.forkSession(
                                    sessionId,
                                    item.eventId!,
                                  );
                                  await openSession(branch.fork.id);
                                  toast(
                                    "New conversation created from this point",
                                    "ok",
                                  );
                                } catch (error) {
                                  toast(String(error), "err");
                                } finally {
                                  setForking(false);
                                }
                              })()
                            }
                          >
                            Fork from here
                          </button>
                        )}
                      </div>
                    );
                  // A task that stopped without a completion record still shows
                  // what it did.
                  const stranded =
                    item.taskId &&
                    lastIndexByTask[item.taskId] === i &&
                    item.taskId !== activeTaskId &&
                    transcript.activity[item.taskId] &&
                    !transcript.activity[item.taskId].finished &&
                    transcript.activity[item.taskId].calls.length > 0;
                  return stranded ? (
                    <div key={i}>
                      {node}
                      <ActivityTimeline
                        activity={transcript.activity[item.taskId!]}
                      />
                    </div>
                  ) : item.taskId && timelineBefore[item.taskId] === i ? (
                    <div key={i} className="msg-with-activity">
                      <ActivityTimeline
                        activity={transcript.activity[item.taskId]}
                        withSummary
                      />
                      {node}
                    </div>
                  ) : (
                    node
                  );
                })}
                {commandCards.map((card, i) => (
                  <CommandCardView key={`c${i}`} card={card} />
                ))}
              </>
            )}
            {approvals.map((a) => (
              <ApprovalCard
                key={a.id}
                approval={a}
                onDecide={(id, decision) =>
                  void decide(id, decision, a.session_id)
                }
              />
            ))}
            {(busy || submitting) && !switching && (
              <div className="working" role="status" aria-live="polite">
                <ActivityTimeline
                  activity={
                    activeTaskId ? transcript.activity[activeTaskId] : undefined
                  }
                  pendingApprovals={approvals.length}
                  elapsed={busy ? formatDuration(elapsed) : undefined}
                />
                {job &&
                  (job.status === "running" || job.status === "paused") && (
                    <TaskSteerBar
                      job={job}
                      onToast={(text, kind) => toast(text, kind || "info")}
                    />
                  )}
              </div>
            )}
          </div>
        </div>
        {(!atBottom || conversation.history.viewing) && (
          <button
            type="button"
            className="jump-latest"
            onClick={() => {
              if (conversation.history.viewing) conversation.history.latest();
              setAtBottom(true);
              stick.current = true;
              streamRef.current?.scrollTo({
                top: streamRef.current.scrollHeight,
                behavior: "smooth",
              });
            }}
          >
            <ArrowDown size={14} aria-hidden="true" />
            Latest activity
          </button>
        )}
        <div className="composer-wrap">
          <QueuedTasks
            jobs={queuedJobs}
            sessions={sessions}
            selected={sessionId}
            cancelling={cancellingQueued}
            disabled={submitting || switching || Boolean(shutdown)}
            onCancel={(queued) => void cancelQueued(queued)}
            onOpen={(id) => void openSession(id)}
          />
          {transcript.plan.length > 0 && (
            <details className="task-plan">
              <summary>
                <ListChecks size={15} aria-hidden="true" />
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
                      <Check size={14} aria-hidden="true" />
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
          <Composer
            task={task}
            onTask={setTask}
            promptRef={promptRef}
            attachments={attachments}
            onRemoveAttachment={removeAttachment}
            onAttach={(files) => void attach(files)}
            canAttachImages={canAttachImages}
            attachDisabled={composerLocked}
            picker={picker}
            controls={controls}
            commands={commands}
            placeholder={
              queueing
                ? "Add a follow-up to the queue…"
                : empty
                  ? "Describe what you want to build…"
                  : "Ask for a follow-up change…"
            }
            hint={
              commandWaiting
                ? "Commands wait until idle"
                : task
                  ? queueing
                    ? "↵ Queue"
                    : "↵ Send"
                  : "/ for commands"
            }
            busy={busy}
            queueing={queueing}
            submitting={submitting}
            locked={locked}
            canSend={canSend}
            sendBlocked={hasContent || !selectedTarget ? sendBlocked : null}
            stopDisabled={job?.status === "cancelling"}
            onSubmit={() => void submit()}
            onStop={() => void stop()}
          />
        </div>
        <footer className="statusline" aria-live="polite">
          <span className={`status-dot ${busy ? "active" : ""}`} />
          <span className="status-label">
            {busy
              ? job?.status === "paused"
                ? "Paused"
                : "Working"
              : connection === "reconnecting"
                ? "Reconnecting"
                : "Ready"}
          </span>
          {git.branch && (
            <>
              <span className="sep" aria-hidden="true">
                ·
              </span>
              <button
                type="button"
                title="Review git changes"
                onClick={() => setPanel("changes")}
              >
                <GitBranch size={12} aria-hidden="true" />
                {git.branch}
              </button>
            </>
          )}
          <span className="grow" />
          {ctx > 0 && (
            <span
              className={`ctx-bar${ctx >= 80 ? " ctx-warn" : ""}`}
              role="img"
              aria-label={`${ctx}% of context used`}
              title={`${ctx}% of context used · ${formatTokens(transcript.usage.total_tokens || 0)} tokens`}
            >
              <span
                className="ctx-fill"
                style={{ width: `${Math.min(ctx, 100)}%` }}
              />
            </span>
          )}
          <span className="version">
            {health?.version ? `v${health.version}` : "Connecting"}
          </span>
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
          busy={busy}
          toast={toast}
          onAskAgent={(prompt) => {
            setTask(prompt);
            setPanel(null);
            promptRef.current?.focus();
          }}
        />
      )}
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`toast ${t.kind}`}
            role={t.kind === "err" ? "alert" : "status"}
            aria-live={t.kind === "err" ? "assertive" : "polite"}
          >
            <span>{t.text}</span>
            <button
              type="button"
              className="icon-btn"
              aria-label="Dismiss notification"
              onClick={() =>
                setToasts((prev) => prev.filter((x) => x.id !== t.id))
              }
            >
              <X size={13} aria-hidden="true" />
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
      {consent && (
        <ConsentDialog
          request={consent.request}
          destination={
            pickerTargets.find((t) => t.id === consent.body.model)?.name
          }
          attachments={consent.original.attachments.map((a) => a.name)}
          onCancel={() => {
            setTask(consent.original.task);
            setAttachments(consent.original.attachments);
            setConsent(null);
            promptRef.current?.focus();
          }}
          onSend={() => {
            const { body, original } = consent;
            setConsent(null);
            void startTask({ ...body, handoff_consent: true }, original);
          }}
        />
      )}
      {overlay === "settings" && (
        <Settings
          cfg={cfg}
          initialSection={settings.section}
          initialAdvanced={settings.advanced}
          focusVendor={settings.vendor}
          health={health}
          sessionId={sessionId}
          busy={busy}
          onClose={() => setOverlay("")}
          onToast={toast}
          onOpenProject={(path) => void pickProject(path)}
          onOpenSession={(id) => void openSession(id)}
          onSkillsChanged={reloadCommands}
          onUseSkill={(name) => {
            setTask(`/skill ${name} `);
            promptRef.current?.focus();
          }}
          onCatalogChanged={() => void reloadPicker()}
          onSave={async (values) => {
            try {
              await api.saveConfig(values);
              await reloadConfig();
              void reloadPicker();
              toast("Settings saved", "ok");
            } catch (e) {
              toast(String(e), "err");
            }
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
    </div>
  );
}
