import { useMemo, type RefObject } from "react";
import {
  api,
  type CommandResult,
  type ConsentRequest,
  type Health,
  type Job,
  type LimitsConfig,
  type StartJobRequest,
} from "../api";
import type { ChatItem } from "../components/cards";
import type { DrawerTab } from "../components/Drawer";
import type { AdvancedTab, SettingsSection } from "../components/Settings";
import { continuationTask, type Fallback } from "../lib/allowance";
import { sendBlockedByImages, type Attachment } from "../lib/attachments";
import { isReady, type PickerTarget } from "../lib/picker";
import { draftKey, isSessionCommand, writeStore } from "../lib/storage";
import { invoke } from "../lib/transport";
import {
  isProjectTrustError,
  trustPromptFor,
  trustRequestFor,
} from "../lib/trust";
import type { useConversation } from "./useConversation";
import type { ComposerExtras } from "./useComposerExtras";
import { purposeFor } from "../lib/effort";
import { mentionsInText } from "../lib/mentions";
import type { ToastKind } from "./useToasts";
import type { WorkspaceStatus } from "./useWorkspace";

const ADVANCED_PANELS: Record<string, AdvancedTab> = {
  goals: "goals",
  skills: "skills",
  health: "health",
  doctor: "health",
  background: "background",
};
const DRAWERS: Record<string, DrawerTab> = {
  sessions: "sessions",
  diff: "changes",
  changes: "changes",
  files: "files",
  terminal: "terminal",
};

/** A cloud route asked for consent before the conversation leaves this
 * computer; `original` restores the composer on Cancel. */
export type Consent = {
  request: ConsentRequest;
  body: StartJobRequest;
  original: { task: string; attachments: Attachment[] };
};
type Trust = {
  path: string;
  name?: string;
  permissions?: Record<string, unknown>;
};

/** What sending needs from the window, read on every render. */
export type TaskActionContext = {
  task: string;
  setTask: (task: string) => void;
  attachments: Attachment[];
  setAttachments: (attachments: Attachment[]) => void;
  workspace: string;
  sessionId: string;
  setSessionId: (id: string) => void;
  selectedRef: RefObject<string>;
  /** Increments on every conversation switch; stale answers are dropped. */
  selection: RefObject<number>;
  submittingRef: RefObject<boolean>;
  taskRef: RefObject<string>;
  jobRef: RefObject<Job | null>;
  busy: boolean;
  queueing: boolean;
  composerLocked: boolean;
  commandWaiting: boolean;
  setSubmitting: (value: boolean) => void;
  setError: (error: string) => void;
  setTrust: (trust: Trust | null) => void;
  setConsent: (consent: Consent | null) => void;
  health: Health | null;
  status: WorkspaceStatus | null;
  pickerLoaded: boolean;
  selectedTarget: PickerTarget | undefined;
  modelChoice: string;
  canAttachImages: boolean;
  webAllowed: boolean;
  webEnabled: boolean;
  conversation: ReturnType<typeof useConversation>;
  refresh: () => Promise<void>;
  reloadConfig: () => Promise<void>;
  reloadAllowance: () => Promise<void>;
  toast: (text: string, kind?: ToastKind) => void;
  newSession: (opts?: { force?: boolean }) => Promise<void>;
  openSession: (id: string) => Promise<void>;
  openSettings: (
    section?: SettingsSection,
    extra?: { advanced?: AdvancedTab; vendor?: string },
  ) => void;
  selectTarget: (id: string) => Promise<void>;
  setPanel: (tab: DrawerTab) => void;
  setPickerOpen: (open: boolean) => void;
  setCommandCards: (update: (prev: CommandResult[]) => CommandResult[]) => void;
  setRunningChoice: (id: string) => void;
  /** Follow the conversation to its newest row. */
  pin: () => void;
  /** @-mentions, prompt history, effort and task mode. */
  extras?: ComposerExtras;
};

/** Sending: tasks, follow-ups, slash commands, consent, plan-limit
 * continuations and the plan-limit setting. */
