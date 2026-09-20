import type { EventRow, PlanStep } from "../api";
import type { ChatItem } from "../components/cards";

export type Transcript = {
  items: ChatItem[];
  cursor: number;
  stage: string;
  usage: Record<string, number>;
  plan: PlanStep[];
};
export const emptyTranscript = (): Transcript => ({
  items: [],
  cursor: 0,
  stage: "IDLE",
  usage: {},
  plan: [],
});

/** The event ID, not the array length, is the replay boundary. Model events are
 * complete responses; tool calls carry IDs so parallel tools cannot cross-wire. */
export function applyEvent(state: Transcript, event: EventRow): Transcript {
  if (event.id && event.id <= state.cursor) return state;
  const p = event.payload;
  let items = state.items;
  let stage = state.stage;
  let usage = state.usage;
  let plan = state.plan;
  const taskId = event.task_id || "";
  const text = String(p.text || p.summary || "");
  if (event.type === "agent.started") {
    items = [...items, { kind: "user", text: String(p.task || "") }];
    stage = "UNDERSTAND";
    plan = [];
    usage = {};
  }
  if (event.type === "model.delta" && text)
    items = [...items, { kind: "agent", text }];
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
    const card: ChatItem = {
      kind: "tool",
      tool: String(p.tool),
      text: String(p.output_preview || p.error || ""),
      fullOutput: String(p.output_full || p.output_preview || p.error || ""),
      live: false,
      ok: Boolean(p.success),
      collapsed: true,
      headline: String(p.headline || p.tool),
      icon: String(p.icon || ""),
      taskId,
      callId: String(p.call_id || ""),
      path: String(args?.path || args?.dest || ""),
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
  return { items, stage, usage, plan, cursor: event.id || state.cursor };
}

export function replay(events: EventRow[]): Transcript {
  return events.reduce(applyEvent, emptyTranscript());
}
