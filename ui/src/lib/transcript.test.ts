import { describe, expect, it } from "vitest";
import { applyEvent, emptyTranscript, replay } from "./transcript";
import type { EventRow } from "../api";
const event = (
  id: number,
  type: string,
  payload: Record<string, unknown>,
  task_id = "one",
): EventRow => ({ id, ts: id, type, payload, task_id });
describe("durable transcript", () => {
  it("joins native stream chunks and replaces them with one complete response", () => {
    const state = replay([
      event(1, "user.message", { text: "Read the project" }),
      event(2, "agent.started", { task: "Read the project" }),
      event(3, "model.stream", { text: "Hello ", message_id: "reply" }),
      event(4, "model.stream", { text: "there", message_id: "reply" }),
      event(5, "model.delta", { text: "Hello there.", message_id: "reply" }),
      event(6, "agent.completed", { summary: "Hello there.", success: true }),
    ]);
    expect(state.items.map((i) => i.text)).toEqual([
      "Read the project",
      "Hello there.",
    ]);
    expect(state.items[1]).toMatchObject({ messageId: "reply", live: false });
  });
  it("keeps interrupted partial responses visibly incomplete", () => {
    const state = replay([
      event(1, "model.stream", { text: "Partial", message_id: "reply" }),
      event(2, "model.stream_end", { message_id: "reply", complete: false }),
      event(3, "agent.completed", {
        summary: "Task cancelled",
        cancelled: true,
        success: false,
      }),
    ]);
    expect(state.items[0]).toMatchObject({
      text: "Partial",
      live: false,
      who: "Interrupted response",
    });
    expect(state.stage).toBe("CANCELLED");
  });
  it("retains native tool paths and structured output for review cards", () => {
    const state = replay([
      event(1, "tool.started", {
        tool: "write_file",
        call_id: "write",
        arguments: { path: "src/main.rs" },
      }),
      event(2, "tool.completed", {
        tool: "write_file",
        call_id: "write",
        success: true,
        output: { path: "src/main.rs", bytes: 42 },
      }),
    ]);
    expect(state.items[0]).toMatchObject({
      path: "src/main.rs",
      ok: true,
      fullOutput: JSON.stringify({ path: "src/main.rs", bytes: 42 }, null, 2),
    });
  });
  it("replays every task once when the event stream reconnects", () => {
    const rows = [
      event(1, "agent.started", { task: "Make it work" }),
      event(2, "model.delta", { text: "Inspecting files" }),
      event(3, "agent.completed", { summary: "Finished", success: true }),
    ];
    const state = replay(rows);
    expect(rows.reduce(applyEvent, state)).toEqual(state);
    expect(state.items.map((i) => i.text)).toEqual([
      "Make it work",
      "Inspecting files",
      "Finished",
    ]);
  });
  it("does not duplicate a final model response", () => {
    expect(
      replay([
        event(1, "model.delta", { text: "Done" }),
        event(2, "agent.completed", { summary: "Done", success: true }),
      ]).items,
    ).toHaveLength(1);
  });
  it("matches simultaneous calls of the same tool by call ID", () => {
    const state = replay([
      event(1, "tool.started", { tool: "read_file", call_id: "a" }),
      event(2, "tool.started", { tool: "read_file", call_id: "b" }),
      event(3, "tool.completed", {
        tool: "read_file",
        call_id: "b",
        success: true,
        output_full: "second",
      }),
      event(4, "tool.completed", {
        tool: "read_file",
        call_id: "a",
        success: true,
        output_full: "first",
      }),
    ]);
    expect(state.items.map((i) => i.kind === "tool" && i.fullOutput)).toEqual([
      "first",
      "second",
    ]);
  });
  it("continues well past the former 800-event boundary", () => {
    let state = emptyTranscript();
    for (let i = 1; i <= 1600; i++)
      state = applyEvent(state, event(i, "model.delta", { text: String(i) }));
    expect(state.cursor).toBe(1600);
    expect(state.items).toHaveLength(1600);
  });
  it("records stopped and failed runs without claiming completion", () => {
    expect(
      replay([event(1, "agent.completed", { cancelled: true, success: false })])
        .stage,
    ).toBe("CANCELLED");
    expect(
      replay([event(1, "agent.completed", { success: false })]).stage,
    ).toBe("FAILED");
  });
});
