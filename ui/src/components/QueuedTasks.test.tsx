import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueuedTasks } from "./QueuedTasks";
import { api, type Job } from "../api";
vi.mock("../api", () => ({ api: { job: vi.fn() } }));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
it("loads a complete queued message only when its compact preview is expanded", async () => {
  const job: Job = {
    id: "job",
    session_id: "session",
    workspace: "/project",
    status: "queued",
    started_at: 1,
    event_cursor: 0,
    task: "preview",
    task_truncated: true,
    purpose: "tester",
  };
  vi.mocked(api.job).mockResolvedValue({
    ...job,
    task: "Full queued instructions beyond the preview",
  });
  render(
    <QueuedTasks
      jobs={[job]}
      sessions={[]}
      selected="session"
      cancelling={[]}
      disabled={false}
      onCancel={() => {}}
      onOpen={() => {}}
    />,
  );
  expect(api.job).not.toHaveBeenCalled();
  expect(screen.getByText("Test")).toBeTruthy();
  const details = screen.getByText("preview…").closest("details")!;
  details.open = true;
  fireEvent(details, new Event("toggle"));
  await waitFor(() =>
    expect(
      screen.getByText("Full queued instructions beyond the preview"),
    ).toBeTruthy(),
  );
  expect(api.job).toHaveBeenCalledExactlyOnceWith("job");
  details.open = false;
  fireEvent(details, new Event("toggle"));
  details.open = true;
  fireEvent(details, new Event("toggle"));
  expect(api.job).toHaveBeenCalledTimes(1);
});
