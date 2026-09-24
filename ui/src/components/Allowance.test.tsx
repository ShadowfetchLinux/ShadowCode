import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AllowanceButton, AllowancePanel, PlanLimitControl } from "./Allowance";
import type { AllowanceResponse } from "../api";
import type { PickerTarget } from "../lib/picker";
import { allowanceLevel, resolveFallback } from "../lib/allowance";

afterEach(() => cleanup());

const NOW = 1_000_000;

const data: AllowanceResponse = {
  generated_at: NOW,
  rows: [
    {
      id: "cli:codex",
      kind: "subscription",
      product: "Codex",
      state: "low",
      headline: "2% left",
      remaining_percent: 2,
      windows: [
        { label: "Weekly", remaining_percent: 2, resets_at: NOW + 3 * 3600 },
      ],
      plan: "pro",
      note: null,
      last_checked: NOW - 90,
      usage_url: "https://chatgpt.com/codex/settings/usage",
    },
    {
      id: "cli:claude",
      kind: "subscription",
      product: "Claude Code",
      state: "sign_in",
      headline: "Not signed in",
      remaining_percent: null,
      windows: [],
      plan: null,
      note: "Claude Code does not report plan usage.",
      last_checked: null,
      usage_url: null,
    },
    {
      id: "cli:cursor",
      kind: "subscription",
      product: "Cursor",
      state: "limit_reached",
      headline: "Plan limit reached",
      remaining_percent: 0,
      windows: [
        { label: "Monthly", remaining_percent: 0, resets_at: NOW + 2 * 3600 },
      ],
      plan: "Pro",
      note: null,
      last_checked: NOW - 300,
      usage_url: "https://cursor.com/dashboard?tab=usage",
    },
    {
      id: "cli:antigravity",
      kind: "subscription",
      product: "Antigravity",
      state: "not_installed",
      headline: "Not installed",
      remaining_percent: null,
      windows: [],
      plan: null,
      note: null,
      last_checked: null,
      usage_url: null,
    },
    {
      id: "openrouter",
      kind: "api_key",
      product: "OpenRouter",
      state: "ok",
      headline: "$8.77 of $10.00 left",
      remaining_percent: 87.655,
      used: 1.2345,
      limit: 10,
      limit_remaining: 8.7655,
      usage_url: "https://openrouter.ai/activity",
    },
    {
      id: "local",
      kind: "local",
      product: "On this computer",
      state: "ok",
      headline: "No quota · 1 model ready",
      remaining_percent: null,
      ready_models: 1,
      on_limit: "local",
      fallback: { id: "local:gguf:qwen", name: "qwen3:14b" },
    },
  ],
};

const local = (id: string, name: string, tools = true): PickerTarget => ({
  id,
  provider: "llamacpp",
  group: "local",
  name: `${name} · This computer`,
  inference: "local",
  availability: "ready",
  tools,
});

function panel(overrides: Partial<Parameters<typeof AllowancePanel>[0]> = {}) {
  const props = {
    data,
    loading: false,
    now: NOW,
    onRefresh: vi.fn(),
    onClose: vi.fn(),
    onOpenAccounts: vi.fn(),
    onOpenLocal: vi.fn(),
    ...overrides,
  };
  render(<AllowancePanel {...props} />);
  return props;
}

