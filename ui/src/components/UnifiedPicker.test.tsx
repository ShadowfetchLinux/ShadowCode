import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { UnifiedPicker } from "./UnifiedPicker";
import type { PickerTarget } from "../lib/picker";

afterEach(() => cleanup());

const targets: PickerTarget[] = [
  {
    id: "cli:codex",
    provider: "cli:codex",
    group: "subscriptions",
    name: "Codex · Auto",
    inference: "cloud",
    availability: "ready",
    featured: true,
    usage: { state: "unavailable", label: "Usage unavailable · Open Codex usage" },
  },
  {
    id: "cli:cursor:auto",
    provider: "cli:cursor",
    group: "subscriptions",
    name: "Cursor · Auto",
    inference: "cloud",
    availability: "sign_in",
    featured: true,
    usage: { state: "unavailable" },
  },
  {
    id: "local:gguf:1",
    provider: "llamacpp",
    group: "local",
    name: "qwen · This computer",
    inference: "local",
    availability: "setup_required",
    featured: true,
    usage: { state: "local" },
  },
];

it("opens one searchable menu with both groups and no fabricated quota", () => {
  const onChange = vi.fn();
  render(
    <UnifiedPicker
      targets={targets}
      value="cli:codex"
      automaticLabel="Choose a model"
      onChange={onChange}
      onConnect={vi.fn()}
      onAddLocal={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByLabelText("Model for this task"));
  expect(screen.getByRole("group", { name: "Subscriptions (use your account)" })).toBeTruthy();
  expect(screen.getByRole("group", { name: "On this computer" })).toBeTruthy();
  expect(screen.getAllByText("Codex · Auto").length).toBeGreaterThan(0);
  expect(screen.getByText("Usage unavailable · Open Codex usage")).toBeTruthy();
  expect(document.body.textContent).not.toMatch(/72% remaining|\$12,430|38 passed/);
  expect(screen.getByRole("option", { name: /Cursor · Auto/i })).toHaveProperty("disabled", true);
  fireEvent.click(screen.getByText("Connect account…"));
});
