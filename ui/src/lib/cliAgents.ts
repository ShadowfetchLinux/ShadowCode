import type { ModelInfo } from "../api";

export const LOCAL_GROUP = "Local model (ShadowCode agent)";
export const VENDOR_GROUP = "Claude / Codex / Grok (vendor agent)";

export type VendorState = {
  state?: string;
  status?: string;
  detail?: string;
  version?: string | null;
  fix?: string;
};

export type VendorStatusMap = Record<string, VendorState>;

export function isVendorProvider(provider?: string | null): boolean {
  return Boolean(provider && provider.startsWith("cli:"));
}

export function vendorId(provider?: string | null): string {
  return (provider || "").replace(/^cli:/, "");
}

export function groupModels(models: ModelInfo[]): {
  local: ModelInfo[];
  vendor: ModelInfo[];
  other: ModelInfo[];
} {
  const local: ModelInfo[] = [];
  const vendor: ModelInfo[] = [];
  const other: ModelInfo[] = [];
  for (const model of models) {
    if (isVendorProvider(model.provider) || model.metadata?.vendor_agent) {
      vendor.push(model);
    } else if (
      ["ollama", "local", "llamacpp", "vllm", "mock"].includes(model.provider)
    ) {
      local.push(model);
    } else {
      other.push(model);
    }
  }
  return { local, vendor, other };
}

export function chipLabel(state?: VendorState | null): string {
  switch (state?.state) {
    case "ready":
      return "Ready";
    case "not_logged_in":
      return "Not logged in";
    case "not_installed":
      return "Not installed";
    case "disabled":
      return "Disabled";
    default:
      return "Unknown";
  }
}

export function chipTone(
  state?: VendorState | null,
): "ok" | "warn" | "info" {
  if (state?.state === "ready") return "ok";
  if (state?.state === "not_logged_in") return "warn";
  return "info";
}
