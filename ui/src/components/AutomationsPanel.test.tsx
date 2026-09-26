import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { AutomationEditor, AutomationsPanel } from "./AutomationsPanel";
import {
  automationsApi,
  defaultDraft,
  type Automation,
} from "../lib/automations";

vi.mock("../api", () => ({
  api: {
    picker: vi.fn(async () => ({
      targets: [
        {
          id: "api:openrouter:qwen/qwen3-coder",
          name: "Qwen3 Coder",
          provider: "openrouter",
          group: "api",
          inference: "cloud",
          availability: "ready",
        },
        {
          id: "cli:cursor",
          name: "Cursor",
          provider: "cli:cursor",
          group: "subscriptions",
          inference: "cloud",
          availability: "sign_in",
        },
      ],
    })),
  },
}));
vi.mock("../lib/forge", async (original) => {
  const real = await original<typeof import("../lib/forge")>();
  return {
    ...real,
    forgeApi: { overview: vi.fn(async () => ({ repo: true })) },
  };
});
vi.mock("../lib/automations", async (original) => {
  const real = await original<typeof import("../lib/automations")>();
  return {
    ...real,
    automationsApi: {
      list: vi.fn(),
      get: vi.fn(),
      create: vi.fn(),
      update: vi.fn(),
      remove: vi.fn(),
      pause: vi.fn(),
      resume: vi.fn(),
      runNow: vi.fn(),
      stop: vi.fn(),
      preview: vi.fn(),
    },
  };
});

const mocked = vi.mocked(automationsApi);
const text = (element: Element) => element.textContent || "";
const disabled = (element: Element) => (element as HTMLButtonElement).disabled;

beforeEach(() => {
  mocked.preview.mockImplementation(async (schedule) =>
    schedule.kind === "cron" && schedule.expr === "nope"
      ? { ok: false, error: "Invalid minute field 'nope'" }
      : {
          ok: true,
          description: "Weekdays at 09:00",
          next: [1000 + 300, 1000 + 86_400, 1000 + 2 * 86_400],
          now: 1000,
        },
  );
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("previews the next times and saves the whole draft", async () => {
  const onSave = vi.fn(async () => undefined);
  render(
    <AutomationEditor
      initial={defaultDraft(true)}
      existing={false}
      onSave={onSave}
      onCancel={() => undefined}
    />,
  );
  expect(text(await screen.findByLabelText("Next runs"))).toContain(
    "Weekdays at 09:00. Next: in 5 min",
  );
  const create = screen.getByRole("button", { name: "Create automation" });
  expect(disabled(create)).toBe(true);
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "Morning review" },
  });
  fireEvent.change(screen.getByLabelText("What should it do?"), {
    target: { value: "Review yesterday's commits" },
  });
  // Only ready models are offered.
  await screen.findByRole("option", { name: "Qwen3 Coder" });
  expect(screen.queryByRole("option", { name: "Cursor" })).toBeNull();
  fireEvent.change(screen.getByLabelText("Model"), {
    target: { value: "api:openrouter:qwen/qwen3-coder" },
  });
  fireEvent.change(screen.getByLabelText("Mode"), { target: { value: "ask" } });
  fireEvent.change(screen.getByLabelText("Repeat"), {
    target: { value: "weekly" },
  });
  fireEvent.change(screen.getByLabelText("Day of the week"), {
    target: { value: "3" },
  });
  fireEvent.change(screen.getByLabelText("Time"), {
    target: { value: "18:30" },
  });
  fireEvent.click(
    screen.getByLabelText(/In the project folder/, { selector: "input" }),
  );
  fireEvent.click(screen.getByLabelText(/wait for me/, { selector: "input" }));
  fireEvent.change(screen.getByLabelText("Time limit in minutes"), {
    target: { value: "15" },
  });
  await waitFor(() =>
    expect(mocked.preview).toHaveBeenLastCalledWith(
      { kind: "weekly", day: 3, time: "18:30" },
      "local",
    ),
  );
  fireEvent.click(create);
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
  expect(onSave).toHaveBeenCalledWith({
    name: "Morning review",
    prompt: "Review yesterday's commits",
    model: "api:openrouter:qwen/qwen3-coder",
    mode: "ask",
    schedule: { kind: "weekly", day: 3, time: "18:30" },
    timezone: "local",
    options: {
      checkout: "main",
      permission: "project",
      on_approval: "wait",
      max_runtime_minutes: 15,
      catch_up_minutes: 120,
      notify: true,
    },
  });
});

it("explains an invalid cron expression and will not save it", async () => {
  render(
    <AutomationEditor
      initial={{ ...defaultDraft(true), name: "x", prompt: "y" }}
      existing
      onSave={vi.fn()}
      onCancel={() => undefined}
    />,
  );
  fireEvent.change(screen.getByLabelText("Repeat"), {
    target: { value: "cron" },
  });
  fireEvent.change(screen.getByLabelText("Cron expression"), {
    target: { value: "nope" },
  });
  expect(text(await screen.findByRole("alert"))).toContain(
    "Invalid minute field",
  );
  expect(disabled(screen.getByRole("button", { name: "Save" }))).toBe(true);
});

it("lists automations with their next time, runs one now and shows history", async () => {
  const now = Date.now() / 1000;
  const row: Automation = {
    ...defaultDraft(true),
    name: "Nightly tests",
    prompt: "Run the tests",
    id: "a1",
    workspace: "/work/demo",
    paused: false,
    next_run_at: now + 5 * 60 + 20,
    created_at: 1,
    updated_at: 1,
    description: "Every day at 02:00",
    running_run: null,
    last_run: {
      id: "r0",
      automation_id: "a1",
      status: "needs_approval",
      trigger: "schedule",
      started_at: now - 3600,
    },
  };
  mocked.list.mockResolvedValue({
    workspace: "/work/demo",
    automations: [row],
    scheduler: false,
    now,
  });
  mocked.runNow.mockResolvedValue({
    id: "r1",
    automation_id: "a1",
    status: "running",
    trigger: "manual",
    started_at: now,
  });
  mocked.get.mockResolvedValue({
    ...row,
    runs: [
      {
        id: "r1",
        automation_id: "a1",
        status: "completed",
        trigger: "manual",
        started_at: now - 60,
        finished_at: now,
        duration: 125,
        session_id: "s9",
        usage: { cost_usd: 0.25 },
        detail: "No files changed, so its temporary worktree was removed.",
      },
    ],
  });
  const toast = vi.fn();
  const open = vi.fn();
  render(<AutomationsPanel toast={toast} onOpenSession={open} />);
  const card = await screen.findByRole("article", { name: "Nightly tests" });
  expect(text(card)).toContain("Every day at 02:00");
  expect(text(card)).toContain("Next: in 5 min");
  expect(text(card)).toContain("Last: Stopped for approval");
  expect(text(screen.getByRole("status"))).toContain("shadowcode serve");
  fireEvent.click(screen.getByRole("button", { name: "Run now" }));
  await waitFor(() => expect(mocked.runNow).toHaveBeenCalledWith("a1"));
  expect(toast).toHaveBeenCalledWith("Started Nightly tests", "ok");
  fireEvent.click(screen.getByRole("button", { name: "History" }));
  const history = await screen.findByRole("list", { name: "Run history" });
  expect(text(history)).toContain("Finished");
  expect(text(history)).toContain("Run now · 2 min · $0.25");
  expect(text(history)).toContain("temporary worktree was removed");
  fireEvent.click(screen.getByRole("button", { name: "Open conversation" }));
  expect(open).toHaveBeenCalledWith("s9");
});
