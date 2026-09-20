import React from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { WorktreeSettings } from "./WorktreeSettings";
import { api, type ManagedWorktree, type WorktreeInspection } from "../api";
vi.mock("../api", () => ({
  api: {
    worktrees: vi.fn(),
    createWorktree: vi.fn(),
    inspectWorktree: vi.fn(),
    removeWorktree: vi.fn(),
  },
}));
const record: ManagedWorktree = {
  id: "managed",
  source: "/source",
  path: "/isolated",
  branch: "shadowcode/managed",
  base_commit: "abc",
  common_directory: "/source/.git",
  state: "ready",
  created_at: 1,
  detail: "Created",
};
const review: WorktreeInspection = {
  record,
  head: "abc",
  current_branch: record.branch,
  status: "",
  can_remove: true,
  reason: "Clean checkout",
  hash: "reviewed-hash",
};
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
  vi.mocked(api.worktrees).mockResolvedValue({
    workspace: "/source",
    worktrees: [record],
  });
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
it("keeps creation and reviewed removal tied to the displayed source", async () => {
  vi.mocked(api.createWorktree).mockResolvedValue(record);
  vi.mocked(api.inspectWorktree).mockResolvedValue(review);
  vi.mocked(api.removeWorktree).mockResolvedValue({
    ...record,
    state: "removed",
  });
  const onOpen = vi.fn();
  render(<WorktreeSettings onOpen={onOpen} onToast={vi.fn()} />);
  await screen.findByText("shadowcode/managed");
  fireEvent.click(screen.getByText("Open worktree"));
  expect(onOpen).toHaveBeenCalledWith("/isolated");
  fireEvent.change(screen.getByLabelText("Starting commit or branch"), {
    target: { value: "main" },
  });
  fireEvent.click(screen.getByText("Create worktree"));
  await waitFor(() =>
    expect(api.createWorktree).toHaveBeenCalledWith("/source", "main"),
  );
  await waitFor(() =>
    expect(
      (screen.getByText("Inspect removal") as HTMLButtonElement).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByText("Inspect removal"));
  await screen.findByRole("region", { name: "Review worktree removal" });
  expect(api.removeWorktree).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Remove clean worktree"));
  await waitFor(() =>
    expect(api.removeWorktree).toHaveBeenCalledWith(
      "/source",
      "managed",
      "reviewed-hash",
    ),
  );
});
it("blocks dirty removal and clears a stale review after server rejection", async () => {
  vi.mocked(api.inspectWorktree).mockResolvedValue({
    ...review,
    can_remove: false,
    status: "?? keep.txt",
    reason: "Local files remain",
  });
  render(<WorktreeSettings onToast={vi.fn()} />);
  await screen.findByText("Inspect removal");
  fireEvent.click(screen.getByText("Inspect removal"));
  await screen.findByText("Local files remain");
  expect(
    (screen.getByText("Remove clean worktree") as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(api.removeWorktree).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Keep worktree"));
  vi.mocked(api.inspectWorktree).mockResolvedValue(review);
  vi.mocked(api.removeWorktree).mockRejectedValue(
    new Error("Worktree changed; inspect it again"),
  );
  fireEvent.click(screen.getByText("Inspect removal"));
  await screen.findByText("Remove clean worktree");
  fireEvent.click(screen.getByText("Remove clean worktree"));
  await screen.findByRole("alert");
  expect(
    screen.queryByRole("region", { name: "Review worktree removal" }),
  ).toBeNull();
});
