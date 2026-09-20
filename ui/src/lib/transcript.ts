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
  const p = event.payload;
  let items = state.items;
  let stage = state.stage;
  let usage = state.usage;
  let plan = state.plan;
  let routing = state.routing;
  const taskId = event.task_id || "";
  const text = String(p.text || p.summary || "");
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
    stage = "QUEUED";
  }
  if (event.type === "agent.started") {
    if (
      !taskId ||
      !items.some((item) => item.kind === "user" && item.taskId === taskId)
    )
      items = [...items, { kind: "user", text: String(p.task || ""), taskId }];
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
  if (p.stage) stage = String(p.stage);
  if (p.plan) plan = (p.plan as { steps?: PlanStep[] }).steps || plan;
  if (event.type === "agent.completed") {
    const last = items.at(-1);
    if (text && !(last?.kind === "agent" && last.text.trim() === text.trim()))
      items = [
        ...items,
        {
          kind: "agent",
          text,
          who: p.cancelled
            ? "Stopped"
            : p.success
              ? "Result"
              : "Needs attention",
        },
      ];
    stage = p.cancelled ? "CANCELLED" : p.success ? "DONE" : "FAILED";
    usage = (p.usage as Record<string, number>) || {};
  }
  return {
    items,
    stage,
    usage,
    plan,
    routing,
    cursor: event.id || state.cursor,
  };
}

export function replay(events: EventRow[]): Transcript {
  return events.reduce(applyEvent, emptyTranscript());
}
