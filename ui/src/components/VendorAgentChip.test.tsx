import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { VendorAgentChip } from "./VendorAgentChip";

afterEach(() => cleanup());

it("shows doctor states for the three vendor CLIs", () => {
  render(
    <VendorAgentChip
      selected="cli:codex"
      status={{
        codex: { state: "ready", detail: "Ready: fake; login detected" },
        grok: { state: "not_installed", detail: "Not installed" },
        claude: { state: "not_logged_in", detail: "Installed but not logged in" },
      }}
    />,
  );
  const chip = screen.getByTestId("vendor-agent-chip");
  expect(chip.textContent).toContain("Codex · Ready");
  expect(chip.textContent).toContain("Grok · Not installed");
  expect(chip.textContent).toContain("Claude · Not logged in");
  expect(chip.textContent).not.toMatch(/token|secret|password|auth\.json/i);
  expect(chip.querySelector('[data-vendor="codex"]')?.className).toContain(
    "selected",
  );
});
