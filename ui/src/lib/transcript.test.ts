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
  it("preserves running state during queue changes and moves each prompt to its execution turn", () => {
    const started = replay([
      event(1, "user.message", { text: "First" }),
      event(2, "agent.started", { task: "First" }),
      event(3, "plan.updated", {
        plan: { steps: [{ id: "one", title: "Work", status: "running" }] },
      }),
      event(4, "user.message", { text: "Second" }, "two"),
      event(5, "user.message", { text: "Cancel me" }, "three"),
      event(
        6,
        "agent.completed",
        {
          summary: "Queued task cancelled",
          cancelled: true,
          plan: { steps: [] },
          usage: { total_tokens: 0 },
        },
        "three",
      ),
    ]);
    expect(started.stage).toBe("UNDERSTAND");
    expect(started.activeTaskId).toBe("one");
    expect(started.plan).toHaveLength(1);
    const completed = applyEvent(
      started,
      event(7, "agent.completed", {
        summary: "First result",
        success: true,
        usage: { total_tokens: 30 },
      }),
    );
    expect(completed.usage.total_tokens).toBe(30);
    const next = applyEvent(
      completed,
      event(8, "agent.started", { task: "Second" }, "two"),
    );
    expect(next.items.at(-1)).toMatchObject({
      kind: "user",
      text: "Second",
      taskId: "two",
    });
    expect(
      next.items.filter(
        (item) => item.kind === "user" && item.taskId === "two",
      ),
    ).toHaveLength(1);
    expect(next.activeTaskId).toBe("two");
    expect(next.usage).toEqual({});
    expect(next.plan).toEqual([]);
  });
  it("keeps a final answer once when completion hooks follow it, without hiding a later task or failure", () => {
    const rows = [
      event(1, "model.delta", { text: "Done", message_id: "final" }),
      event(2, "hook.completed", {
        id: "completion",
        name: "verify",
        event: "on_complete",
        status: "passed",
        success: true,
      }),
      event(3, "agent.completed", { summary: "Done", success: true }),
    ];
    const first = replay(rows);
    expect(first.items.filter((item) => item.kind === "agent")).toHaveLength(1);
    expect(rows.reduce(applyEvent, first)).toEqual(first);
    const second = applyEvent(
      first,
      event(4, "agent.completed", { summary: "Done", success: true }, "two"),
    );
    expect(second.items.filter((item) => item.kind === "agent")).toHaveLength(
      2,
    );
    const failed = replay([
      rows[0],
      rows[1],
      event(3, "agent.completed", { summary: "Done", success: false }),
    ]);
    expect(failed.items.at(-1)).toMatchObject({
      kind: "agent",
      who: "Needs attention",
      taskId: "one",
    });
  });
  it("keeps interleaved hook checks separate from the tool they gate and replays their result once", () => {
    const rows = [
      event(1, "tool.started", { tool: "exec", call_id: "exec-one" }),
      event(2, "hook.started", {
        id: "check-one",
        name: "lint",
        event: "before_command",
        command: "lint",
        path: ".shadowcode/hooks/lint.yaml",
      }),
      event(3, "hook.completed", {
        id: "check-one",
        name: "lint",
        event: "before_command",
        command: "lint",
        path: ".shadowcode/hooks/lint.yaml",
        status: "failed",
        success: false,
        detail: "Exited with status 2",
        process: { stderr: "Fix this error", truncated: true },
      }),
      event(4, "tool.completed", {
        tool: "exec",
        call_id: "exec-one",
        success: false,
        error: "Action blocked by lifecycle command",
      }),
    ];
    const state = replay(rows);
    expect(state.items).toHaveLength(2);
    expect(state.items[0]).toMatchObject({
      kind: "tool",
      tool: "exec",
      ok: false,
      live: false,
    });
    expect(state.items[1]).toMatchObject({
      kind: "tool",
      tool: "hook",
      ok: false,
      live: false,
      headline: "Hook · lint",
      path: ".shadowcode/hooks/lint.yaml",
    });
    expect(state.items[1]).toHaveProperty(
      "fullOutput",
      "before_command\nlint\nExited with status 2\nFix this error\n[Output truncated]",
    );
    expect(rows.reduce(applyEvent, state)).toEqual(state);
  });
  it("replays selected workflow provenance and command cards without duplicates", () => {
    const rows = [
      event(1, "workflow.selected", {
        name: "audit",
        path: ".agents/skills/audit/SKILL.md",
        mode: "code",
        effective_mode: "review",
      }),
      event(2, "command.completed", {
        name: "run",
        result: { kind: "error", headline: "Command failed", body: "Exit: 7" },
      }),
    ];
    const state = replay(rows);
    expect(state.items[0]).toMatchObject({
      kind: "note",
      text: "Workflow /audit · .agents/skills/audit/SKILL.md · review",
    });
    expect(state.items[1]).toMatchObject({
      kind: "command",
      card: { kind: "error", body: "Exit: 7" },
    });
    expect(state.items.filter((item) => item.kind === "agent")).toHaveLength(0);
    expect(rows.reduce(applyEvent, state)).toEqual(state);
  });

  it("retains readable fallback notices from legacy conversations", () => {
    const state = replay([
      event(1, "routing.fallback", {
        purpose: "coder",
        requested: "missing",
        fallback: "local-model",
      }),
    ]);
    expect(state.items[0].text).toBe(
      "Using default: local-model · coder. The saved model is unavailable.",
    );
    expect(state.routing).toBeUndefined();
  });
  it("replays routing and fallback notices separately from model answers", () => {
    const rows = [
      event(1, "agent.started", { task: "Review the changes" }),
      event(2, "routing.fallback", {
        purpose: "reviewer",
        source: "fallback",
        model_id: "default",
        model_name: "local-coder",
        provider: "ollama",
        context_limit: 4096,
        fallback_reason: "Saved model is not registered",
      }),
      event(3, "model.delta", { text: "Review complete" }),
    ];
    const state = replay(rows);
    expect(state.items[1]).toMatchObject({ kind: "note", warning: true });
    expect(state.items[1].text).toContain(
      "Using default: local-coder · ollama · reviewer",
    );
    expect(state.items[1].text).toContain("not registered");
    expect(state.items.filter((item) => item.kind === "agent")).toHaveLength(1);
    expect(state.routing?.context_limit).toBe(4096);
    expect(rows.reduce(applyEvent, state)).toEqual(state);
    expect(
      applyEvent(state, event(4, "agent.started", { task: "Next task" }, "two"))
        .routing,
    ).toBeUndefined();
  });
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
  it("surfaces local context and autonomy notes without claiming verification", () => {
    const state = replay([
      event(1, "context.budget", {
        used_estimated_tokens: 1200,
        limit: 8000,
      }),
      event(2, "autonomy.budget", { ratio: 0.8, max_steps: 64 }),
      event(3, "runaway.warning", {
        action: "replan",
        tool: "read_file",
        repeats: 4,
      }),
      event(4, "runaway.warning", {
        kind: "assistant_text",
        action: "pause",
        repeats: 5,
      }),
      event(5, "runaway.warning", {
        kind: "prose_command",
        action: "pause",
        repeats: 5,
      }),
    ]);
    expect(state.items.map((item) => item.text).join("\n")).toMatch(
      /Context 1200\/8000/,
    );
    expect(state.items.some((item) => /Autonomy budget/.test(item.text))).toBe(
      true,
    );
    expect(state.items.some((item) => /Loop replan/.test(item.text))).toBe(
      true,
    );
    expect(
      state.items.some((item) => /assistant text repeated/.test(item.text)),
    ).toBe(true);
    expect(
      state.items.some((item) =>
        /described a command without calling a tool/.test(item.text),
      ),
    ).toBe(true);
  });
  it("replays 10000 stream events without dropping the last cursor", () => {
    const started = performance.now();
    let state = emptyTranscript();
    for (let i = 1; i <= 10000; i++)
      state = applyEvent(state, event(i, "model.delta", { text: String(i) }));
    expect(state.cursor).toBe(10000);
    expect(state.items).toHaveLength(10000);
    expect(state.items[9999].text).toBe("10000");
    expect(performance.now() - started).toBeLessThan(4000);
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
