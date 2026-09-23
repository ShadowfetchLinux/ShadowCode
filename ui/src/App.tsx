import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronRight,
  Code2,
  FileCode2,
  FolderOpen,
  GitBranch,
  GitPullRequest,
  ListChecks,
  ListPlus,
  LoaderCircle,
  PanelLeft,
  Paperclip,
  Search,
  ShieldCheck,
  Sparkles,
  Square,
  TerminalSquare,
  X,
  Cpu,
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
import {
  ApprovalCard,
  CommandCardView,
  OpCard,
  ThinkingCard,
} from "./components/cards";
import { Drawer, type DrawerTab } from "./components/Drawer";
import { Sidebar } from "./components/Sidebar";
import { Markdown } from "./components/Markdown";
import { Onboarding } from "./components/Onboarding";
import { FlowGuide } from "./components/FlowGuide";
import { OpenWeightHub } from "./components/OpenWeightHub";
import { WelcomeBanner } from "./components/WelcomeBanner";
import { ModeTabs } from "./components/ModeTabs";
import {
  CustomModelDialog,
  Help,
  Palette,
  ProjectPicker,
  TrustDialog,
  type PaletteItem,
} from "./components/overlays";
import { Settings } from "./components/Settings";
import { QueuedTasks } from "./components/QueuedTasks";
import { TaskSteerBar } from "./components/TaskSteerBar";
import { ModelChooser } from "./components/ModelChooser";
import { VendorAgentChip } from "./components/VendorAgentChip";
import { useConversation } from "./hooks/useConversation";
import { modelLabel } from "./lib/models";
import {
  isVendorProvider,
  LOCAL_GROUP,
  VENDOR_GROUP,
  type VendorStatusMap,
} from "./lib/cliAgents";
import { conversationJob } from "./lib/jobs";
import {
  isProjectTrustError,
  sameWorkspacePath,
  trustErrorHint,
  trustPromptFor,
  trustRequestFor,
} from "./lib/trust";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  exportSession as saveExport,
  isNative,
  openExternal,
} from "./lib/transport";

type Overlay =
  | ""
  | "settings"
  | "help"
  | "palette"
  | "project"
  | "custom-model"
  | "flow-guide"
  | "open-weights";
type Toast = { id: number; text: string; kind: "ok" | "err" | "info" };
const formatTokens = (n: number) =>
  n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
const draftKey = (id: string, workspace: string) =>
  `shadow:draft:${id || workspace}`;
const isSessionCommand = (text: string) =>
  ["/new", "/clear"].includes(text.trim());

