import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { api, type Job } from "../api";
import { TaskSteerBar } from "./TaskSteerBar";

vi.mock("../api", () => ({ api: { pauseJob: vi.fn(), rewindJob: vi.fn() } }));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
const job: Job = {
  id: "task",
  session_id: "session",
  workspace: "/project",
  event_cursor: 0,
  started_at: 1,
  status: "running",
};

it("distinguishes task pause from goal pause and prevents rewind while running", async () => {
  const toast = vi.fn();
  render(<TaskSteerBar job={job} onToast={toast} />);
  const rewind = screen.getByRole("button", {
    name: "Rewind files",
  }) as HTMLButtonElement;
  expect(rewind.disabled).toBe(true);
  fireEvent.click(rewind);
  expect(api.rewindJob).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Pause task" }));
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith("Pause requested", "ok"),
  );
  expect(api.pauseJob).toHaveBeenCalledWith("task");
});

it("reports a pending pause boundary without claiming restoration succeeded", async () => {
  vi.mocked(api.rewindJob).mockRejectedValue(
    new Error("Wait for the pause boundary"),
  );
  const toast = vi.fn();
  render(<TaskSteerBar job={{ ...job, status: "paused" }} onToast={toast} />);
  fireEvent.click(screen.getByRole("button", { name: "Rewind files" }));
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith(
      "Error: Wait for the pause boundary",
      "err",
    ),
  );
  expect(toast).not.toHaveBeenCalledWith("Rewound files", "ok");
});
