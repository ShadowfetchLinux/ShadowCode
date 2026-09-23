import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ModelChooser } from "./ModelChooser";
import type { ModelInfo } from "../api";

afterEach(() => cleanup());

const models: ModelInfo[] = [
  { id: "local-1", name: "qwen2.5", provider: "ollama", endpoint: "http://127.0.0.1:11434/v1" },
  {
    id: "cli:codex",
    name: "Codex (vendor agent)",
    provider: "cli:codex",
    endpoint: "",
    metadata: { vendor_agent: true, label: "Codex (vendor agent)" },
  },
  {
    id: "cli:claude",
    name: "Claude (vendor agent)",
    provider: "cli:claude",
    endpoint: "",
    metadata: { vendor_agent: true, label: "Claude (vendor agent)" },
  },
];

it("keeps vendor CLI agents in their own picker group", () => {
  const onChange = vi.fn();
  render(
    <ModelChooser
      models={models}
      value=""
      automaticLabel="Choose model"
      onChange={onChange}
    />,
  );
  expect(screen.getByLabelText("Model for this task")).toBeTruthy();
  expect(
    screen.getByRole("group", { name: "Local model (ShadowCode agent)" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("group", {
      name: "Claude / Codex / Grok (vendor agent)",
    }),
  ).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Model for this task"), {
    target: { value: "cli:codex" },
  });
  expect(onChange).toHaveBeenCalledWith("cli:codex");
});
