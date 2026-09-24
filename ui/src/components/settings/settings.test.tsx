import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installFakeBackend } from "../../../e2e/fakeBackend";
import { AccountsPage, LoginLine } from "./AccountsPage";
import { LocalModelsPage } from "./LocalModelsPage";
import { PermissionsPage } from "./PreferencePages";

let fake: ReturnType<typeof installFakeBackend>;
beforeEach(() => {
  vi.stubEnv("VITE_SHADOW_TEST_TRANSPORT", "1");
  fake = installFakeBackend({ stepMs: 5 });
});
afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
  delete window.__SHADOW_TEST_TRANSPORT__;
});

it("Accounts: status, usage windows, models and Connect for signed-out vendors", async () => {
  const onChanged = vi.fn();
  render(<AccountsPage onChanged={onChanged} onToast={vi.fn()} />);
  const codex = await screen.findByRole("article", { name: "Codex" });
  expect(within(codex).getByText("Ready")).toBeTruthy();
  expect(within(codex).getByText("codex-cli 0.155.0")).toBeTruthy();
  expect(within(codex).getByText("dev@example.com · Pro plan")).toBeTruthy();
  expect(within(codex).getByText(/Weekly · 2% left · resets in/)).toBeTruthy();
  expect(within(codex).getByText(/Last checked/)).toBeTruthy();
  expect(
    within(codex).getByRole("link", { name: "Open Codex usage" }),
  ).toBeTruthy();
  expect(within(codex).getByText("2 models available")).toBeTruthy();
  expect(within(codex).queryByRole("button", { name: "Connect" })).toBeNull();
  expect(
    within(codex).getByRole("button", { name: "Disconnect" }),
  ).toBeTruthy();

  const claude = screen.getByRole("article", { name: "Claude Code" });
  expect(within(claude).getByText("Sign in")).toBeTruthy();
  expect(
    within(claude).queryByRole("button", { name: "Disconnect" }),
  ).toBeNull();
  fireEvent.click(within(claude).getByRole("button", { name: "Connect" }));
  // Login lines stream in with links and a selectable device code.
  const link = await within(claude).findByRole(
    "link",
    { name: /claude\.ai\/oauth/ },
    { timeout: 4000 },
  );
  expect(link.getAttribute("href")).toContain(
    "https://claude.ai/oauth/authorize",
  );
  await waitFor(
    () => expect(within(claude).getByText("WXYZ-1234").tagName).toBe("CODE"),
    { timeout: 4000 },
  );
  await waitFor(
    () => expect(within(claude).getByText("Signed in.")).toBeTruthy(),
    {
      timeout: 5000,
    },
  );
  await waitFor(() => expect(within(claude).getByText("Ready")).toBeTruthy());
  expect(onChanged).toHaveBeenCalled();
});

it("Accounts: Disconnect asks first and shows the shared CLI note", async () => {
  render(<AccountsPage onChanged={vi.fn()} onToast={vi.fn()} />);
  const codex = await screen.findByRole("article", { name: "Codex" });
  fireEvent.click(within(codex).getByRole("button", { name: "Disconnect" }));
  const dialog = screen.getByRole("dialog", { name: "Disconnect Codex" });
  expect(
    within(dialog).getByText(
      /signs out the codex CLI for your whole user account/,
    ),
  ).toBeTruthy();
  expect(fake.log.some((r) => r.path.endsWith("/disconnect"))).toBe(false);
  fireEvent.click(within(dialog).getByRole("button", { name: "Disconnect" }));
  await waitFor(() =>
    expect(
      fake.log.find((r) => r.path === "/api/accounts/codex/disconnect")?.body,
    ).toEqual({ confirm: true }),
  );
  await waitFor(() => expect(within(codex).getByText("Sign in")).toBeTruthy());
});

it("renders login output links and device codes safely", () => {
  render(
    <p>
      <LoginLine line="Visit https://example.com/device and enter ABCD-EFGH now" />
    </p>,
  );
  expect(screen.getByRole("link").getAttribute("href")).toBe(
    "https://example.com/device",
  );
  expect(screen.getByText("ABCD-EFGH").tagName).toBe("CODE");
});

