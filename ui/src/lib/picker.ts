import type { ModelInfo } from "../api";

export type UsageSnapshot = {
  state?: string;
  label?: string;
  remaining_percent?: number | null;
  remaining_quantity?: number | null;
  window?: string | null;
  reset?: string | null;
  shared_pool?: string | null;
  last_refresh?: number | null;
  provider_usage_url?: string | null;
};

export type PickerTarget = {
  id: string;
  provider: string;
  account?: string;
  model?: string;
  route?: string;
  group: "subscriptions" | "local" | string;
  name: string;
  subtitle?: string;
  inference?: "local" | "cloud" | string;
  availability?: string;
  availability_label?: string;
  reason?: string;
  featured?: boolean;
  vision?: boolean;
  tools?: boolish;
  usage?: UsageSnapshot;
};

type boolish = boolean | undefined;

export function usageLabel(usage?: UsageSnapshot | null, inference?: string): string {
  if (inference === "local" || usage?.state === "local") {
    return "Runs on this computer · No subscription quota";
  }
  if (!usage) return "Usage unavailable";
  if (usage.state === "stale") return usage.label || "Last checked recently";
  if (
    usage.state === "unavailable" ||
    (usage.remaining_percent == null && usage.remaining_quantity == null)
  ) {
    return usage.label || "Usage unavailable";
  }
  return usage.label || "Usage unavailable";
}

export function availabilityLabel(target: PickerTarget): string {
  if (target.availability_label) return target.availability_label;
  switch (target.availability) {
    case "ready":
      return "Ready";
    case "sign_in":
      return "Sign in";
    case "setup_required":
      return "Setup required";
    default:
      return "Unavailable";
  }
}

export function isSelectable(target: PickerTarget): boolean {
  return target.availability === "ready";
}

export function groupTargets(targets: PickerTarget[]): {
  subscriptions: PickerTarget[];
  local: PickerTarget[];
} {
  const subscriptions: PickerTarget[] = [];
  const local: PickerTarget[] = [];
  for (const target of targets) {
    if (target.group === "local" || target.inference === "local") local.push(target);
    else if (target.featured !== false) subscriptions.push(target);
  }
  return { subscriptions, local };
}

/** Build picker rows from the existing models list when /api/picker is empty. */
export function targetsFromModels(models: ModelInfo[]): PickerTarget[] {
  return models.map((model) => {
    const vendor = Boolean(model.metadata?.vendor_agent || model.provider?.startsWith("cli:"));
    const local = ["ollama", "local", "llamacpp", "vllm"].includes(model.provider);
    const featured = model.metadata?.featured !== false && model.metadata?.kind !== "grok";
    return {
      id: model.id,
      provider: model.provider,
      model: model.name,
      group: local ? "local" : "subscriptions",
      name: vendor
        ? `${String(model.metadata?.product_label || model.name)} · ${model.metadata?.model === "auto" ? "Auto" : "Default"}`
        : `${model.name}${local ? " · This computer" : ""}`,
      subtitle: local
        ? "Runs on this computer · No subscription quota"
        : vendor
          ? "Cloud · subscription"
          : "Remote API",
      inference: local ? "local" : "cloud",
      availability: "ready",
      availability_label: "Ready",
      featured: vendor ? featured : local,
      vision: Boolean(model.metadata?.capabilities?.vision),
      tools: model.metadata?.capabilities?.tools !== false,
      usage: local
        ? { state: "local", label: "Runs on this computer · No subscription quota" }
        : { state: "unavailable", label: "Usage unavailable" },
    };
  });
}
