import type { RoutingDecision, Usage } from "../api";
import { isLocal, type PickerTarget } from "./picker";
import { providerLabel, type Transcript } from "./transcript";

/** What the chip shows for the running (or last) route, else the model
 * chosen for the next message. */
export function chipInput(
  route: RoutingDecision | null | undefined,
  target: PickerTarget | undefined,
  transcript: Pick<Transcript, "budget" | "sessionUsage" | "usage">,
): ChipInput {
  const provider =
    route?.provider ||
    (target && !isLocal(target) && target.id.startsWith("cli:")
      ? target.id.split(":").slice(0, 2).join(":")
      : "");
  const usage: Usage | undefined =
    transcript.sessionUsage || (transcript.usage as Usage);
  const local =
    usage?.source === "local" ||
    (route
      ? /^(local|llamacpp|ollama)/.test(route.provider || "")
      : Boolean(target && isLocal(target)));
  return {
    provider,
    vendor: provider.startsWith("cli:") ? providerLabel(provider) : undefined,
    local,
    budget: transcript.budget,
    contextLimit: route?.context_limit || 0,
    usage,
  };
}

/** What the context & cost chip knows about the open conversation. */
export type ChipInput = {
  /** The route's provider ("local", "openrouter", "cli:claude", …). */
  provider?: string;
  /** Product name for a subscription CLI ("Claude Code"). */
  vendor?: string;
  /** The model runs on this computer. */
  local: boolean;
  /** The latest context estimate (ShadowCode's own agent loop only). */
  budget?: { used: number; limit: number };
  /** The model's context window, when the budget has not arrived yet. */
  contextLimit?: number;
  /** The conversation's token and cost totals. */
  usage?: Usage;
};

export type Chip = {
  text: string;
  /** Context use in percent, or null when unknown (vendor CLIs). */
  percent: number | null;
  level: "ok" | "warn" | "full";
  /** "reported by Claude Code" for vendor numbers. */
  reportedBy: string | null;
  label: string;
};

/** 950 · 38k · 128k · 1.2M */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  if (n >= 1_000_000) {
    const m = n / 1_000_000;
    return `${m >= 10 ? Math.round(m) : Number(m.toFixed(1))}M`;
  }
  if (n >= 1000) {
    const k = n / 1000;
    return `${k >= 10 ? Math.round(k) : Number(k.toFixed(1))}k`;
  }
  return String(Math.round(n));
}

/** "$0.12", "$0.004 est.", "$0 · local", or "" when no cost is known. */
export function formatCost(usage: Usage | undefined, local: boolean): string {
  if (local || usage?.source === "local") return "$0 · local";
  const cost = usage?.cost_usd;
  if (typeof cost !== "number" || !Number.isFinite(cost)) return "";
  const amount =
    cost === 0
      ? "$0"
      : cost < 0.01
        ? `$${cost.toFixed(3)}`
        : `$${cost.toFixed(2)}`;
  return usage?.cost_estimated ? `${amount} est.` : amount;
}

const isVendor = (provider?: string) => Boolean(provider?.startsWith("cli:"));

/** The chip's text, or null when nothing is known yet. Native models show
 * context use from the engine's estimate; vendor CLIs show the tokens they
 * reported (no percentage: their context is theirs to manage). */
export function contextChip(input: ChipInput): Chip | null {
  const usage = input.usage || {};
  const cost = formatCost(usage, input.local);
  const tokens =
    usage.total_tokens ||
    (usage.prompt_tokens || 0) + (usage.completion_tokens || 0);
  if (isVendor(input.provider)) {
    if (!tokens && !cost) return null;
    const who = input.vendor || "the subscription";
    const parts = [tokens ? `${formatTokens(tokens)} tokens` : "", cost];
    const text = parts.filter(Boolean).join(" · ");
    return {
      text,
      percent: null,
      level: "ok",
      reportedBy: `reported by ${who}`,
      label: `${text}, reported by ${who}`,
    };
  }
  const limit = input.budget?.limit || input.contextLimit || 0;
  const used = input.budget?.used || 0;
  if (limit > 0 && used > 0) {
    const percent = Math.min(100, Math.round((used / limit) * 100));
    const text = [
      `${percent}%`,
      `${formatTokens(used)} / ${formatTokens(limit)}`,
      cost,
    ]
      .filter(Boolean)
      .join(" · ");
    return {
      text,
      percent,
      level: percent >= 90 ? "full" : percent >= 75 ? "warn" : "ok",
      reportedBy: null,
      label: `${percent}% of context used, ${formatTokens(used)} of ${formatTokens(limit)} tokens${cost ? `, ${cost}` : ""}`,
    };
  }
  if (!tokens && !cost) return null;
  const text = [tokens ? `${formatTokens(tokens)} tokens` : "", cost]
    .filter(Boolean)
    .join(" · ");
  return { text, percent: null, level: "ok", reportedBy: null, label: text };
}

/** The breakdown rows under the chip: [label, value]. */
export function breakdownRows(
  input: ChipInput,
  compaction?: {
    ts: number;
    before: number;
    after: number;
    omitted: number;
    method: string;
  },
  now = Date.now() / 1000,
): [string, string][] {
  const usage = input.usage || {};
  const rows: [string, string][] = [];
  const limit = input.budget?.limit || input.contextLimit || 0;
  if (!isVendor(input.provider) && limit > 0)
    rows.push([
      "Context",
      `${formatTokens(input.budget?.used || 0)} of ${formatTokens(limit)} tokens (estimated)`,
    ]);
  rows.push(["Input tokens", (usage.prompt_tokens || 0).toLocaleString()]);
  rows.push(["Output tokens", (usage.completion_tokens || 0).toLocaleString()]);
  rows.push(["Cached input", (usage.cached_tokens || 0).toLocaleString()]);
  if (usage.cache_write_tokens)
    rows.push(["Cache writes", usage.cache_write_tokens.toLocaleString()]);
  const cost = formatCost(usage, input.local);
  rows.push([
    "Cost",
    cost
      ? usage.cost_estimated && !input.local
        ? `${cost.replace(/ est\.$/, "")} (estimated from the price list)`
        : cost
      : "Not reported",
  ]);
  rows.push(["Model requests", String(usage.turns || 0)]);
  rows.push([
    "Counted by",
    isVendor(input.provider)
      ? `Reported by ${input.vendor || "the subscription"}`
      : usage.estimated
        ? "ShadowCode (some counts estimated)"
        : input.local
          ? "This computer"
          : "The provider",
  ]);
  rows.push([
    "Last compaction",
    compaction
      ? `${ago(now - compaction.ts)} · ${compaction.method === "model_summary" ? "model summary" : "short digest"} · ${compaction.omitted} messages folded · ${formatTokens(compaction.before)} → ${formatTokens(compaction.after)}`
      : "None yet",
  ]);
  return rows;
}

function ago(seconds: number) {
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.round(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.round(seconds / 3600)} h ago`;
  return `${Math.round(seconds / 86400)} d ago`;
}
