import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  review: vi.fn(),
  reviewFile: vi.fn(),
  reviewUndo: vi.fn(),
  taskCheckpoint: vi.fn(),
  rewindTask: vi.fn(),
  undoRewind: vi.fn(),
  git: vi.fn(),
  gitDiff: vi.fn(),
}));
vi.mock("../api", () => ({ api: mocks }));

import { ReviewView } from "./ReviewView";
import { RewindDialog } from "./RewindDialog";
import { useRewind } from "../hooks/useRewind";

const hunk = (id: string, from: string, to: string) => ({
  id,
  header: `@@ -${id.slice(1)},1 +${id.slice(1)},1 @@`,
  lines: [
    { kind: "del", text: from },
    { kind: "add", text: to },
  ],
});
const detail = (hunks: ReturnType<typeof hunk>[]) => ({
  path: "src/app.ts",
  status: hunks.length ? "modified" : "unchanged",
  source: "checkpoint",
  added: hunks.length,
  removed: hunks.length,
  binary: false,
  hash: "h",
  hunks,
});

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  mocks.review.mockResolvedValue({
    task_id: "t1",
    session_id: "s1",
    workspace: "/w",
    busy: false,
    files: [
      {
        path: "src/app.ts",
        status: "modified",
        source: "checkpoint",
        added: 2,
        removed: 2,
        binary: false,
      },
      {
        path: "notes.md",
        status: "added",
        source: "checkpoint",
        added: 1,
        removed: 0,
        binary: false,
      },
    ],
  });
  mocks.reviewFile.mockResolvedValue(
    detail([hunk("h1", "a - b", "a + b"), hunk("h12", "V = 1", "V = 2")]),
  );
  mocks.reviewUndo.mockResolvedValue(detail([hunk("h12", "V = 1", "V = 2")]));
});
afterEach(cleanup);

function view(busy = false) {
  const toast = vi.fn();
  render(
    <ReviewView
      taskId="t1"
      busy={busy}
      onClose={vi.fn()}
      toast={toast}
      refresh={async () => {}}
      onAskAgent={vi.fn()}
      memory={{} as never}
      onMemory={vi.fn()}
    />,
  );
  return toast;
}

describe("ReviewView", () => {
  it("lists only the task's files and undoes one hunk", async () => {
    view();
    const list = await screen.findByRole("navigation", {
      name: "Changed files",
    });
    expect(within(list).getAllByRole("button")).toHaveLength(2);
    await screen.findByText("a + b");
    fireEvent.click(
      screen.getByRole("button", { name: "Undo change @@ -1,1 +1,1 @@" }),
    );
    await waitFor(() =>
      expect(mocks.reviewUndo).toHaveBeenCalledWith("t1", "src/app.ts", "h1"),
    );
    await waitFor(() => expect(screen.queryByText("a + b")).toBeNull());
    expect(document.querySelector(".review-diff")?.textContent).toContain(
      "V = 2",
    );
  });

  it("keeps a hunk without touching the file and switches layout", async () => {
    view();
    await screen.findByText("a + b");
    fireEvent.click(
      screen.getByRole("button", { name: "Keep change @@ -1,1 +1,1 @@" }),
    );
    expect(mocks.reviewUndo).not.toHaveBeenCalled();
    expect(screen.getByText("Kept")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Split" }));
    expect(document.querySelector(".diff-split")).toBeTruthy();
  });

  it("asks before Undo all, in the app's dialog", async () => {
    view();
    await screen.findByText("a + b");
    fireEvent.click(screen.getByRole("button", { name: "Undo all" }));
    const dialog = screen.getByRole("dialog", {
      name: "Undo this task's changes?",
    });
    await act(async () => {
      fireEvent.click(
        within(dialog).getByRole("button", { name: "Undo 2 files" }),
      );
    });
    expect(mocks.reviewUndo).toHaveBeenCalledWith(
      "t1",
      "src/app.ts",
      undefined,
    );
    expect(mocks.reviewUndo).toHaveBeenCalledWith("t1", "notes.md", undefined);
  });

  it("is read-only while a task runs", async () => {
    view(true);
    await screen.findByText("a + b");
    expect(screen.getByText(/review is read-only/)).toBeTruthy();
    const undo = screen.getByRole("button", {
      name: "Undo change @@ -1,1 +1,1 @@",
    }) as HTMLButtonElement;
    expect(undo.disabled).toBe(true);
    expect(
      (screen.getByRole("button", { name: "Undo all" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });
});

describe("rewind", () => {
  it("lists the files in the confirmation", () => {
    const confirm = vi.fn();
    render(
      <RewindDialog
        paths={["src/app.ts", "notes.md"]}
        onConfirm={confirm}
        onCancel={vi.fn()}
      />,
    );
    const files = screen.getByRole("list", { name: "Files that will change" });
    expect(files.textContent).toContain("src/app.ts");
    fireEvent.click(screen.getByRole("button", { name: "Rewind 2 files" }));
    expect(confirm).toHaveBeenCalled();
  });

  it("asks, rewinds, and offers Undo in the notification", async () => {
    mocks.taskCheckpoint.mockResolvedValue({
      rewindable: true,
      checkpoint: { changes: 1, paths: ["src/app.ts"] },
    });
    mocks.rewindTask.mockResolvedValue({
      ok: true,
      restored: ["src/app.ts"],
      undo_id: "u1",
    });
    mocks.undoRewind.mockResolvedValue({
      ok: true,
      restored: ["src/app.ts"],
      task_id: "t1",
    });
    const toast = vi.fn();
    const refresh = vi.fn(async () => {});
    const { result } = renderHook(() =>
      useRewind({ busy: false, refresh, toast }),
    );
    await act(() => result.current.ask("t1"));
    expect(result.current.asking).toEqual({
      taskId: "t1",
      paths: ["src/app.ts"],
    });
    expect(mocks.rewindTask).not.toHaveBeenCalled();
    await act(() => result.current.confirm());
    expect(mocks.rewindTask).toHaveBeenCalledWith("t1");
    expect(result.current.asking).toBeNull();
    const [text, kind, action] = toast.mock.calls.at(-1)!;
    expect(text).toBe("Restored 1 file.");
    expect(kind).toBe("ok");
    expect(action.label).toBe("Undo");
    await act(async () => action.run());
    await waitFor(() => expect(mocks.undoRewind).toHaveBeenCalledWith("u1"));
  });

  it("does not ask while a task runs", async () => {
    const toast = vi.fn();
    const { result } = renderHook(() =>
      useRewind({ busy: true, refresh: async () => {}, toast }),
    );
    await act(() => result.current.ask("t1"));
    expect(result.current.asking).toBeNull();
    expect(toast).toHaveBeenCalledWith(
      "Stop the task before rewinding its files.",
      "info",
    );
  });
});
