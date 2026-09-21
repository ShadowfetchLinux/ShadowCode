import type {
  CommandResult,
  EventRow,
  PlanStep,
  RoutingDecision,
} from "../api";
import type { ChatItem } from "../components/cards";

export type Transcript = {
  items: ChatItem[];
  cursor: number;
  stage: string;
  usage: Record<string, number>;
  plan: PlanStep[];
  routing?: RoutingDecision;
  activeTaskId?: string;
};
export const emptyTranscript = (): Transcript => ({
  items: [],
  cursor: 0,
  stage: "IDLE",
  usage: {},
  plan: [],
});

/** The event ID is the replay boundary. Native streaming messages and parallel
 * tool calls carry their own IDs; a final message replaces its streamed text. */
export function applyEvent(state: Transcript, event: EventRow): Transcript {
  if (event.id && event.id <= state.cursor) return state;
  const p = event.payload || {};
  let items = state.items;
  let stage = state.stage;
  let usage = state.usage;
  let plan = state.plan;
  let routing = state.routing;
  let activeTaskId = state.activeTaskId;
  const taskId = event.task_id || "";
  const text = String(p.text || p.summary || "");
  if (event.type === "history.omitted") {
    items = [...items, { kind: "note", taskId, text, warning: true }];
  }
  if (event.type === "hook.started" || event.type === "hook.completed") {
    const callId = `hook-${String(p.id)}`;
    const index = items.findIndex(
      (item) =>
        item.kind === "tool" &&
        item.taskId === taskId &&
        item.callId === callId,
    );
    const previous = index < 0 ? undefined : items[index];
    const process = p.process as {
      stdout?: string;
      stderr?: string;
      truncated?: boolean;
    } | null;
    const card: ChatItem = {
      kind: "tool",
      tool: "hook",
      taskId,
      callId,
      headline: `Hook · ${String(p.name)}`,
      path: String(p.path || ""),
      text:
        event.type === "hook.started"
          ? `Running ${String(p.event)}`
          : `${String(p.status)} · ${String(p.detail || "")}`,
      fullOutput: [
        String(p.event),
        String(p.command || ""),
        String(p.detail || ""),
        process?.stdout || "",
        process?.stderr || "",
        process?.truncated ? "[Output truncated]" : "",
      ]
        .filter(Boolean)
        .join("\n"),
      live: event.type === "hook.started",
      ok: event.type === "hook.completed" ? Boolean(p.success) : undefined,
      collapsed: previous?.kind === "tool" ? previous.collapsed : true,
    };
    items = [...items];
    if (index < 0) items.push(card);
    else items[index] = card;
  }
  if (
    event.type === "command.completed" &&
    p.result &&
    typeof p.result === "object"
  ) {
    items = [
      ...items,
      {
        kind: "command",
        card: p.result as CommandResult,
        text: (p.result as CommandResult).body || "",
        taskId,
      },
    ];
  }
  if (
    event.type === "context.budget" ||
    event.type === "autonomy.budget" ||
    event.type === "runaway.warning" ||
    event.type === "context.compacted"
  ) {
    const detail =
      event.type === "runaway.warning"
        ? `Loop ${String(p.action || "warn")}: ${String(p.tool || "tool")} repeated ${String(p.repeats || "")}`
        : event.type === "autonomy.budget"
          ? `Autonomy budget ${Math.round(Number(p.ratio || 0) * 100)}% of ${String(p.max_steps || "")} steps`
          : event.type === "context.compacted"
            ? `Context compacted; ${String(p.omitted_messages || 0)} earlier messages omitted`
            : `Context ${String(p.used_estimated_tokens || 0)}/${String(p.limit || 0)} estimated tokens`;
    items = [
      ...items,
      { kind: "note", taskId, text: detail },
    ];
  }
  if (event.type === "workflow.selected") {
    items = [
      ...items,
      {
        kind: "note",
        taskId,
        text: `Workflow /${String(p.name || "workflow")} · ${String(p.path || "project")} · ${String(p.effective_mode || p.mode || "current task mode")}`,
      },
    ];
  }
  if (event.type === "user.message") {
    items = [...items, { kind: "user", text, taskId }];
    if (!activeTaskId) stage = "QUEUED";
  }
  if (event.type === "agent.started") {
    const pending = taskId
      ? items.find((item) => item.kind === "user" && item.taskId === taskId)
      : undefined;
    items = [
      ...items.filter((item) => item !== pending),
      pending || { kind: "user", text: String(p.task || ""), taskId },
    ];
    activeTaskId = taskId;
    stage = "UNDERSTAND";
    plan = [];
    usage = {};
    routing = undefined;
  }
  if (event.type === "routing.selected" || event.type === "routing.fallback") {
    if (p.model_id && p.model_name && p.provider)
      routing = p as unknown as RoutingDecision;
    const selected = [
      p.model_name || p.model_id || p.fallback || "configured model",
      p.provider,
      p.purpose,
    ]
      .filter(Boolean)
      .map(String)
      .join(" · ");
    const warning = event.type === "routing.fallback";
    items = [
      ...items,
      {
        kind: "note",
        taskId,
        warning,
        text: warning
          ? `Using default: ${selected}. ${String(p.fallback_reason || "The saved model is unavailable.")}`
          : `Using ${selected}${p.source === "explicit" ? " · selected for this task" : ""}`,
      },
    ];
  }
  if (
    ["model.stream", "model.delta", "model.stream_end"].includes(event.type)
  ) {
    const messageId = String(p.message_id || "");
    const index = messageId
      ? items.findIndex(
          (item) =>
            item.kind === "agent" &&
            item.messageId === messageId &&
            item.taskId === taskId,
        )
      : -1;
    const previous = index < 0 ? undefined : items[index];
    if (event.type === "model.stream_end") {
      if (previous?.kind === "agent") {
        items = [...items];
        items[index] = {
          ...previous,
          live: false,
          who: "Interrupted response",
        };
      }
    } else if (text) {
      const next: ChatItem = {
        kind: "agent",
        taskId,
        messageId: messageId || undefined,
        text:
          event.type === "model.stream" && previous?.kind === "agent"
            ? previous.text + text
            : text,
        live: event.type === "model.stream",
      };
      items = [...items];
      if (index < 0) items.push(next);
      else items[index] = next;
    }
  }
  if (event.type === "tool.started") {
    items = [
      ...items,
      {
        kind: "tool",
        tool: String(p.tool),
        text: "",
        live: true,
        collapsed: true,
        taskId,
        callId: String(p.call_id || ""),
        headline: String(p.tool),
        path: String(
          (p.arguments as Record<string, unknown> | undefined)?.path || "",
        ),
      },
    ];
  }
  if (event.type === "tool.completed") {
    const index = items.findIndex(
      (i) =>
        i.kind === "tool" &&
        i.live &&
        i.taskId === taskId &&
        (p.call_id ? i.callId === p.call_id : i.tool === p.tool),
    );
    const args = p.arguments as Record<string, unknown> | undefined;
    const previous = index >= 0 ? items[index] : undefined;
    const card: ChatItem = {
      kind: "tool",
      tool: String(p.tool),
      text: String(p.output_preview || p.error || ""),
      fullOutput: String(
        p.output_full ||
          (p.output ? JSON.stringify(p.output, null, 2) : "") ||
          p.output_preview ||
          p.error ||
          "",
      ),
      live: false,
      ok: Boolean(p.success),
      collapsed: previous?.kind === "tool" ? previous.collapsed : true,
      headline: String(p.headline || p.tool),
      icon: String(p.icon || ""),
      taskId,
      callId: String(p.call_id || ""),
      path: String(
        args?.path ||
          args?.dest ||
          (previous?.kind === "tool" ? previous.path : "") ||
          "",
      ),
    };
    items = [...items];
    if (index < 0) items.push(card);
    else items[index] = card;
  }
  if (p.stage && (!activeTaskId || activeTaskId === taskId))
    stage = String(p.stage);
  if (p.plan && (!activeTaskId || activeTaskId === taskId))
    plan = (p.plan as { steps?: PlanStep[] }).steps || plan;
  if (event.type === "agent.completed") {
    // Completion checks can appear after the final model response. Match the
    // latest answer within this task, rather than whichever card is last.
    let last: ChatItem | undefined;
    for (let index = items.length - 1; index >= 0; index--) {
      if (items[index].kind === "agent" && items[index].taskId === taskId) {
        last = items[index];
        break;
      }
    }
    if (
      text &&
      !(
        last?.kind === "agent" &&
        !last.live &&
        last.text.trim() === text.trim() &&
        p.success &&
        !p.cancelled
      )
    )
      items = [
        ...items,
        {
          kind: "agent",
          taskId,
          text,
          who: p.cancelled
            ? "Stopped"
            : p.success
              ? "Result"
              : "Needs attention",
        },
      ];
    if (!activeTaskId || activeTaskId === taskId) {
      stage = p.cancelled ? "CANCELLED" : p.success ? "DONE" : "FAILED";
      usage = (p.usage as Record<string, number>) || {};
      activeTaskId = undefined;
    }
  }
  return {
    items,
    stage,
    usage,
    plan,
    routing,
    activeTaskId,
    cursor: event.id || state.cursor,
  };
}

export function replay(events: EventRow[]): Transcript {
  return events.reduce(applyEvent, emptyTranscript());
}
