import { describe, expect, it } from "vitest";
import {
  breakdownRows,
  chipInput,
  contextChip,
  formatCost,
  formatTokens,
} from "./usageChip";
import { applyEvent, emptyTranscript } from "./transcript";
import type { RoutingDecision } from "../api";

const route = (provider: string, context_limit = 128000): RoutingDecision => ({
  purpose: "coder",
  source: "picker",
  requested: "x",
  model_id: "x",
  model_name: "X",
  provider,
  context_limit,
});

describe("context & cost chip", () => {
  it("formats tokens and costs", () => {
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(38_400)).toBe("38k");
    expect(formatTokens(1_500)).toBe("1.5k");
    expect(formatTokens(128_000)).toBe("128k");
    expect(formatTokens(1_200_000)).toBe("1.2M");
    expect(formatCost({ cost_usd: 0.1234 }, false)).toBe("$0.12");
    expect(formatCost({ cost_usd: 0.004 }, false)).toBe("$0.004");
    expect(formatCost({ cost_usd: 0.5, cost_estimated: true }, false)).toBe(
      "$0.50 est.",
    );
    expect(formatCost({ cost_usd: null }, false)).toBe("");
    expect(formatCost({}, true)).toBe("$0 · local");
    expect(formatCost({ source: "local" }, false)).toBe("$0 · local");
  });

  it("shows context use and cost for native models", () => {
    const chip = contextChip({
      provider: "openrouter",
      local: false,
      budget: { used: 53_760, limit: 128_000 },
      usage: { cost_usd: 0.12, total_tokens: 60_000 },
    });
    expect(chip?.text).toBe("42% · 54k / 128k · $0.12");
    expect(chip?.percent).toBe(42);
    expect(chip?.level).toBe("ok");
    const local = contextChip({
      provider: "local",
      local: true,
      budget: { used: 100_000, limit: 110_000 },
    });
    expect(local?.text).toBe("91% · 100k / 110k · $0 · local");
    expect(local?.level).toBe("full");
    expect(
      contextChip({
        provider: "local",
        local: false,
        budget: { used: 80, limit: 100 },
      })?.level,
    ).toBe("warn");
  });

  it("hides the percentage for vendor CLIs and names who reported", () => {
    const chip = contextChip({
      provider: "cli:claude",
      vendor: "Claude Code",
      local: false,
      budget: { used: 1000, limit: 2000 },
      usage: { total_tokens: 38_000, cost_usd: 0.3, source: "vendor" },
    });
    expect(chip?.percent).toBeNull();
    expect(chip?.text).toBe("38k tokens · $0.30");
    expect(chip?.reportedBy).toBe("reported by Claude Code");
    expect(
      contextChip({ provider: "cli:codex", vendor: "Codex", local: false }),
    ).toBeNull();
    expect(
      contextChip({ provider: "cli:codex", local: false, usage: {} }),
    ).toBeNull();
  });

  it("builds its input from the route, falling back to the chosen model", () => {
    const state = { ...emptyTranscript(), budget: { used: 10, limit: 100 } };
    const native = chipInput(route("openrouter"), undefined, state);
    expect(native.local).toBe(false);
    expect(native.contextLimit).toBe(128000);
    const vendor = chipInput(route("cli:codex"), undefined, state);
    expect(vendor.vendor).toBe("Codex");
    const local = chipInput(route("local"), undefined, state);
    expect(local.local).toBe(true);
  });

  it("lists a breakdown with compaction", () => {
    const rows = Object.fromEntries(
      breakdownRows(
        {
          provider: "openrouter",
          local: false,
          budget: { used: 9000, limit: 32000 },
          usage: {
            prompt_tokens: 12000,
            completion_tokens: 800,
            cached_tokens: 4000,
            cost_usd: 0.02,
            cost_estimated: true,
            turns: 3,
          },
        },
        {
          ts: 1000,
          before: 30000,
          after: 9000,
          omitted: 14,
          method: "model_summary",
        },
        1000 + 300,
      ),
    );
    expect(rows["Input tokens"]).toBe((12000).toLocaleString());
    expect(rows["Cached input"]).toBe((4000).toLocaleString());
    expect(rows.Cost).toBe("$0.02 (estimated from the price list)");
    expect(rows["Model requests"]).toBe("3");
    expect(rows["Last compaction"]).toBe(
      "5 min ago · model summary · 14 messages folded · 30k → 9k",
    );
    expect(rows.Context).toContain("9k of 32k");
  });

  it("follows usage.updated and context events in the transcript", () => {
    let state = emptyTranscript();
    state = applyEvent(state, {
      id: 1,
      ts: 1,
      type: "context.budget",
      payload: { used_estimated_tokens: 5000, limit: 32000 },
    });
    state = applyEvent(state, {
      id: 2,
      ts: 2,
      type: "usage.updated",
      payload: {
        purpose: "turn",
        turn: {},
        job: {},
        session: { total_tokens: 7000, cost_usd: 0.01 },
      },
    });
    // A vendor's rate-limit push is not the conversation's usage.
    state = applyEvent(state, {
      id: 3,
      ts: 3,
      type: "usage.updated",
      payload: { vendor: "codex", usage: { total_tokens: 1 } },
    });
    expect(state.budget).toEqual({ used: 5000, limit: 32000 });
    expect(state.sessionUsage?.total_tokens).toBe(7000);
    state = applyEvent(state, {
      id: 4,
      ts: 4,
      type: "context.compacted",
      payload: {
        before_estimated_tokens: 30000,
        after_estimated_tokens: 4000,
        omitted_messages: 9,
        method: "bounded_history",
      },
    });
    expect(state.compaction?.omitted).toBe(9);
    expect(state.budget?.used).toBe(4000);
  });
});
