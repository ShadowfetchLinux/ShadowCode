import { describe, expect, it } from "vitest";
import {
  availabilityLabel,
  groupTargets,
  rowAction,
  UNKNOWN_USAGE,
  usageDetailLines,
  usageLabel,
  vendorSections,
  type PickerTarget,
} from "./picker";

const now = 1_800_000_000;
const row = (over: Partial<PickerTarget>): PickerTarget => ({
  id: "cli:codex:gpt-6-astra",
  provider: "cli:codex",
  group: "subscriptions",
  name: "Codex · GPT-6-Astra",
  inference: "cloud",
  availability: "ready",
  availability_label: "Ready",
  usage: { state: "unavailable", label: UNKNOWN_USAGE },
  ...over,
});

describe("usage labels", () => {
  it("uses the backend label and never invents numbers", () => {
    expect(
      usageLabel({
        state: "ok",
        label: "Shared plan usage · 2% left · resets Fri",
      }),
    ).toBe("Shared plan usage · 2% left · resets Fri");
    expect(usageLabel({ state: "unavailable", label: "" })).toBe(UNKNOWN_USAGE);
    expect(usageLabel(null)).toBe(UNKNOWN_USAGE);
    expect(usageLabel({ state: "unavailable", label: "" })).not.toMatch(/\d+%/);
  });

  it("computes the age of a stale snapshot at render time", () => {
    expect(
      usageLabel(
        { state: "stale", label: "", last_refresh: now - 5 * 60 },
        "cloud",
        now,
      ),
    ).toBe("Last checked 5m ago");
    expect(
      usageLabel(
        { state: "stale", label: "2% left", last_refresh: now - 7200 },
        "cloud",
        now,
      ),
    ).toBe("2% left · Last checked 2h ago");
  });

  it("labels local rows as free of subscription quota", () => {
    expect(usageLabel(undefined, "local")).toBe(
      "Runs on this computer · No subscription quota",
    );
  });

  it("lists windows with reset times, pool and last check only when reported", () => {
    const lines = usageDetailLines(
      {
        state: "ok",
        label: "2% left",
        detail: ["ChatGPT Pro"],
        pool: "codex",
        pool_shared: true,
        windows: [
          {
            label: "Weekly",
            used_percent: 98,
            remaining_percent: 2,
            window_minutes: 10080,
            resets_at: now + 3 * 3600,
          },
        ],
        credits: null,
        last_refresh: now - 60,
      },
      now,
    );
    expect(lines[0]).toBe("ChatGPT Pro");
    expect(lines).toContain("Weekly · 2% left · resets in 3h");
    expect(lines.some((l) => l.startsWith("Shared pool: codex"))).toBe(true);
    expect(lines).toContain("Last checked 1m ago");
    expect(usageDetailLines({ state: "unavailable", label: "" }, now)).toEqual(
      [],
    );
  });
});

describe("grouping", () => {
  it("puts every cloud row under Subscriptions even when not featured", () => {
    const grok = row({
      id: "cli:grok",
      provider: "cli:grok",
      name: "Grok · Default",
      featured: false,
    });
    const local = row({
      id: "local:gguf:abc",
      provider: "llamacpp",
      group: "local",
      name: "qwen3:14b · This computer",
      inference: "local",
    });
    const groups = groupTargets([grok, local, row({})]);
    expect(groups.subscriptions.map((t) => t.id)).toEqual([
      "cli:codex:gpt-6-astra",
      "cli:grok",
    ]);
    expect(groups.local.map((t) => t.name)).toEqual([
      "qwen3:14b · This computer",
    ]);
  });

  it("collapses long vendor lists to default, recent and selected rows", () => {
    const cursor = Array.from({ length: 40 }, (_, i) =>
      row({
        id: `cli:cursor:m${i}`,
        provider: "cli:cursor",
        name: `Cursor · Model ${i}`,
        is_default: i === 0,
      }),
    );
    const [section] = vendorSections(cursor, {
      expanded: [],
      recent: ["cli:cursor:m7"],
      selected: "cli:cursor:m9",
      searching: false,
    });
    expect(section.rows.map((r) => r.id)).toEqual([
      "cli:cursor:m0",
      "cli:cursor:m7",
      "cli:cursor:m9",
    ]);
    expect(section.hidden).toBe(37);
    const [open] = vendorSections(cursor, {
      expanded: ["cursor"],
      recent: [],
      selected: "",
      searching: false,
    });
    expect(open.rows).toHaveLength(40);
  });
});

describe("row actions", () => {
  it("never leaves a non-ready row without an explanation or a fix", () => {
    expect(rowAction(row({}))).toEqual({ kind: "select" });
    expect(
      rowAction(row({ provider: "cli:cursor", availability: "sign_in" })),
    ).toEqual({ kind: "connect", vendor: "cursor" });
    expect(
      rowAction(
        row({
          availability: "setup_required",
          reason: "Install the Codex CLI",
        }),
      ),
    ).toMatchObject({
      kind: "setup",
      hint: "Install the Codex CLI",
      local: false,
    });
    expect(
      rowAction(row({ availability: "unavailable", reason: "Offline mode" })),
    ).toEqual({ kind: "explain", reason: "Offline mode" });
    expect(
      availabilityLabel(
        row({ availability: "sign_in", availability_label: "" }),
      ),
    ).toBe("Sign in");
  });
});
