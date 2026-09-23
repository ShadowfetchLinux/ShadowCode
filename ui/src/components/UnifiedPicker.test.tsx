import { useState } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { UnifiedPicker } from "./UnifiedPicker";
import type { PickerTarget } from "../lib/picker";

afterEach(() => cleanup());
beforeEach(() => localStorage.clear());

const now = Date.now() / 1000;
const codex: PickerTarget = {
  id: "cli:codex:gpt-6-astra",
  provider: "cli:codex",
  group: "subscriptions",
  name: "Codex · GPT-6-Astra",
  inference: "cloud",
  availability: "ready",
  availability_label: "Ready",
  vision: true,
  tools: true,
  is_default: true,
  usage: {
    state: "ok",
    label: "Shared plan usage · 2% left",
    windows: [
      {
        label: "Weekly",
        used_percent: 98,
        remaining_percent: 2,
        window_minutes: 10080,
        resets_at: now + 2 * 3600 + 120,
      },
    ],
    last_refresh: now - 120,
    provider_usage_url: "https://chatgpt.com/codex/settings/usage",
  },
};
const cursor: PickerTarget = {
  id: "cli:cursor:auto",
  provider: "cli:cursor",
  group: "subscriptions",
  name: "Cursor · Auto",
  inference: "cloud",
  availability: "sign_in",
  availability_label: "Sign in",
  reason: "Not signed in",
  vision: false,
  tools: true,
  usage: {
    state: "unavailable",
    label: "Usage unavailable · Open provider usage",
  },
};
const grok: PickerTarget = {
  ...cursor,
  id: "cli:grok",
  provider: "cli:grok",
  name: "Grok · Default",
  featured: false,
  availability: "unavailable",
  availability_label: "Unavailable",
  reason: "Offline mode: cloud rows are off",
};
const qwen: PickerTarget = {
  id: "local:gguf:1",
  provider: "llamacpp",
  group: "local",
  name: "qwen3:14b · This computer",
  inference: "local",
  availability: "ready",
  availability_label: "Ready",
  vision: false,
  tools: true,
  usage: {
    state: "local",
    label: "Runs on this computer · No subscription quota",
  },
};
const gptoss: PickerTarget = {
  ...qwen,
  id: "local:gguf:2",
  name: "gpt-oss:20b · This computer",
  availability: "setup_required",
  availability_label: "Setup required",
  reason: "unknown model architecture: gptoss",
  tools: false,
};

function Harness({
  targets = [codex, cursor, grok, qwen, gptoss],
  initial = "",
  onConnect = vi.fn(),
  onSetup = vi.fn(),
  onSelect = vi.fn(),
}: {
  targets?: PickerTarget[];
  initial?: string;
  onConnect?: (vendor?: string) => void;
  onSetup?: (target: PickerTarget) => void;
  onSelect?: (id: string) => void;
}) {
  const [value, setValue] = useState(initial);
  const [open, setOpen] = useState(false);
  return (
    <UnifiedPicker
      targets={targets}
      value={value}
      open={open}
      onOpenChange={setOpen}
      onSelect={(id) => {
        setValue(id);
        onSelect(id);
      }}
      onConnect={onConnect}
      onSetup={onSetup}
      onAddLocal={vi.fn()}
    />
  );
}

const trigger = () =>
  screen.getByRole("button", { name: /Model for this task/ });
const search = () => screen.getByRole("combobox", { name: "Search models" });
const key = (k: string) => fireEvent.keyDown(search(), { key: k });
const activeOption = () => {
  const id = search().getAttribute("aria-activedescendant");
  return id ? document.getElementById(id) : null;
};

it("shows 'Choose a model' until a row is chosen and groups both sources", () => {
  render(<Harness />);
  expect(trigger().textContent).toContain("Choose a model");
  fireEvent.click(trigger());
  const subs = screen.getByRole("group", { name: "Subscriptions" });
  const local = screen.getByRole("group", { name: "On this computer" });
  expect(
    within(subs).getByRole("option", { name: /Codex · GPT-6-Astra/ }),
  ).toBeTruthy();
  // featured:false still lands in Subscriptions.
  expect(
    within(subs).getByRole("option", { name: /Grok · Default/ }),
  ).toBeTruthy();
  expect(
    within(local).getByRole("option", { name: /qwen3:14b · This computer/ }),
  ).toBeTruthy();
  // Each row: Local/Cloud, availability, usage.
  const text = (name: RegExp) =>
    screen.getByRole("option", { name }).textContent || "";
  expect(text(/Codex/)).toMatch(/Cloud · Ready · Shared plan usage · 2% left/);
  expect(text(/qwen3/)).toMatch(
    /Local · Ready · Runs on this computer · No subscription quota/,
  );
  expect(text(/Cursor/)).toMatch(
    /Cloud · Sign in · Usage unavailable · Open provider usage/,
  );
  expect(text(/gpt-oss/)).toMatch(/Local · Setup required/);
  // Capability badges only when verified.
  expect(screen.getAllByText("Vision")).toHaveLength(1);
  expect(screen.getAllByText("Chat only")).toHaveLength(1);
  // Non-ready rows are options, not disabled buttons.
  const blocked = screen.getByRole("option", { name: /Cursor · Auto/ });
  expect(blocked.tagName).not.toBe("BUTTON");
  expect(document.body.textContent).not.toMatch(/72% remaining|\$12,430/);
});