it("Local models: runtime, hardware, compatibility, memory and actions", async () => {
  const onChanged = vi.fn();
  render(<LocalModelsPage onChanged={onChanged} onToast={vi.fn()} />);
  expect(await screen.findByText(/Ready · Vulkan · b6500/)).toBeTruthy();
  expect(
    screen.getByText(
      /16 CPU threads · 62 GB RAM · NVIDIA GeForce RTX 5060 Ti \(16 GB\)/,
    ),
  ).toBeTruthy();
  expect(screen.getByText("No model loaded")).toBeTruthy();
  const qwen = screen.getByRole("article", { name: "qwen3:14b" });
  expect(within(qwen).getByText("Compatible")).toBeTruthy();
  expect(within(qwen).getByText(/Needs about 11 GB/)).toBeTruthy();
  expect(within(qwen).getByText(/fits in GPU memory/)).toBeTruthy();
  const gptoss = screen.getByRole("article", { name: "gpt-oss:20b" });
  expect(within(gptoss).getByText("Not compatible")).toBeTruthy();
  expect(
    within(gptoss).getByText(/unknown model architecture: gptoss/),
  ).toBeTruthy();
  expect(within(gptoss).getByText("Chat only")).toBeTruthy();
  expect(within(gptoss).getByRole("button", { name: "Load" })).toHaveProperty(
    "disabled",
    true,
  );

  fireEvent.click(within(qwen).getByRole("button", { name: "Load" }));
  await waitFor(() =>
    expect(screen.getByText(/qwen3:14b · Vulkan0/)).toBeTruthy(),
  );
  expect(within(qwen).getByText("Loaded")).toBeTruthy();
  expect(onChanged).toHaveBeenCalled();

  // Ollama store: already-added rows are marked, others import in place.
  expect(screen.getByText("Added")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Import" }));
  expect(
    await screen.findByRole("article", { name: "gemma-4:12b" }),
  ).toBeTruthy();
  fireEvent.click(
    within(screen.getByRole("article", { name: "gemma-4:12b" })).getByRole(
      "button",
      {
        name: "Remove",
      },
    ),
  );
  await waitFor(() =>
    expect(screen.queryByRole("article", { name: "gemma-4:12b" })).toBeNull(),
  );
});

it("Permissions: saves only permissions and network groups", async () => {
  const onSave = vi.fn(async () => undefined);
  render(
    <PermissionsPage
      cfg={{
        permissions: {
          mode: "ask",
          level: "workspace",
          vendor_notes: { codex: "Codex sandbox note" },
        },
        network: { mode: "online" },
        ui: { theme: "dark" },
      }}
      onSave={onSave}
    />,
  );
  expect(screen.getByText("Codex sandbox note")).toBeTruthy();
  fireEvent.click(screen.getByLabelText(/Allow project edits/));
  fireEvent.click(screen.getByLabelText(/^Offline/));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(onSave).toHaveBeenCalled());
  const values = (onSave.mock.calls[0] as unknown[])[0] as Record<
    string,
    unknown
  >;
  expect(Object.keys(values).sort()).toEqual(["network", "permissions"]);
  expect(values).toMatchObject({
    permissions: { mode: "allow_edits", level: "workspace" },
    network: { mode: "offline" },
  });
});

