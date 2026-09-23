import { expect, it } from "vitest";
import {
  chipLabel,
  groupModels,
  isVendorProvider,
  LOCAL_GROUP,
  VENDOR_GROUP,
} from "./cliAgents";
import type { ModelInfo } from "../api";

const models: ModelInfo[] = [
  { id: "qwen", name: "qwen2.5-coder", provider: "ollama", endpoint: "http://127.0.0.1:11434/v1" },
  {
    id: "cli:codex",
    name: "Codex (vendor agent)",
    provider: "cli:codex",
    endpoint: "",
    metadata: { vendor_agent: true, label: "Codex (vendor agent)" },
  },
  { id: "gpt", name: "gpt-4.1", provider: "openai_compatible", endpoint: "https://api.openai.com/v1" },
];

it("splits local ShadowCode agents from vendor CLI agents", () => {
  const groups = groupModels(models);
  expect(groups.local.map((m) => m.id)).toEqual(["qwen"]);
  expect(groups.vendor.map((m) => m.id)).toEqual(["cli:codex"]);
  expect(groups.other.map((m) => m.id)).toEqual(["gpt"]);
  expect(isVendorProvider("cli:claude")).toBe(true);
  expect(isVendorProvider("ollama")).toBe(false);
  expect(LOCAL_GROUP).toContain("ShadowCode agent");
  expect(VENDOR_GROUP).toContain("vendor agent");
});

it("labels doctor chip states without mentioning credentials", () => {
  expect(chipLabel({ state: "ready" })).toBe("Ready");
  expect(chipLabel({ state: "not_logged_in" })).toBe("Not logged in");
  expect(chipLabel({ state: "not_installed" })).toBe("Not installed");
  expect(JSON.stringify({ state: "ready", detail: "login detected" })).not.toMatch(
    /token|secret|password/i,
  );
});
