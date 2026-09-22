import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { api } from "../api";
import { AdvancedTools } from "./AdvancedTools";
vi.mock("../api", () => ({
  api: {
    parallelPlan: vi.fn(),
    guardianStatus: vi.fn(),
    prepareParallel: vi.fn(),
    parallelWorkerStatus: vi.fn(),
    verifyParallel: vi.fn(),
    cleanupParallel: vi.fn(),
    runGuardian: vi.fn(),
  },
}));
beforeEach(() => {
  vi.mocked(api.guardianStatus).mockResolvedValue({ enabled: false });
  vi.mocked(api.parallelPlan).mockResolvedValue({ plan: null });
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
it("reports preparation errors and never implies a model task was started", async () => {
  render(<AdvancedTools />);
  fireEvent.change(screen.getByLabelText("Work items"), {
    target: { value: "Fix the regression" },
  });
  await waitFor(() =>
    expect(
      (screen.getByText("Prepare workspaces") as HTMLButtonElement).disabled,
    ).toBe(false),
  );
  vi.mocked(api.prepareParallel).mockResolvedValue({
    ok: false,
    error: "Open a Git repository",
  });
  fireEvent.click(screen.getByText("Prepare workspaces"));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Open a Git repository",
  );
  expect(
    (screen.getByText("Run diagnostics") as HTMLButtonElement).disabled,
  ).toBe(true);
});
it("shows combined conflicts and retains backend cleanup refusal", async () => {
  vi.mocked(api.parallelPlan).mockResolvedValue({
    plan: {
      id: "p",
      goal: "work",
      source: "/source",
      lead_note: "Committed HEAD",
      verify_status: "pending",
      workers: [
        {
          item: { id: "w1", title: "Fix", prompt: "fix" },
          branch: "shadowcode/p",
          worktree_path: "/worker",
          status: "finished",
        },
      ],
    },
  });
  vi.mocked(api.verifyParallel).mockResolvedValue({
    ok: false,
    conflicts: [{ worker: "w1", detail: "same.txt conflicts" }],
  });
  vi.mocked(api.cleanupParallel).mockRejectedValue(
    new Error("Contains uncommitted work"),
  );
  const onOpen = vi.fn();
  render(<AdvancedTools onOpen={onOpen} />);
  await screen.findByText("shadowcode/p");
  fireEvent.click(screen.getByText("Open workspace"));
  expect(onOpen).toHaveBeenCalledWith("/worker");
  fireEvent.click(screen.getByText("Check integration"));
  expect((await screen.findByRole("status")).textContent).toContain(
    "same.txt conflicts",
  );
  await waitFor(() =>
    expect(
      (screen.getByText("Remove clean checkouts") as HTMLButtonElement)
        .disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByText("Remove clean checkouts"));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "uncommitted work",
  );
  expect(screen.getByText("shadowcode/p")).toBeTruthy();
});