it("Accounts: OpenRouter key is validated, saved, never shown and removable", async () => {
  const onChanged = vi.fn();
  render(<AccountsPage onChanged={onChanged} onToast={vi.fn()} />);
  const card = await screen.findByRole("article", { name: "OpenRouter" });
  expect(within(card).getByText("API key")).toBeTruthy();
  await within(card).findByText("Add API key");
  expect(
    within(card)
      .getByRole("link", { name: "openrouter.ai/keys" })
      .getAttribute("href"),
  ).toBe("https://openrouter.ai/keys");
  const input = within(card).getByLabelText(
    "OpenRouter API key",
  ) as HTMLInputElement;
  expect(input.type).toBe("password");
  expect(input.getAttribute("autocomplete")).toBe("off");
  const save = within(card).getByRole("button", { name: "Save" });
  expect(save).toHaveProperty("disabled", true);

  // A rejected key: the engine's reason inline, nothing saved.
  fireEvent.change(input, { target: { value: "sk-or-wrong" } });
  fireEvent.click(save);
  const alert = await within(card).findByRole("alert");
  expect(alert.textContent).toBe("OpenRouter rejected this key (401)");
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(onChanged).not.toHaveBeenCalled();

  fireEvent.change(input, { target: { value: "sk-or-valid" } });
  expect(within(card).queryByRole("alert")).toBeNull();
  fireEvent.click(within(card).getByRole("button", { name: "Save" }));
  await within(card).findByText("Key: sk-or-v1-a1b…9f2");
  expect(
    fake.log.find(
      (r) =>
        r.path === "/api/openrouter/key" && r.body.api_key === "sk-or-valid",
    ),
  ).toBeTruthy();
  expect(onChanged).toHaveBeenCalledTimes(1);
  // The field is gone and the key appears nowhere on the page.
  expect(within(card).queryByLabelText("OpenRouter API key")).toBeNull();
  expect(document.body.innerHTML).not.toContain("sk-or-valid");
  expect(within(card).getByText("Ready")).toBeTruthy();
  expect(
    within(card).getByText("Credits used: $1.23 of $10.00 limit ($8.77 left)"),
  ).toBeTruthy();
  expect(within(card).getByText(/40 models \(30 support tools\)/)).toBeTruthy();
  expect(within(card).getByText("Billed per token by OpenRouter")).toBeTruthy();
  expect(
    within(card)
      .getByRole("link", { name: "Open OpenRouter activity" })
      .getAttribute("href"),
  ).toBe("https://openrouter.ai/activity");
  await waitFor(() =>
    expect(document.activeElement?.textContent).toBe("Refresh models"),
  );

  fireEvent.click(within(card).getByRole("button", { name: "Refresh models" }));
  await waitFor(() =>
    expect(fake.log.some((r) => r.path === "/api/openrouter/refresh")).toBe(
      true,
    ),
  );
  await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(2));

  // Remove asks first, like Disconnect.
  fireEvent.click(within(card).getByRole("button", { name: "Remove key" }));
  const dialog = screen.getByRole("dialog", { name: "Remove OpenRouter key" });
  expect(
    fake.log.some(
      (r) => r.path === "/api/openrouter/key" && r.body.api_key === "",
    ),
  ).toBe(false);
  fireEvent.click(within(dialog).getByRole("button", { name: "Remove key" }));
  const again = await within(card).findByLabelText("OpenRouter API key");
  expect((again as HTMLInputElement).value).toBe("");
  expect(
    fake.log.some(
      (r) => r.path === "/api/openrouter/key" && r.body.api_key === "",
    ),
  ).toBe(true);
  expect(onChanged).toHaveBeenCalledTimes(3);
});

it("Accounts: opening for OpenRouter focuses the key field; offline disables it", async () => {
  const { unmount } = render(
    <AccountsPage
      focusVendor="openrouter"
      onChanged={vi.fn()}
      onToast={vi.fn()}
    />,
  );
  const input = await screen.findByLabelText("OpenRouter API key");
  await waitFor(() => expect(document.activeElement).toBe(input));
  unmount();

  fake.state.config.network.mode = "offline";
  render(<AccountsPage onChanged={vi.fn()} onToast={vi.fn()} />);
  const card = await screen.findByRole("article", { name: "OpenRouter" });
  await within(card).findByText("Offline mode: OpenRouter is off.");
  expect(within(card).getByLabelText("OpenRouter API key")).toHaveProperty(
    "disabled",
    true,
  );
  expect(within(card).getByRole("button", { name: "Save" })).toHaveProperty(
    "disabled",
    true,
  );
});
