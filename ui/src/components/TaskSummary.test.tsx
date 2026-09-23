import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { TaskSummary } from "./TaskSummary";
import { WelcomeBanner } from "./WelcomeBanner";
import { emptyActivity, type TaskActivity } from "../lib/activity";

afterEach(() => cleanup());

const base: TaskActivity = {
  ...emptyActivity("t1"),
  startedAt: 100,
  finishedAt: 175,
  changed: ["src/app.ts", "README.md"],
  verification: {
    status: "last_command_failed",
    commands: [
      { command: "npm test", exit_code: 0, success: true },
      { command: "npm run lint", exit_code: 2, success: false },
    ],
  },
  finished: { success: true, cancelled: false, summary: "Done" },
};

it("lists changed files with diff counts, checks with exit codes and duration", async () => {
  const onReview = vi.fn();
  const diffStat = vi.fn(async (path: string) =>
    path === "src/app.ts" ? { add: 3, del: 1 } : null,
  );
  render(
    <TaskSummary
      activity={base}
      onReview={onReview}
      diffStat={diffStat}
      onRewind={vi.fn()}
    />,
  );
  expect(screen.getByText("Finished")).toBeTruthy();
  expect(screen.getByText(/1m 15s/)).toBeTruthy();
  expect(screen.getByText("npm test")).toBeTruthy();
  expect(screen.getByText("exit 0")).toBeTruthy();
  expect(screen.getByText("exit 2")).toBeTruthy();
  await waitFor(() => expect(screen.getByText("+3")).toBeTruthy());
  expect(screen.getByText("−1")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "src/app.ts" }));
  expect(onReview).toHaveBeenCalledWith("src/app.ts");
  fireEvent.click(screen.getByRole("button", { name: "Review changes" }));
  expect(onReview).toHaveBeenCalledWith();
  expect(screen.getByRole("button", { name: "Rewind" })).toBeTruthy();
});

it("states vendor-owned checks and unverified claims honestly", () => {
  render(
    <TaskSummary
      activity={{
        ...base,
        changed: [],
        verification: {
          status: "vendor_owned",
          commands: [],
          note: "The vendor CLI owns verification; ShadowCode does not claim a harness verdict.",
        },
      }}
      onReview={vi.fn()}
    />,
  );
  expect(screen.getByText(/vendor CLI owns verification/)).toBeTruthy();
  expect(screen.getByText("No file changes were recorded.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Rewind" })).toBeNull();
  cleanup();
  render(
    <TaskSummary
      activity={{
        ...base,
        verification: {
          status: "not_run",
          commands: [],
          presentedAs: "unverified",
        },
      }}
      onReview={vi.fn()}
    />,
  );
  expect(screen.getByText("No test or build command was run.")).toBeTruthy();
  expect(
    screen.getByText(/claims results that no recorded check confirms/),
  ).toBeTruthy();
});

it("welcome state offers at most three suggestions", () => {
  const onSelect = vi.fn();
  render(<WelcomeBanner onSelect={onSelect} />);
  const chips = screen.getAllByRole("button");
  expect(chips).toHaveLength(3);
  fireEvent.click(chips[0]);
  expect(onSelect).toHaveBeenCalled();
});
