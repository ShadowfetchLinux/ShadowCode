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

vi.mock("../api", () => ({
  api: { pauseJob: vi.fn(), rewindJob: vi.fn(), steerJob: vi.fn() },
}));
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

it("hides rewind for vendor CLI agent tasks", () => {
  const toast = vi.fn();
  render(
    <TaskSteerBar
      job={{
        ...job,
        status: "paused",
        routing: {
          purpose: "coder",
          source: "explicit",
          requested: "cli:codex",
          model_id: "cli:codex",
          model_name: "Codex (vendor agent)",
          provider: "cli:codex",
          context_limit: 200000,
        },
      }}
      onToast={toast}
    />,
  );
  expect(screen.queryByRole("button", { name: "Rewind files" })).toBeNull();
});

it("distinguishes task pause from goal pause and prevents rewind while running", async () => {
  const toast = vi.fn();
  render(<TaskSteerBar job={job} onToast={toast} />);
  // No rewind control while the task runs (only at a pause boundary).
  expect(screen.queryByRole("button", { name: "Rewind files" })).toBeNull();
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

it("pauses a running task before steering and leaves it paused", async () => {
  const toast = vi.fn();
  render(<TaskSteerBar job={job} onToast={toast} />);
  fireEvent.click(screen.getByRole("button", { name: "Steer" }));
  fireEvent.change(screen.getByLabelText("Steering instruction"), {
    target: { value: "  use db/v2.sql  " },
  });
  fireEvent.change(screen.getByLabelText("Manually edited path"), {
    target: { value: "db/v2.sql" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() =>
    expect(toast).toHaveBeenCalledWith(
      "Steering saved. Resume the task to apply it.",
      "ok",
    ),
  );
  // Pause is requested first so the engine accepts the instruction; the
  // trimmed instruction and the noted path are sent; the form closes.
  expect(api.pauseJob).toHaveBeenCalledWith("task");
  expect(api.steerJob).toHaveBeenCalledWith(
    "task",
    "use db/v2.sql",
    "db/v2.sql",
  );
  expect(vi.mocked(api.pauseJob).mock.invocationCallOrder[0]).toBeLessThan(
    vi.mocked(api.steerJob).mock.invocationCallOrder[0],
  );
  expect(screen.queryByLabelText("Steering instruction")).toBeNull();
});

it("does not pause again when steering an already paused task", async () => {
  const toast = vi.fn();
  render(<TaskSteerBar job={{ ...job, status: "paused" }} onToast={toast} />);
  fireEvent.click(screen.getByRole("button", { name: "Steer" }));
  fireEvent.change(screen.getByLabelText("Steering instruction"), {
    target: { value: "stop after tests" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.steerJob).toHaveBeenCalled());
  expect(api.pauseJob).not.toHaveBeenCalled();
  expect(api.steerJob).toHaveBeenCalledWith(
    "task",
    "stop after tests",
    undefined,
  );
});
