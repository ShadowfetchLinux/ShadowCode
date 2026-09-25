import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { PermissionsPage, noteSummary } from "./PreferencePages";

afterEach(cleanup);

it("puts each runner's rules behind a disclosure and never shows raw keys", () => {
  const { container } = render(
    <PermissionsPage
      cfg={{
        permissions: {
          mode: "ask",
          vendor_notes: {
            network: "Network limits apply to ShadowCode's own tools only.",
            codex: "Codex runs in its own sandbox.",
            native: "Ask before actions: enforced by ShadowCode.",
          },
        },
        network: { mode: "online" },
      }}
      onSave={vi.fn()}
    />,
  );
  const summaries = [...container.querySelectorAll("details > summary")]
    .map((s) => s.textContent)
    .filter((t) => t?.startsWith("How"));
  expect(summaries).toEqual([
    "How ShadowCode applies this (local, API and OpenRouter models)",
    "How Codex applies this",
    "How the network setting applies to subscriptions",
  ]);
  // Closed by default: the prose is there for whoever opens it.
  const codex = screen.getByText("Codex runs in its own sandbox.");
  expect(codex.closest("details")?.open).toBe(false);
  expect(container.textContent).not.toMatch(/\bnative\b/);
});

it("names subscription runners in words", () => {
  expect(noteSummary("cli:claude")).toBe("How Claude Code applies this");
  expect(noteSummary("antigravity")).toBe("How Antigravity applies this");
  expect(noteSummary("unknown")).toBe("How this subscription applies this");
});