export function useTaskActions(c: TaskActionContext) {
  const { task, attachments, selectedTarget, modelChoice, pickerLoaded } = c;
  const { canAttachImages } = c;
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
    !c.composerLocked &&
    !c.commandWaiting &&
    hasContent &&
    !sendBlocked &&
    (task.trim().startsWith("/") || Boolean(selectedTarget));

  async function runSlash(text: string) {
    const ticket = c.selection.current;
    const originSession = c.selectedRef.current;
    const [name, ...rest] = text.slice(1).split(" ");
    const args = rest.join(" ");
    if (name === "new" || name === "clear") {
      await c.newSession();
      return;
    }
    if (name === "model") {
      // The picker is the only place a model is chosen.
      c.setPickerOpen(true);
      return;
    }
    if (DRAWERS[name] && !args) {
      c.setPanel(DRAWERS[name]);
      return;
    }
    if (ADVANCED_PANELS[name] && !args) {
      c.openSettings("advanced", { advanced: ADVANCED_PANELS[name] });
      return;
    }
    if (name === "settings" && !args) {
      c.openSettings();
      return;
    }
    const result = await api.runCommand(name, args, c.sessionId || undefined, {
      model: c.modelChoice || undefined,
    });
    if (ticket !== c.selection.current) {
      await c.refresh();
      return;
    }
    const metadata = result.metadata || {};
    const started = metadata.job as Job | undefined;
    if (started?.id) {
      if (started.session_id !== c.selectedRef.current) {
        c.selectedRef.current = started.session_id;
        c.setSessionId(started.session_id);
        writeStore("shadow:selected", started.session_id);
      }
      c.conversation.start(started);
    } else if (typeof metadata.session_id === "string") {
      await c.openSession(metadata.session_id);
    } else if (result.kind === "overlay") {
      if (["model", "picker"].includes(result.overlay)) c.setPickerOpen(true);
      else c.openSettings();
    } else if (metadata.action === "quit" || result.quit) {
      await invoke("desktop_quit");
    } else if (!metadata.panel) {
      if (originSession) {
        const [detail, current] = await Promise.all([
          api.session(originSession),
          api.currentJob(originSession),
        ]);
        if (ticket === c.selection.current)
          c.conversation.load(detail, current.job);
      } else c.setCommandCards((prev) => [...prev, result]);
    }
    if (typeof metadata.panel === "string") {
      const panelName = metadata.panel;
      if (DRAWERS[panelName]) c.setPanel(DRAWERS[panelName]);
      else if (["sessions", "files", "terminal", "changes"].includes(panelName))
        c.setPanel(panelName as DrawerTab);
      else if (ADVANCED_PANELS[panelName])
        c.openSettings("advanced", { advanced: ADVANCED_PANELS[panelName] });
    }
    if (metadata.reload_config) await c.reloadConfig();
    await c.refresh();
  }

  /** `original` is the composer content to restore on failure; null when
   * the task did not come from the composer (Continue on …). */
  async function startTask(
    body: StartJobRequest,
    original: { task: string; attachments: Attachment[] } | null,
  ) {
    const submitTicket = c.selection.current;
    c.submittingRef.current = true;
    c.setSubmitting(true);
    c.setError("");
    try {
      const result = await api.startJob(body);
      if ("consent" in result) {
        if (submitTicket === c.selection.current)
          c.setConsent({
            request: result.consent,
            body,
            original: original || { task: c.taskRef.current, attachments: [] },
          });
        return;
      }
      const started = result.job;
      if (submitTicket !== c.selection.current) {
        await c.refresh();
        return;
      }
      if (started.session_id !== c.selectedRef.current) {
        c.selectedRef.current = started.session_id;
        c.setSessionId(started.session_id);
        writeStore("shadow:selected", started.session_id);
      }
      for (const a of original?.attachments || [])
        if (a.preview) URL.revokeObjectURL(a.preview);
      // Keep streaming the current task while a follow-up waits.
      if (!c.busy) {
        c.conversation.start(started);
        c.setRunningChoice(body.model || "");
      }
      if (body.queue)
        c.toast(
          "Follow-up queued. It will run after earlier project tasks.",
          "ok",
        );
      await c.refresh().catch(() => undefined);
    } catch (e) {
      if (submitTicket !== c.selection.current) {
        c.toast(String(e), "err");
        return;
      }
      if (original) {
        c.setTask(original.task);
        c.setAttachments(original.attachments);
        for (const m of body.mentions || []) c.extras?.addMention(m);
      }
      c.setError(String(e));
      c.toast(String(e), "err");
      if (isProjectTrustError(e) && c.workspace)
        c.setTrust(trustRequestFor(c.workspace, c.status?.permissions));
    } finally {
      c.setSubmitting(false);
      c.submittingRef.current = false;
    }
  }

  /** A plan-limit card's "Continue on …": the same conversation continues on
   * the local model, and the composer follows it. */
  async function continueOnFallback(
    item: Extract<ChatItem, { kind: "limit" }>,
    fallback: Fallback,
  ) {
    if (c.composerLocked || c.submittingRef.current) return;
    await c.selectTarget(fallback.id);
    c.pin();
    await startTask(
      {
        task: continuationTask(item.from, item.request || ""),
        workspace: c.workspace || undefined,
        session_id: c.sessionId || undefined,
        model: fallback.id,
        purpose: "coder",
        queue: c.queueing,
        images: [],
        web: false,
      },
      null,
    );
  }

  /** In "ask" mode the engine records the stop after the job ends, when the
   * job's own event stream has closed: read it from the conversation. */
  async function followLimit(done: Job) {
    for (let attempt = 0; attempt < 8; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 300 + attempt * 300));
      if (c.selectedRef.current !== done.session_id) return;
      let detail;
      try {
        detail = await api.session(done.session_id);
      } catch {
        continue;
      }
      const record = detail.events.find(
        (e) => e.type === "limit.fallback" && e.task_id === done.task_id,
      );
      if (!record) continue;
      // An automatic follow-up arrives with the project's jobs.
      if (record.payload.ok) {
        void c.refresh().catch(() => undefined);
        return;
      }
      if (
        c.selectedRef.current === done.session_id &&
        !c.submittingRef.current &&
        c.jobRef.current?.id === done.id
      )
        c.conversation.load(detail, done, true);
      return;
    }
  }

  async function saveLimits(next: LimitsConfig) {
    try {
      await api.saveConfig({ limits: next });
      await c.reloadConfig();
      void c.reloadAllowance();
    } catch (e) {
      c.toast(String(e), "err");
      throw e;
    }
  }

  async function submit() {
    if (c.composerLocked || c.submittingRef.current || !hasContent) return;
    if (c.commandWaiting) {
      c.setError(
        "Wait for this project's active work to finish before running a slash command. You can queue a message now.",
      );
      return;
    }
    if (isSessionCommand(task)) {
      c.setTask("");
      await c.newSession({ force: true });
      return;
    }
    const prompt = trustPromptFor(
      c.workspace || c.health?.workspace,
      c.status?.trusted ?? c.health?.trusted,
      c.status?.permissions || c.health?.permissions,
    );
    if (prompt) {
      c.setTrust(prompt);
      c.setError("");
      return;
    }
    const original = { task, attachments };
    c.extras?.history.push(task);
    if (task.trim().startsWith("/")) {
      c.setTask("");
      c.pin();
      c.submittingRef.current = true;
      c.setSubmitting(true);
      try {
        await runSlash(task.trim());
      } catch (e) {
        c.setTask(original.task);
        c.toast(String(e), "err");
      } finally {
        c.setSubmitting(false);
        c.submittingRef.current = false;
      }
      return;
    }
    // Re-check at send time: the row may have changed after attaching.
    if (!selectedTarget || !isReady(selectedTarget) || sendBlocked) {
      if (sendBlocked) c.toast(sendBlocked, "err");
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
    const extras = c.extras;
    const mentions = extras ? mentionsInText(task, extras.mentions) : [];
    c.setTask("");
    c.setAttachments([]);
    extras?.setMentions([]);
    writeStore(draftKey(c.sessionId, c.workspace), null);
    c.pin();
    await startTask(
      {
        task: text || "Describe the attached image(s).",
        workspace: c.workspace || undefined,
        session_id: c.sessionId || undefined,
        model: selectedTarget.id,
        purpose: extras ? purposeFor(extras.mode) : "coder",
        queue: c.queueing,
        images,
        web: c.webAllowed && c.webEnabled,
        ...(extras?.effortShown && extras.effort !== "default"
          ? { effort: extras.effort }
          : {}),
        ...(mentions.length ? { mentions } : {}),
      },
      original,
    );
  }

  return {
    sendBlocked,
    canSend,
    startTask,
    continueOnFallback,
    followLimit,
    saveLimits,
    submit,
  };
}