it("selects with the keyboard and restores focus to the trigger", async () => {
  const onSelect = vi.fn();
  render(<Harness onSelect={onSelect} />);
  fireEvent.click(trigger());
  await waitFor(() => expect(document.activeElement).toBe(search()));
  expect(activeOption()?.textContent).toContain("Codex · GPT-6-Astra");
  key("End");
  expect(activeOption()?.textContent).toContain("gpt-oss:20b");
  key("Home");
  expect(activeOption()?.textContent).toContain("Codex");
  key("ArrowDown");
  key("ArrowDown");
  key("ArrowDown");
  expect(activeOption()?.textContent).toContain("qwen3:14b");
  key("ArrowUp");
  key("ArrowDown");
  key("Enter");
  expect(onSelect).toHaveBeenCalledWith("local:gguf:1");
  expect(screen.queryByRole("listbox")).toBeNull();
  await waitFor(() => expect(document.activeElement).toBe(trigger()));
  expect(trigger().textContent).toMatch(/^Local/);
  expect(trigger().textContent).toContain("qwen3:14b · This computer");
});

it("filters by typing and closes with Escape", async () => {
  render(<Harness />);
  fireEvent.click(trigger());
  fireEvent.change(search(), { target: { value: "qwen" } });
  expect(screen.getAllByRole("option")).toHaveLength(1);
  expect(screen.getByText("No subscription matches")).toBeTruthy();
  key("Escape");
  expect(screen.queryByRole("listbox")).toBeNull();
  await waitFor(() => expect(document.activeElement).toBe(trigger()));
});

it("routes non-ready rows to their fix instead of disabling them", () => {
  const onConnect = vi.fn();
  const onSetup = vi.fn();
  render(<Harness onConnect={onConnect} onSetup={onSetup} />);
  fireEvent.click(trigger());
  // Sign in → Accounts › Connect for that vendor.
  fireEvent.click(screen.getByRole("option", { name: /Cursor · Auto/ }));
  expect(onConnect).toHaveBeenCalledWith("cursor");

  fireEvent.click(trigger());
  // Unavailable → the reason is shown.
  fireEvent.click(screen.getByRole("option", { name: /Grok · Default/ }));
  expect(
    screen.getAllByText(/Offline mode: cloud rows are off/).length,
  ).toBeGreaterThan(0);
  // Setup required → the setup hint, then the place that fixes it.
  const setup = screen.getByRole("option", { name: /gpt-oss:20b/ });
  fireEvent.click(setup);
  expect(
    screen.getAllByText(/unknown model architecture: gptoss/).length,
  ).toBeGreaterThan(0);
  fireEvent.click(screen.getByRole("button", { name: "Open Local models" }));
  expect(onSetup).toHaveBeenCalledWith(
    expect.objectContaining({ id: "local:gguf:2" }),
  );
});

it("opens usage details from the keyboard with windows and reset times", () => {
  render(<Harness />);
  fireEvent.click(trigger());
  key("ArrowRight");
  const details = document.querySelector(".unified-picker-details")!;
  expect(details.textContent).toContain("Weekly · 2% left · resets in 2h");
  expect(details.textContent).toContain("Last checked 2m ago");
  expect(
    within(details as HTMLElement).getByRole("link", {
      name: "Open Codex usage",
    }),
  ).toBeTruthy();
  expect(activeOption()?.getAttribute("aria-describedby")).toContain("details");
  key("ArrowLeft");
  expect(document.querySelector(".unified-picker-details")).toBeNull();
});

it("collapses a vendor with many models and expands on request", () => {
  const many = Array.from({ length: 40 }, (_, i) => ({
    ...codex,
    id: `cli:cursor:m${i}`,
    provider: "cli:cursor",
    name: i === 0 ? "Cursor · Auto" : `Cursor · Model ${i}`,
    is_default: i === 0,
  }));
  render(<Harness targets={many} />);
  fireEvent.click(trigger());
  expect(screen.getAllByRole("option", { name: /^Cursor/ })).toHaveLength(1);
  const more = screen.getByRole("option", {
    name: "Show all 40 Cursor models",
  });
  fireEvent.click(more);
  expect(screen.getAllByRole("option", { name: /^Cursor/ })).toHaveLength(40);
});