describe("Allowance panel", () => {
  it("lists every source with its headline, meter, resets and last check", () => {
    panel({ planLimit: <p>plan control</p> });
    const dialog = screen.getByRole("dialog", { name: "Allowance" });
    expect(within(dialog).getAllByRole("listitem").length).toBeGreaterThan(5);
    const codex = screen.getByRole("article", { name: "Codex" });
    expect(codex.textContent).toContain("2% left");
    const meter = within(codex).getByRole("meter", { name: "Codex remaining" });
    expect(meter.getAttribute("aria-valuenow")).toBe("2");
    expect(meter.getAttribute("aria-valuetext")).toBe("2% left");
    expect(codex.textContent).toContain("Weekly · 2% left · resets in 3h");
    expect(codex.textContent).toContain("Pro plan");
    expect(codex.textContent).toContain("Last checked 1m ago");
    expect(
      within(codex)
        .getByRole("link", { name: "Open usage page for Codex" })
        .getAttribute("href"),
    ).toBe("https://chatgpt.com/codex/settings/usage");
    expect(codex.className).toContain("tone-warn");

    const cursor = screen.getByRole("article", { name: "Cursor" });
    expect(cursor.textContent).toContain("Plan limit reached");
    expect(
      within(cursor).getByRole("meter").getAttribute("aria-valuenow"),
    ).toBe("0");

    // No reported percentage, no meter.
    const claude = screen.getByRole("article", { name: "Claude Code" });
    expect(within(claude).queryByRole("meter")).toBeNull();
    expect(claude.textContent).toContain(
      "Claude Code does not report plan usage.",
    );

    const openrouter = screen.getByRole("article", { name: "OpenRouter" });
    expect(openrouter.textContent).toContain("$8.77 of $10.00 left");
    expect(openrouter.textContent).toContain("$1.23 used on this key");
    expect(
      within(openrouter).getByRole("meter").getAttribute("aria-valuenow"),
    ).toBe("88");

    // The plan-limit control sits on the local row only.
    const localRow = screen.getByRole("article", { name: "On this computer" });
    expect(localRow.textContent).toContain("plan control");
    expect(codex.textContent).not.toContain("plan control");
  });

  it("leads rows that need setup to Accounts and refreshes on request", () => {
    const props = panel({
      data: {
        ...data,
        rows: data.rows.map((row) =>
          row.id === "openrouter"
            ? {
                ...row,
                state: "no_key",
                headline: "No API key",
                remaining_percent: null,
                used: null,
                limit: null,
              }
            : row.id === "local"
              ? { ...row, state: "none", headline: "No local model ready" }
              : row,
        ),
      },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Open Accounts for Claude Code" }),
    );
    expect(props.onOpenAccounts).toHaveBeenLastCalledWith("claude");
    fireEvent.click(
      screen.getByRole("button", { name: "Open Accounts for Antigravity" }),
    );
    expect(props.onOpenAccounts).toHaveBeenLastCalledWith("antigravity");
    // A ready subscription needs no Accounts action.
    expect(
      screen.queryByRole("button", { name: "Open Accounts for Codex" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Add a key/ }));
    expect(props.onOpenAccounts).toHaveBeenLastCalledWith("openrouter");
    fireEvent.click(screen.getByRole("button", { name: "Open Local models" }));
    expect(props.onOpenLocal).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(props.onRefresh).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(screen.getByRole("button", { name: "Refresh" }), {
      key: "Escape",
    });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("shows checking states and errors", () => {
    panel({ data: null, loading: true });
    expect(screen.getByRole("status").textContent).toBe("Checking allowance…");
    expect(
      (screen.getByRole("button", { name: "Checking…" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    cleanup();
    panel({ data: null, error: "Engine offline" });
    expect(screen.getByRole("alert").textContent).toBe("Engine offline");
  });
});

describe("Allowance status-bar button", () => {
  it("colours its dot by the worst ready source", () => {
    const { container, rerender } = render(
      <AllowanceButton data={data} open={false} onOpen={vi.fn()} />,
    );
    const button = screen.getByRole("button", { name: /^Allowance/ });
    expect(container.querySelector(".allowance-dot")?.className).toContain(
      "warn",
    );
    expect(button.textContent).toContain("Codex: 2% left");
    expect(button.textContent).toContain("Cursor: plan limit reached");
    const calm: AllowanceResponse = {
      ...data,
      rows: data.rows.filter(
        (row) => !["cli:codex", "cli:cursor"].includes(row.id),
      ),
    };
    rerender(<AllowanceButton data={calm} open={false} onOpen={vi.fn()} />);
    expect(container.querySelector(".allowance-dot")?.className).toContain(
      "ok",
    );
    expect(allowanceLevel(undefined)).toBe("none");
    // Rows that need setup never colour the dot.
    expect(
      allowanceLevel(
        data.rows.filter((row) =>
          ["cli:claude", "cli:antigravity"].includes(row.id),
        ),
      ),
    ).toBe("none");
  });
});

describe("When a plan runs out", () => {
  const targets = [
    local("local:gguf:qwen", "qwen3:14b"),
    local("local:gguf:gemma", "gemma-4:12b"),
  ];

  it("saves on_limit and the fallback model", async () => {
    const onSave = vi.fn(async () => undefined);
    render(
      <PlanLimitControl
        limits={{ on_limit: "local", fallback_model: "" }}
        localTargets={targets}
        automaticName="qwen3:14b"
        onSave={onSave}
      />,
    );
    const group = screen.getByRole("group", { name: "When a plan runs out" });
    const cont = within(group).getByRole("radio", {
      name: /Continue on qwen3:14b/,
    }) as HTMLInputElement;
    expect(cont.checked).toBe(true);
    const select = within(group).getByRole("combobox", {
      name: "Local model",
    }) as HTMLSelectElement;
    expect(select.value).toBe("");
    expect(select.options[0].textContent).toBe("Automatic (qwen3:14b)");
    fireEvent.change(select, { target: { value: "local:gguf:gemma" } });
    expect(onSave).toHaveBeenLastCalledWith({
      fallback_model: "local:gguf:gemma",
    });
    await waitFor(() =>
      expect(
        within(group).getByRole("radio", { name: /Continue on gemma-4:12b/ }),
      ).toBeTruthy(),
    );
    fireEvent.click(within(group).getByRole("radio", { name: /Ask me/ }));
    expect(onSave).toHaveBeenLastCalledWith({ on_limit: "ask" });
    // Asking needs no model choice.
    await waitFor(() =>
      expect(within(group).queryByRole("combobox")).toBeNull(),
    );
  });

  it("follows values saved elsewhere and reverts a failed save", async () => {
    const onSave = vi.fn(async () => {
      throw new Error("limits.on_limit must be local or ask");
    });
    const { rerender } = render(
      <PlanLimitControl
        limits={{ on_limit: "local", fallback_model: "" }}
        localTargets={targets}
        onSave={onSave}
      />,
    );
    const ask = screen.getByRole("radio", {
      name: /Ask me/,
    }) as HTMLInputElement;
    fireEvent.click(ask);
    await waitFor(() => expect(ask.checked).toBe(false));
    rerender(
      <PlanLimitControl
        limits={{ on_limit: "ask", fallback_model: "" }}
        localTargets={targets}
        onSave={onSave}
      />,
    );
    expect(
      (screen.getByRole("radio", { name: /Ask me/ }) as HTMLInputElement)
        .checked,
    ).toBe(true);
  });

  it("points to Local models when nothing can run here", () => {
    const onOpenLocal = vi.fn();
    render(
      <PlanLimitControl
        limits={{ on_limit: "local", fallback_model: "" }}
        localTargets={[]}
        onSave={vi.fn(async () => undefined)}
        onOpenLocal={onOpenLocal}
      />,
    );
    expect(
      screen.getByRole("radio", { name: /Continue on a local model/ }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open Local models" }));
    expect(onOpenLocal).toHaveBeenCalled();
  });

  it("resolves the fallback: saved choice, engine pick, then first with tools", () => {
    expect(
      resolveFallback({ fallback_model: "local:gguf:gemma" }, data, targets),
    ).toEqual({ id: "local:gguf:gemma", name: "gemma-4:12b" });
    expect(resolveFallback({ fallback_model: "" }, data, targets)).toEqual({
      id: "local:gguf:qwen",
      name: "qwen3:14b",
    });
    expect(
      resolveFallback({}, null, [
        local("local:gguf:a", "a", false),
        local("local:gguf:b", "b"),
      ]),
    ).toEqual({ id: "local:gguf:b", name: "b" });
    expect(resolveFallback({}, null, [])).toBeNull();
  });
});
