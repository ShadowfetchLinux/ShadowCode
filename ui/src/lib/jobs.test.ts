import { expect, it } from "vitest";
import { conversationJob } from "./jobs";
import type { Job } from "../api";
const job = (
  id: string,
  status: string,
  started_at: number,
  finished_at?: number,
): Job => ({
  id,
  status,
  started_at,
  finished_at,
  workspace: "/project",
  session_id: "session",
  event_cursor: 0,
});
it("keeps the active task selected ahead of newer waiting or cancelled submissions", () => {
  const active = job("running", "running", 1);
  expect(
    conversationJob(
      [job("cancelled", "cancelled", 3, 4), job("later", "queued", 2), active],
      null,
    ),
  ).toBe(active);
});
it("selects the oldest waiting task, then the most recent actual completion", () => {
  const first = job("first", "queued", 1);
  expect(conversationJob([job("second", "queued", 2), first], null)).toBe(
    first,
  );
  const done = job("first", "completed", 1, 5);
  expect(conversationJob([job("second", "cancelled", 2, 3), done], null)).toBe(
    done,
  );
  expect(conversationJob([], done)).toBe(done);
});
