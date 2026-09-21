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
    reviewWorktreeCopy: vi.fn(),
    copyWorktreeChanges: vi.fn(),
    createWorktree: vi.fn(),
    inspectWorktree: vi.fn(),
    removeWorktree: vi.fn(),
    worktreeRecovery: vi.fn(),
    restoreWorktree: vi.fn(),
    reviewWorktreeReturn: vi.fn(),
    returnWorktreeChanges: vi.fn(),
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

it("reviews recovery explicitly and rejects stale recovery without reuse", async () => {
  vi.mocked(api.worktreeRecovery).mockResolvedValue({
    record,
    commit: "retained-commit",
    branch: record.branch,
    warning: "Missing uncommitted files are not reconstructed.",
    hash: "recovery-hash",
  });
  vi.mocked(api.restoreWorktree).mockRejectedValue(
    new Error("Recovery state changed"),
  );
  render(<WorktreeSettings onToast={vi.fn()} />);
  fireEvent.click(await screen.findByText("Review missing checkout"));
  const region = await screen.findByRole("region", {
    name: "Review worktree recovery",
  });
  expect(document.activeElement).toBe(region);
  expect(api.worktreeRecovery).toHaveBeenCalledWith("/source", "managed");
  expect(api.restoreWorktree).not.toHaveBeenCalled();
  await screen.findByText("Missing uncommitted files are not reconstructed.");
  fireEvent.click(screen.getByText("Restore in new worktree"));
  await screen.findByRole("alert");
  expect(api.restoreWorktree).toHaveBeenCalledWith(
    "/source",
    "managed",
    "recovery-hash",
  );
  expect(
    screen.queryByRole("region", { name: "Review worktree recovery" }),
  ).toBeNull();
  expect(api.removeWorktree).not.toHaveBeenCalled();
});

it("shows the incoming diff and reports merge conflicts as needing attention", async () => {
  vi.mocked(api.reviewWorktreeReturn).mockResolvedValue({
    record,
    source_head: "source-commit",
    source_branch: "main",
    worktree_head: "incoming-commit",
    worktree_branch: record.branch,
    merge_base: "base",
    diff: "+ reviewed change",
    hash: "return-hash",
  });
  vi.mocked(api.returnWorktreeChanges).mockResolvedValue({
    ...record,
    state: "needs_attention",
    detail: "Resolve the source conflict or use Git merge --abort.",
  });
  const toast = vi.fn();
  render(<WorktreeSettings onToast={toast} />);
  fireEvent.click(await screen.findByText("Review return"));
  const region = await screen.findByRole("region", {
    name: "Review returned changes",
  });
  expect(document.activeElement).toBe(region);
  expect(
    screen.getByRole("region", { name: "Incoming worktree diff" }).textContent,
  ).toContain("+ reviewed change");
  expect(api.returnWorktreeChanges).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Prepare merge in source"));
  await screen.findByRole("alert");
  expect(api.returnWorktreeChanges).toHaveBeenCalledWith(
    "/source",
    "managed",
    "return-hash",
  );
  expect(toast).toHaveBeenCalledWith(
    "Return needs attention in the source project",
    "err",
  );
  expect(
    screen.queryByRole("region", { name: "Review returned changes" }),
  ).toBeNull();
});

it("reviews separate staged and unstaged edits and rejects a stale copy", async () => {
  vi.mocked(api.reviewWorktreeCopy).mockResolvedValue({
    source: "/source",
    head: "reviewed-head",
    staged_diff: "+ staged text",
    unstaged_diff: "+ unstaged text",
    untracked: [{ path: "new.txt", bytes: 12, hash: "file-hash", mode: 420 }],
    intent_to_add: ["planned.txt"],
    hash: "copy-hash",
  });
  vi.mocked(api.copyWorktreeChanges).mockRejectedValue(
    new Error("Source changed; review again"),
  );
  render(<WorktreeSettings onToast={vi.fn()} />);
  await screen.findByText("shadowcode/managed");
  fireEvent.click(screen.getByText("Review current edits"));
  const region = await screen.findByRole("region", {
    name: "Review copied changes",
  });
  expect(document.activeElement).toBe(region);
  expect(api.reviewWorktreeCopy).toHaveBeenCalledWith("/source");
  expect(
    screen.getByRole("region", { name: "Staged copy diff" }).textContent,
  ).toContain("+ staged text");
  expect(
    screen.getByRole("region", { name: "Unstaged copy diff" }).textContent,
  ).toContain("+ unstaged text");
  expect(screen.getByText("new.txt")).toBeTruthy();
  expect(api.copyWorktreeChanges).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Copy into new worktree"));
  await screen.findByRole("alert");
  expect(api.copyWorktreeChanges).toHaveBeenCalledWith("/source", "copy-hash");
  expect(
    screen.queryByRole("region", { name: "Review copied changes" }),
  ).toBeNull();
  expect(api.createWorktree).not.toHaveBeenCalled();
});
