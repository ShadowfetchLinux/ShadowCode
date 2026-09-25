import { useEffect, useRef, useState, type RefObject } from "react";
import { api, type CompareRecord, type SessionDetail } from "../api";
import { rememberRecent } from "../lib/picker";
import {
  draftKey,
  isSessionCommand,
  readStore,
  targetKey,
  writeStore,
} from "../lib/storage";
import { sameWorkspacePath, trustPromptFor } from "../lib/trust";
import type { useConversation } from "./useConversation";
import type { ToastKind } from "./useToasts";
import type { useWorkspace } from "./useWorkspace";

export type Trust = {
  path: string;
  name?: string;
  permissions?: Record<string, unknown>;
};

/** What navigation needs from the rest of the window. Callbacks may reach
 * hooks created later in the same render; they only run after it. */
export type NavigationContext = {
  ws: ReturnType<typeof useWorkspace>;
  conversation: ReturnType<typeof useConversation>;
  taskRef: RefObject<string>;
  setTask: (task: string) => void;
  clearComposer: () => void;
  submittingRef: RefObject<boolean>;
  submitting: boolean;
  lanes: {
    track: (id: string, detail: SessionDetail) => void;
    showChat: () => void;
    note: (records: CompareRecord[]) => void;
  };
  reloadCatalog: () => void;
  pin: () => void;
  toast: (text: string, kind?: ToastKind) => void;
  showProjects: () => void;
  closeOverlay: () => void;
  focusPrompt: () => void;
};

/** Startup, opening conversations and projects, trust, and the model each
 * conversation uses. Opening is serialized (the engine activates one
 * conversation at a time) and a later choice always wins over an earlier
 * one still loading. */
export function useNavigation(ctx: NavigationContext) {
  const { ws, conversation } = ctx;
  const [ready, setReady] = useState(false);
  const [bootTimeout, setBootTimeout] = useState(false);
  const [needsOnboard, setNeedsOnboard] = useState(false);
  const [sessionId, setSessionId] = useState("");
  const [switching, setSwitching] = useState(false);
  const [error, setError] = useState("");
  const [trust, setTrust] = useState<Trust | null>(null);
  const [modelChoice, setModelChoice] = useState("");
  const [runningChoice, setRunningChoice] = useState("");
  const selectedRef = useRef("");
  const selection = useRef(0);
  const booted = useRef(false);
  const activationQueue = useRef<Promise<unknown>>(Promise.resolve());
  const appliedFallback = useRef("");
  const workspace = ws.workspace;

  async function openSession(id: string) {
    if (ctx.submittingRef.current) return;
    if (selectedRef.current) {
      const draft = ctx.taskRef.current;
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
      ws.setWorkspace(detail.workspace);
      writeStore("shadow:selected", id);
      conversation.load(detail, active.job);
      ctx.lanes.track(id, detail);
      ctx.lanes.showChat();
      // The conversation's own target wins; a new conversation starts on the
      // project's last choice. Nothing falls back to a configured default.
      setModelChoice(
        detail.execution_target || readStore(targetKey(detail.workspace)) || "",
      );
      setRunningChoice(
        active.job?.model || active.job?.routing?.requested || "",
      );
      ctx.setTask(readStore(draftKey(id, detail.workspace)) || "");
      ctx.clearComposer();
      ctx.pin();
      await ws.refresh();
    } catch (e) {
      if (ticket === selection.current) {
        setError(String(e));
        ctx.toast(String(e), "err");
      }
    } finally {
      if (ticket === selection.current) {
        ctx.pin();
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
      ws.setHealth(h);
      ws.setWorkspace(h.workspace);
      setNeedsOnboard(!onboard.completed);
      void api
        .compares(h.workspace)
        .then((r) => ctx.lanes.note(r.compares))
        .catch(() => undefined);
      await ws.reloadConfig();
      ctx.reloadCatalog();
      await ws.refresh();
      const saved = readStore("shadow:selected");
      let initial =
        sessionData.sessions.find((s) => s.id === saved) ||
        sessionData.sessions.find((s) => s.workspace === h.workspace);
      // Lane conversations are left out of the list; reopen one directly.
      if (saved && !sessionData.sessions.some((s) => s.id === saved)) {
        const lane = await api.session(saved).catch(() => null);
        if (lane?.compare_id) initial = lane;
      }
      if (onboard.completed && initial) await openSession(initial.id);
      else setModelChoice(readStore(targetKey(h.workspace)) || "");
      const latest = await api.status();
      ws.setStatus(latest);
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
  useEffect(() => {
    if (!ready) {
      const timer = setTimeout(() => setBootTimeout(true), 10000);
      return () => clearTimeout(timer);
    }
  }, [ready]);

  // A plan ran out and the engine continued on a local model: the composer
  // follows the conversation's saved target (now that local model).
  const fallback = conversation.transcript.fallback;
  const jobId = conversation.job?.id;
  useEffect(() => {
    const next = fallback;
    if (!next?.jobId || jobId !== next.jobId) return;
    if (appliedFallback.current === next.jobId) return;
    appliedFallback.current = next.jobId;
    const sid = sessionId;
    const apply = (target: string) => {
      if (selectedRef.current !== sid || !target) return;
      setModelChoice(target);
      setRunningChoice(target);
      if (workspace) writeStore(targetKey(workspace), target);
    };
    void api
      .session(sid)
      .then((detail) => apply(detail.execution_target || next.target))
      .catch(() => apply(next.target));
  }, [fallback, jobId, sessionId, workspace]);

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
    if (!opts?.force && (ctx.submitting || switching)) return;
    if (!workspace) {
      ctx.showProjects();
      return;
    }
    try {
      const created = await api.createSession(workspace);
      writeStore("shadow:selected", created.id);
      await openSession(created.id);
      ctx.focusPrompt();
    } catch (e) {
      ctx.toast(String(e), "err");
    }
  }

  async function pickProject(path?: string) {
    if (!path) {
      ctx.showProjects();
      return;
    }
    try {
      const opened = await api.openProject(path);
      ctx.closeOverlay();
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
      ctx.toast(String(e), "err");
    }
  }

  async function confirmTrust() {
    if (!trust) return;
    try {
      const opened = await api.trustProject(trust.path);
      const [latest, h] = await Promise.all([api.status(), api.health()]);
      ws.setStatus(latest);
      ws.setHealth(h);
      if (latest.trusted === false) {
        ctx.toast(
          "Trust did not persist. Click Trust and open again, then send the task.",
          "err",
        );
        return;
      }
      setTrust(null);
      await ws.reloadConfig();
      if (
        sessionId &&
        sameWorkspacePath(workspace, opened.path || latest.workspace)
      ) {
        ctx.toast("Project trusted. You can send a task.", "ok");
      } else if (opened.session_id) {
        await openSession(opened.session_id);
      } else {
        ctx.toast("Project trusted. You can send a task.", "ok");
      }
    } catch (e) {
      ctx.toast(String(e), "err");
    }
  }

  return {
    ready,
    bootTimeout,
    setBootTimeout,
    needsOnboard,
    setNeedsOnboard,
    sessionId,
    setSessionId,
    selectedRef,
    selection,
    switching,
    error,
    setError,
    trust,
    setTrust,
    modelChoice,
    runningChoice,
    setRunningChoice,
    boot,
    openSession,
    newSession,
    pickProject,
    confirmTrust,
    selectTarget,
  };
}
