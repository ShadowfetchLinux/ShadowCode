import { describe, expect, it } from "vitest";
import {
  defaultDraft,
  formatCost,
  formatDuration,
  formatWhen,
  nextRunText,
  runText,
  withKind,
  type Automation,
  type AutomationRun,
} from "./automations";
import { closingPr, issueFollowUp, type IssueLink } from "./issues";

/** Local wall-clock times, so the tests hold in any time zone. */
const at = (y: number, m: number, d: number, h: number, min: number) =>
  new Date(y, m - 1, d, h, min).getTime() / 1000;

describe("next-run display", () => {
  const now = at(2026, 9, 25, 10, 0); // a Friday
  it("says minutes, today, tomorrow, or the date", () => {
    expect(formatWhen(now + 30, now)).toBe("in a minute");
    expect(formatWhen(now + 5 * 60, now)).toBe("in 5 min");
    expect(formatWhen(at(2026, 9, 25, 18, 30), now)).toBe("Today 18:30");
    expect(formatWhen(at(2026, 9, 26, 9, 0), now)).toBe("Tomorrow 09:00");
    expect(formatWhen(at(2026, 9, 24, 9, 0), now)).toBe("Yesterday 09:00");
    expect(formatWhen(at(2026, 9, 28, 9, 0), now)).toBe("Mon 28 Sep 09:00");
  });
  it("prefers running and paused over the next time", () => {
    const base = {
      ...defaultDraft(true),
      id: "a",
      workspace: "/w",
      paused: false,
      next_run_at: now + 600,
      created_at: 0,
      updated_at: 0,
      description: "Weekdays at 09:00",
      running_run: null,
      last_run: null,
    } satisfies Automation;
    expect(nextRunText(base, now)).toBe("Next: in 10 min");
    expect(nextRunText({ ...base, running_run: "r1" }, now)).toBe(
      "Running now",
    );
    expect(nextRunText({ ...base, paused: true }, now)).toBe("Paused");
    expect(nextRunText({ ...base, next_run_at: null }, now)).toBe(
      "No upcoming time",
    );
  });
});

describe("run history text", () => {
  const run: AutomationRun = {
    id: "r",
    automation_id: "a",
    status: "completed",
    trigger: "schedule",
    started_at: 0,
  };
  it("formats duration, cost and status", () => {
    expect(formatDuration(null)).toBe("");
    expect(formatDuration(0.2)).toBe("1 s");
    expect(formatDuration(42)).toBe("42 s");
    expect(formatDuration(600)).toBe("10 min");
    expect(formatDuration(3 * 3600 + 5 * 60)).toBe("3 h 5 min");
    expect(formatCost(run)).toBe("");
    expect(formatCost({ ...run, usage: { total_tokens: 1234 } })).toBe(
      `${(1234).toLocaleString()} tokens`,
    );
    expect(formatCost({ ...run, usage: { cost_usd: 0.4 } })).toBe("$0.40");
    expect(
      formatCost({ ...run, usage: { cost_usd: 0.0012, cost_estimated: true } }),
    ).toBe("about $0.0012");
    expect(runText(run)).toBe("Finished");
    expect(runText({ ...run, status: "missed", missed: 3 })).toBe(
      "Missed (3 times)",
    );
    expect(runText({ ...run, status: "needs_approval" })).toBe(
      "Stopped for approval",
    );
  });
});

describe("drafts", () => {
  it("default to a worktree only in a repository, and to stopping at approvals", () => {
    expect(defaultDraft(true).options.checkout).toBe("worktree");
    expect(defaultDraft(false).options.checkout).toBe("main");
    expect(defaultDraft(true).options.on_approval).toBe("stop");
  });
  it("keep the chosen time when the repeat changes", () => {
    const daily = { kind: "daily", time: "07:15" } as const;
    expect(withKind(daily, "weekly")).toEqual({
      kind: "weekly",
      day: 1,
      time: "07:15",
    });
    expect(withKind(daily, "weekdays")).toEqual({
      kind: "weekdays",
      time: "07:15",
    });
    expect(withKind(daily, "hourly")).toEqual({ kind: "hourly", minute: 0 });
    expect(withKind({ kind: "hourly", minute: 5 }, "daily")).toEqual({
      kind: "daily",
      time: "09:00",
    });
  });
});

describe("issue follow-up", () => {
  const link: IssueLink = {
    number: 12,
    title: "Login times out",
    url: "https://github.com/octo/demo/issues/12",
    provider: "github",
    marker: "Resolve GitHub issue #12:",
    branch: "issue-12-login-times-out",
  };
  const job = {
    task: "Resolve GitHub issue #12: Login times out\n…",
    status: "completed",
  };
  it("offers the pull request only after that issue's task completed", () => {
    expect(issueFollowUp(link, job, false)).toBe(link);
    expect(issueFollowUp(link, job, true)).toBeNull();
    expect(issueFollowUp(link, { ...job, status: "failed" }, false)).toBeNull();
    expect(
      issueFollowUp(link, { ...job, task: "Something else" }, false),
    ).toBeNull();
    expect(issueFollowUp(null, job, false)).toBeNull();
  });
  it("writes a pull request that closes the issue", () => {
    expect(closingPr(link, "develop")).toEqual({
      title: "Login times out",
      body: "Closes #12\n\nhttps://github.com/octo/demo/issues/12",
      base: "develop",
      draft: false,
    });
  });
});
