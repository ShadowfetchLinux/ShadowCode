import { describe, expect, it } from "vitest";
import { replay } from "./transcript";
import type { EventRow } from "../api";

const event = (
  id: number,
  type: string,
  payload: Record<string, unknown>,
  task_id = "one",
): EventRow => ({ id, ts: id, type, payload, task_id });

describe("rewind, review and approval rows", () => {
  const events = [
    event(1, "user.message", { text: "First" }),
    event(2, "agent.started", { task: "First" }),
    event(3, "agent.completed", { summary: "Done", success: true }),
    event(4, "user.message", { text: "Second" }, "two"),
    event(5, "agent.started", { task: "Second" }, "two"),
    event(6, "agent.completed", { summary: "Done", success: true }, "two"),
  ];

  it("keeps the event id of each prompt for Edit & resend", () => {
    const users = replay(events).items.filter((i) => i.kind === "user");
    expect(users.map((u) => u.kind === "user" && u.eventId)).toEqual([1, 4]);
  });

  it("puts the rewind divider above the rewound task's prompt", () => {
    const { items } = replay([
      ...events,
      event(
        7,
        "checkpoint.restored",
        { paths: ["a.txt", "b.txt"], undo_id: "u1" },
        "two",
      ),
    ]);
    const at = items.findIndex((i) => i.kind === "divider");
    expect(items[at]).toMatchObject({
      kind: "divider",
      rewound: true,
      text: "Rewound to here · 2 files restored",
    });
    expect(items[at + 1]).toMatchObject({ kind: "user", text: "Second" });
    const undone = replay([
      ...events,
      event(7, "checkpoint.restored", { paths: ["a.txt"] }, "two"),
      event(8, "checkpoint.rewind_undone", { paths: ["a.txt"] }, "two"),
    ]).items.at(-1);
    expect(undone).toMatchObject({
      kind: "note",
      text: "Rewind undone · 1 file put back as they were",
    });
  });

  it("notes review undos and deny notes", () => {
    const { items } = replay([
      ...events,
      event(
        7,
        "review.undone",
        { path: "src/app.ts", hunk: "h1", whole: false },
        "two",
      ),
      event(
        8,
        "approval.resolved",
        { approved: false, note: "use docs/" },
        "two",
      ),
      event(9, "approval.resolved", { approved: true, scope: "task" }, "two"),
    ]);
    const notes = items.filter((i) => i.kind === "note").map((i) => i.text);
    expect(notes).toContain("Review: undid one change in src/app.ts");
    expect(notes).toContain("Denied with a note: “use docs/”");
    expect(notes).toHaveLength(2);
  });
});