export default function App() {
  const [ready, setReady] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [cliAgents, setCliAgents] = useState<VendorStatusMap | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
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
  const [shutdown, setShutdown] = useState<{
    status: string;
    message?: string;
  } | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [cancellingQueued, setCancellingQueued] = useState<string[]>([]);
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
  const taskRef = useRef("");
  const toastSeq = useRef(0);
  const booted = useRef(false);
  const activationQueue = useRef<Promise<unknown>>(Promise.resolve());
  const stick = useRef(true);
  const browsingHistory = useRef(false);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = ++toastSeq.current;
    setToasts((prev) => [...prev.slice(-3), { id, text, kind }]);
    const duration = kind === "err" ? 8000 : 5000;
    setTimeout(
      () => setToasts((prev) => prev.filter((t) => t.id !== id)),
      duration,
    );
  }, []);

  useEffect(() => {
    if (!isNative()) return;
    let stopped = false;
    let unsubscribe: (() => void) | undefined;
    void listen<{ status: string; message?: string }>(
      "shadowcode:shutdown",
      (event) => setShutdown(event.payload),
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
  const locked = busy || submitting || switching || Boolean(shutdown);
  const projectBusy =
    busy ||
    jobs.some(
      (item) =>
        item.workspace === workspace &&
        ["queued", "running", "cancelling"].includes(item.status),
    );
  const queueing = isNative() && projectBusy;
  const composerLocked =
    submitting || switching || Boolean(shutdown) || (busy && !isNative());
  const queuedJobs = isNative()
    ? jobs
        .filter(
          (item) => item.workspace === workspace && item.status === "queued",
        )
        .slice()
        .reverse()
    : [];
  const commandWaiting = queueing && task.trim().startsWith("/");
  taskRef.current = task;

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
    setCliAgents(modelData.cli_agents || null);
    setProviders(providerData.providers);
  }

  async function openSession(id: string) {
    if (submittingRef.current) return;
    if (selectedRef.current) {
      const draft = taskRef.current;
      if (draft && !isSessionCommand(draft))
        localStorage.setItem(draftKey(selectedRef.current, workspace), draft);
      else if (!draft)
        localStorage.removeItem(draftKey(selectedRef.current, workspace));
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
      await refresh();
      const saved = localStorage.getItem("shadow:selected");
      const initial =
        sessionData.sessions.find((s) => s.id === saved) ||
        sessionData.sessions.find((s) => s.workspace === h.workspace);
      if (onboard.completed && initial) await openSession(initial.id);
      const latest = await api.status();
      setStatus(latest);
      const prompt = trustPromptFor(
        latest.workspace || h.workspace,
        latest.trusted ?? h.trusted,
        latest.permissions || h.permissions,
      );
      if (prompt) setTrust(prompt);
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
    const compact = window.matchMedia("(max-width: 760px)");
    const resize = (event: MediaQueryListEvent) => {
      if (event.matches) setSidebar(false);
    };
    compact.addEventListener("change", resize);
    return () => compact.removeEventListener("change", resize);
  }, []);
  useEffect(() => {
    document.title = `${busy ? "● " : ""}ShadowCode`;
  }, [busy]);
  useEffect(() => {
    const key = draftKey(sessionId, workspace);
    const timer = setTimeout(() => {
      if (isSessionCommand(task)) return;
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
  useLayoutEffect(() => {
    if (ready && !switching && stick.current)
      streamRef.current?.scrollTo({ top: streamRef.current.scrollHeight });
  }, [
    transcript.items,
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
    const timer = setInterval(poll, busy ? 1200 : isNative() ? 2000 : 5000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [sessionId, busy]);
  // Goals and queued follow-ups can start after the visible job has finished.
  // Reload before attaching their stream so intervening milestones are retained.
  useEffect(() => {
    if (!sessionId || busy || submitting || switching) return;
    const sessionJobs = jobs.filter((item) => item.session_id === sessionId);
    const next = conversationJob(sessionJobs, job);
    if (!next || next.id === job?.id) return;
    let live = true;
    void Promise.all([api.session(sessionId), api.job(next.id)])
      .then(([detail, fullJob]) => {
        if (live && selectedRef.current === sessionId && !submittingRef.current)
          conversation.load(detail, fullJob, true);
      })
      .catch(() => {
        /* The next poll retries a failed snapshot. */
      });
    return () => {
      live = false;
    };
  }, [
    jobs,
    sessionId,
    busy,
    submitting,
    switching,
    job?.id,
    conversation.load,
  ]);
  useEffect(() => {
    if (!job || !busy) return;
    const tick = () =>
      setElapsed(Math.max(0, Math.floor(Date.now() / 1000 - job.started_at)));
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [job?.id, busy]);

  async function newSession(opts?: { force?: boolean }) {
    if (!opts?.force && (submitting || switching)) return;
    if (!workspace) {
      setOverlay("project");
      return;
    }
    try {
      const created = await api.createSession(workspace, "New task");
      localStorage.setItem("shadow:selected", created.id);
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
    const panels: Record<string, DrawerTab> = {
      sessions: "sessions",
      diff: "changes",
      goals: "goals",
      skills: "skills",
      health: "health",
      ...(!isNative() ? { doctor: "health" as DrawerTab } : {}),
      background: "background",
    };
    if (panels[name] && !args) {
      setPanel(panels[name]);
      return;
    }
    if (name === "settings" && !args) {
      setOverlay("settings");
      return;
    }
    const result = await api.runCommand(name, args, sessionId || undefined, {
      model: modelChoice || undefined,
      purpose: mode,
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
        localStorage.setItem("shadow:selected", started.session_id);
      }
      conversation.start(started);
    } else if (typeof metadata.session_id === "string") {
      await openSession(metadata.session_id);
    } else if (result.kind === "overlay") {
      setOverlay((result.overlay as Overlay) || "settings");
    } else if (metadata.action === "expand") {
      setTranscript((state) => {
        const index = state.items.map((item) => item.kind).lastIndexOf("tool");
        return {
          ...state,
          items: state.items.map((item, i) =>
            i === index && item.kind === "tool"
              ? { ...item, collapsed: item.collapsed === false }
              : item,
          ),
        };
      });
    } else if (metadata.action === "quit" || result.quit) {
      if (isNative()) await invoke("desktop_quit");
      else toast("Close this browser tab to leave ShadowCode.", "info");
    } else if (!metadata.panel) {
      if (isNative() && originSession) {
        const [detail, current] = await Promise.all([
          api.session(originSession),
          api.currentJob(originSession),
        ]);
        if (ticket === selection.current)
          conversation.load(detail, current.job);
      } else setCommandCards((prev) => [...prev, result]);
    }
    if (
      typeof metadata.panel === "string" &&
      [...Object.values(panels), "changes"].includes(
        metadata.panel as DrawerTab,
      )
    )
      setPanel(metadata.panel as DrawerTab);
    if (metadata.reload_config) await reloadConfig();
    await refresh();
  }
  async function submit() {
    if (
      composerLocked ||
      submittingRef.current ||
      (!task.trim() && !chips.length)
    )
      return;
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
    const submitTicket = selection.current;
    const original = task;
    const attached = [...chips];
    const imagePaths = attached.filter((p) => /\.(png|jpe?g|webp)$/i.test(p));
    const textPaths = attached.filter((p) => !imagePaths.includes(p));
    const text = (
      task.trim() +
      (textPaths.length ? `\n\nAttached paths: ${textPaths.join(", ")}` : "") +
      (imagePaths.length ? `\n\nAttached images: ${imagePaths.join(", ")}` : "")
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
        text || (imagePaths.length ? "Describe the attached image(s)." : ""),
        workspace || undefined,
        sessionId || undefined,
        modelChoice || undefined,
        mode,
        queueing,
        imagePaths,
      );
      if (submitTicket !== selection.current) {
        await refresh();
        return;
      }
      if (started.session_id !== selectedRef.current) {
        selectedRef.current = started.session_id;
        setSessionId(started.session_id);
        localStorage.setItem("shadow:selected", started.session_id);
      }
      // Keep streaming the current task while the follow-up waits. A task
      // submitted from an idle conversation can itself be waiting on a project.
      if (!busy) conversation.start(started);
      if (queueing)
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
      setTask(original);
      setChips(attached);
      setError(String(e));
      toast(String(e), "err");
      if (isProjectTrustError(e) && workspace) {
        setTrust(trustRequestFor(workspace, status?.permissions));
      }
    } finally {
      setSubmitting(false);
      submittingRef.current = false;
    }
  }
  async function attach(files: FileList | File[]) {
    if (projectBusy || composerLocked) {
      toast("Attach files after the project's active work finishes.", "info");
      return;
    }
    const imageExt = /\.(png|jpe?g|webp)$/i;
    for (const file of Array.from(files)) {
      const isImage =
        file.type.startsWith("image/") || imageExt.test(file.name);
      if (file.size > 1_000_000 && !isImage) {
        toast(
          `${file.name}: text attachments must be smaller than 1 MB`,
          "err",
        );
        continue;
      }
      if (isImage) {
        if (file.size > 4_000_000) {
          toast(`${file.name}: images must be smaller than 4 MB`, "err");
          continue;
        }
        if (
          !["image/png", "image/jpeg", "image/webp", ""].includes(file.type) &&
          !imageExt.test(file.name)
        ) {
          toast(`${file.name}: use PNG, JPEG, or WebP`, "err");
          continue;
        }
        try {
          const buffer = await file.arrayBuffer();
          const bytes = new Uint8Array(buffer);
          let binary = "";
          const chunk = 0x8000;
          for (let i = 0; i < bytes.length; i += chunk) {
            binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
          }
          const data_base64 = btoa(binary);
          const saved = await api.attachImage(file.name, data_base64);
          setChips((prev) => [...new Set([...prev, saved.path])]);
        } catch (e) {
          toast(String(e), "err");
        }
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
          toast(`${file.name}: attach a text, source, or image file`, "err");
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
  function exportSession(format: "md" | "json" = "md") {
    if (sessionId)
      void saveExport(sessionId, format).catch((e) => toast(String(e), "err"));
  }
  const palette: PaletteItem[] = [
    {
      id: "flow-guide",
      label: "20-Minute Fast-Track Guide",
      hint: "Master the workflow",
      run: () => setOverlay("flow-guide"),
    },
    {
      id: "open-weights",
      label: "Open-Weight Models Showcase",
      hint: "Qwen, DeepSeek, Llama",
      run: () => setOverlay("open-weights"),
    },
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
      if (mod && key === "b" && !e.shiftKey) {
        e.preventDefault();
        setSidebar((v) => !v);
      }
      if (mod && e.shiftKey && key === "b") {
        e.preventDefault();
        setPanel((p) => (p ? null : "changes"));
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
  }, [
    overlay,
    trust,
    slashOpen,
    panel,
    workspace,
    sessionId,
    busy,
    modelChoice,
    mode,
    submitting,
    switching,
  ]);
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

  const current = sessions.find((s) => s.id === sessionId);
  const title = current?.title || "New task";
  const selectedModel = models.find(
    (candidate) => candidate.id === modelChoice,
  );
  const activeModel = job?.routing || transcript.routing;
  const model =
    busy && activeModel
      ? activeModel.model_name
      : selectedModel
        ? modelLabel(selectedModel, models)
        : status?.routing?.enabled
          ? "Automatic by task mode"
          : status?.model.name || status?.model.default || "Choose model";
  const contextLimit =
    activeModel?.context_limit || status?.model.context_limit;
  const ctx = contextLimit
    ? Math.min(
        100,
        Math.round(
          ((transcript.usage.prompt_tokens || 0) / contextLimit) * 100,
        ),
      )
    : 0;
  const slashHits = slashOpen
    ? commands.filter((c) => c.name.startsWith(task.slice(1))).slice(0, 8)
    : [];
  const empty =
    !transcript.items.length &&
    !commandCards.length &&
    !busy &&
    !submitting &&
    !switching;
  const completedSteps = transcript.plan.filter(
    (p) => p.status === "done",
  ).length;
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
        <LoaderCircle className="spin" size={18} />
        {bootTimeout && (
          <div className="boot-timeout">
            <p>
              Taking longer than expected. The backend may be starting up or
              unreachable.
            </p>
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
            className="icon-btn"
            aria-label="Open-Weight Models & Vendor CLIs"
            title="Models & AI providers"
            onClick={() => setOverlay("open-weights")}
          >
            <Cpu size={17} />
          </button>
          <button
            type="button"
            className="icon-btn"
            aria-label="20-Minute Fast-Track Guide"
            title="Fast-track guide (new to ShadowCode?)"
            onClick={() => setOverlay("flow-guide")}
          >
            <Sparkles size={16} />
          </button>
          <button
            type="button"
            className={`top-action ${panel === "changes" ? "on" : ""}`}
            aria-label="Review changes"
            title="Git changes"
            onClick={() => setPanel(panel === "changes" ? null : "changes")}
          >
            <GitPullRequest size={15} />
            <span>Changes</span>
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
        {health?.desktop_attached && (
          <div className="connection-banner" role="status">
            Connected to your running engine. Closing this window leaves its
            work running.
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
                  <X size={14} />
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
                <LoaderCircle size={20} className="spin" />
                Opening task…
              </div>
            ) : empty && !conversation.history.viewing ? (
              <WelcomeBanner
                workspace={workspace}
                model={model || status?.model.name || ""}
                onSelect={(prompt, selectedMode) => {
                  setTask(prompt);
                  setMode(selectedMode);
                  promptRef.current?.focus();
                }}
              />
            ) : (
              <>
                {transcript.items.map((item, i) =>
                  item.kind === "user" &&
                  transcript.activeTaskId !== item.taskId &&
                  queuedJobs.some(
                    (queued) => queued.task_id === item.taskId,
                  ) ? null : item.kind === "tool" ? (
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
                  ) : item.kind === "command" ? (
                    <CommandCardView key={i} card={item.card} />
                  ) : item.kind === "note" ? (
                    <div
                      key={i}
                      className={`msg-note ${item.warning ? "warning" : ""}`}
                    >
                      {item.text}
                    </div>
                  ) : item.kind === "thinking" ? (
                    <ThinkingCard
                      key={i}
                      text={item.text}
                      durationSec={item.durationSec}
                      live={item.live}
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
                      {isNative() && item.eventId && !item.live && (
                        <button
                          type="button"
                          className="ghost fork-action"
                          disabled={busy || submitting || switching || forking}
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
                onDecide={(id, decision) =>
                  void decide(id, decision, a.session_id)
                }
              />
            ))}
            {(busy || submitting) && !switching && (
              <div className="working" role="status">
                <LoaderCircle size={15} className="spin" />
                <span>
                  {submitting
                    ? queueing
                      ? "Queuing follow-up"
                      : "Starting task"
                    : job?.status === "cancelling"
                      ? "Stopping safely"
                      : job?.status === "paused"
                        ? "Paused — steer or resume"
                        : job?.status === "queued"
                          ? "Waiting for earlier work to finish"
                          : transcript.stage === "UNDERSTAND"
                            ? "Exploring your request"
                            : transcript.stage
                                .toLowerCase()
                                .replaceAll("_", " ")}
                </span>
                <span className="dim">
                  {elapsed >= 60
                    ? `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`
                    : `${elapsed}s`}
                </span>
                {job &&
                  isNative() &&
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
                queueing
                  ? "Add a follow-up to the queue…"
                  : busy
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
                aria-label="Attach files or images"
                title="Attach text files or images (PNG, JPEG, WebP)"
                disabled={projectBusy || composerLocked}
                onClick={() => fileRef.current?.click()}
              >
                <Paperclip size={17} />
              </button>
              <input
                ref={fileRef}
                type="file"
                multiple
                accept=".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp,text/*,.md,.json,.ts,.tsx,.js,.jsx,.py,.rs,.toml,.yaml,.yml,.css,.html,.svg"
                hidden
                onChange={(e) => {
                  if (e.target.files) void attach(e.target.files);
                  e.target.value = "";
                }}
              />
              <ModelChooser
                models={models}
                value={modelChoice}
                automaticLabel={
                  status?.routing?.enabled
                    ? "Automatic by task mode"
                    : status?.model.name ||
                      status?.model.default ||
                      "Choose model"
                }
                onChange={(id) => {
                  if (id === "__custom__") setOverlay("custom-model");
                  else setModelChoice(id);
                }}
              />
              <VendorAgentChip
                status={cliAgents}
                selected={
                  selectedModel?.provider ||
                  String(status?.model?.provider || "")
                }
              />
              <span className="control-divider" />
              <ModeTabs
                value={mode}
                onChange={setMode}
                disabled={busy && !isNative()}
              />
              <span className="grow" />
              <span className="composer-hint">
                {commandWaiting
                  ? "Commands wait until idle"
                  : task
                    ? queueing
                      ? "↵ Queue"
                      : "↵ Send"
                    : queueing
                      ? "Queue a follow-up"
                      : "/ for commands"}
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
              ) : null}
              {(!busy || isNative()) && (
                <button
                  type="submit"
                  className="submit-btn"
                  aria-label={queueing ? "Queue follow-up" : "Send task"}
                  title={queueing ? "Queue follow-up" : "Send task"}
                  disabled={
                    composerLocked ||
                    commandWaiting ||
                    (!task.trim() && !chips.length)
                  }
                >
                  {submitting ? (
                    <LoaderCircle size={17} className="spin" />
                  ) : queueing ? (
                    <ListPlus size={19} />
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
                ? "Read-only"
                : "Workspace tools"}
            </span>
            <button type="button" onClick={() => setOverlay("help")}>
              Shortcuts
            </button>
          </div>
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
              <span className="sep">·</span>
              <button
                type="button"
                title="Review git changes"
                onClick={() => setPanel("changes")}
              >
                <GitBranch size={12} />
                {git.branch}
              </button>
            </>
          )}
          <span className="grow" />
          {ctx > 0 && (
            <span
              className={`ctx-bar${ctx >= 80 ? " ctx-warn" : ""}`}
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
          onSkillsChanged={reloadCommands}
          onUseSkill={(name) => {
            setTask(`/skill ${name} `);
            setPanel(null);
            promptRef.current?.focus();
          }}
          diffPath={diffPath}
          onDiffPath={setDiffPath}
          health={health}
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
          onOpenProject={(path) => void pickProject(path)}
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
      {overlay === "flow-guide" && (
        <FlowGuide
          onClose={() => setOverlay("")}
          onSelectPrompt={(prompt, chosenMode) => {
            setTask(prompt);
            if (chosenMode) setMode(chosenMode);
            promptRef.current?.focus();
          }}
        />
      )}
      {overlay === "open-weights" && (
        <OpenWeightHub
          models={models}
          providers={providers}
          activeModelId={modelChoice || status?.model.name || ""}
          onSelectModel={(id) => {
            setModelChoice(id);
          }}
          onClose={() => setOverlay("")}
          onToast={toast}
        />
      )}
    </div>
  );
}
