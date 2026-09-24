import type { AllowanceResponse, AllowanceRow, LimitsConfig } from "../api";
import { isLocal, isReady, shortName, type PickerTarget } from "./picker";

/** States of a source that can run a turn now (or just ran out). Rows that
 * need setup, sign-in or a key say so in the panel but never colour the dot. */
const READY_STATES = ["ok", "low", "limit_reached"];

/** The status-bar dot: "warn" when any ready source is low or at its plan
 * limit, "ok" when every ready source has room, "none" before anything is
 * known. */
export function allowanceLevel(
  rows: AllowanceRow[] | undefined,
): "ok" | "warn" | "none" {
  const ready = (rows || []).filter((row) => READY_STATES.includes(row.state));
  if (!ready.length) return "none";
  return ready.some(
    (row) => row.state === "low" || row.state === "limit_reached",
  )
    ? "warn"
    : "ok";
}

/** A short spoken/tooltip summary of the rows that need attention. */
export function allowanceSummary(rows: AllowanceRow[] | undefined): string {
  const parts = (rows || [])
    .filter((row) => row.state === "low" || row.state === "limit_reached")
    .map((row) =>
      row.state === "limit_reached"
        ? `${row.product}: plan limit reached`
        : `${row.product}: ${row.headline}`,
    );
  return parts.join(" · ");
}

/** "cli:claude" → "claude"; "openrouter" stays. The key Settings › Accounts
 * uses to focus a vendor card (the picker's Connect uses the same). */
export const accountKey = (row: AllowanceRow) => row.id.replace(/^cli:/, "");

export const formatUsd = (value: number) => `$${value.toFixed(2)}`;

export const planName = (plan: string) =>
  `${plan.charAt(0).toUpperCase()}${plan.slice(1)} plan`;

/** The task the engine gives the follow-up job; a manual "Continue on …"
 * uses the same words so both read the same in the conversation. */
export function continuationTask(from: string, request: string): string {
  return `Continue where ${from} stopped when its plan limit was reached. The request was:\n\n${request}`;
}

export const CONTINUATION =
  /^Continue where .+? stopped when its plan limit was reached\. The request was:/;

export const readyLocalTargets = (targets: PickerTarget[]) =>
  targets.filter((target) => isLocal(target) && isReady(target));

export type Fallback = { id: string; name: string };

/** The local model a plan limit continues on: the saved choice when it is
 * ready, else the engine's pick (Allowance › On this computer), else the
 * first ready local row with tools. */
export function resolveFallback(
  limits: LimitsConfig,
  allowance: AllowanceResponse | null,
  targets: PickerTarget[],
): Fallback | null {
  const local = readyLocalTargets(targets);
  const named = (target: PickerTarget): Fallback => ({
    id: target.id,
    name: shortName(target),
  });
  const saved = local.find((t) => t.id === limits.fallback_model);
  if (saved) return named(saved);
  const engine = allowance?.rows.find((row) => row.id === "local")?.fallback;
  if (engine) {
    const row = local.find((t) => t.id === engine.id);
    return row ? named(row) : engine;
  }
  const first = local.find((t) => t.tools) || local[0];
  return first ? named(first) : null;
}

export const limitsFrom = (cfg: Record<string, unknown>): LimitsConfig => {
  const limits = (cfg.limits || {}) as LimitsConfig;
  return {
    on_limit: limits.on_limit === "ask" ? "ask" : "local",
    fallback_model: limits.fallback_model || "",
  };
};
