import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import { installFakeBackend } from "../e2e/fakeBackend";

let fake: ReturnType<typeof installFakeBackend>;

beforeEach(() => {
  localStorage.clear();
  vi.stubEnv("VITE_SHADOW_TEST_TRANSPORT", "1");
  fake = installFakeBackend({ stepMs: 5 });
});
afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
  delete window.__SHADOW_TEST_TRANSPORT__;
});

const trigger = () => screen.getByRole("button", { name: /Model for this task/ });
const send = () => screen.getByRole("button", { name: /Send task|Queue follow-up/ });
const prompt = () => screen.getByRole("textbox", { name: "Message ShadowCode" });

async function boot() {
  render(<App />);
  await screen.findByRole("textbox", { name: "Message ShadowCode" });
  await waitFor(() => expect(trigger().textContent).toContain("Choose a model"));
}

async function choose(name: RegExp) {
  fireEvent.click(trigger());
  fireEvent.click(await screen.findByRole("option", { name }));
  await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
}

it("disables Send until a ready row is chosen and remembers it per conversation", async () => {
  await boot();
  fireEvent.change(prompt(), { target: { value: "Fix the add function" } });
  expect(send()).toHaveProperty("disabled", true);
  expect(screen.getByText("Choose a model to send.")).toBeTruthy();
  fireEvent.click(trigger());
  const local = screen.getByRole("group", { name: "On this computer" });
  expect(
    within(local).getByRole("option", { name: /qwen3:14b · This computer/ }),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("option", { name: /Codex · GPT-6-Astra/ }));
  await waitFor(() => expect(send()).toHaveProperty("disabled", false));
  expect(trigger().textContent).toContain("Cloud");
  await waitFor(() =>
    expect(
      fake.log.some(
        (r) =>
          r.path === "/api/sessions/s1/target" &&
          r.body.target_id === "cli:codex:gpt-6-astra",
      ),
    ).toBe(true),
  );
});

it("runs a task and shows the event-derived timeline and summary", async () => {
  await boot();
  await choose(/qwen3:14b · This computer/);
  fireEvent.change(prompt(), { target: { value: "Fix the add function" } });
  fireEvent.click(send());
  const summary = await screen.findByRole("region", { name: "Task summary" }, { timeout: 3000 });
  expect(within(summary).getByText("src/app.ts")).toBeTruthy();
  expect(within(summary).getByText("npm test")).toBeTruthy();
  expect(within(summary).getByText("exit 0")).toBeTruthy();
  await waitFor(() => expect(within(summary).getByText("+1")).toBeTruthy());
  const timeline = screen.getAllByLabelText("Agent activity").at(-1)!;
  for (const label of ["Reading project", "Editing files", "Running tests", "Finished"])
    expect(within(timeline).getByText(label)).toBeTruthy();
  expect(screen.getByText(/Using .*This computer/)).toBeTruthy();
  fireEvent.click(within(summary).getByRole("button", { name: "Review changes" }));
  expect(await screen.findByRole("complementary", { name: "Drawer" })).toBeTruthy();
  const post = fake.log.find((r) => r.path === "/api/jobs" && r.method === "POST");
  expect(post?.body).toMatchObject({ model: "local:gguf:qwen", web: false });
});

it("asks before sending local conversation content to a cloud provider", async () => {
  await boot();
  await choose(/qwen3:14b · This computer/);
  fireEvent.change(prompt(), { target: { value: "First on this computer" } });
  fireEvent.click(send());
  await screen.findByRole("region", { name: "Task summary" }, { timeout: 3000 });
  await choose(/Codex · GPT-6-Astra/);
  fireEvent.change(prompt(), { target: { value: "Continue in the cloud" } });
  await waitFor(() => expect(send()).toHaveProperty("disabled", false));
  fireEvent.click(send());
  const dialog = await screen.findByRole("dialog", { name: "Send to a cloud provider?" });
  expect(within(dialog).getByText(/Send to Codex · GPT-6-Astra\?/)).toBeTruthy();
  expect(within(dialog).getByText(/2,400 characters/)).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(prompt()).toHaveProperty("value", "Continue in the cloud");
  const posts = () => fake.log.filter((r) => r.path === "/api/jobs" && r.method === "POST");
  expect(posts().every((r) => !r.body.handoff_consent)).toBe(true);
  fireEvent.click(send());
  const again = await screen.findByRole("dialog", { name: "Send to a cloud provider?" });
  fireEvent.click(within(again).getByRole("button", { name: "Send" }));
  await waitFor(() =>
    expect(posts().at(-1)?.body).toMatchObject({
      model: "cli:codex:gpt-6-astra",
      handoff_consent: true,
    }),
  );
});

it("accepts images only for vision rows and re-checks at send time", async () => {
  await boot();
  await choose(/qwen3:14b · This computer/);
  const input = document.querySelector<HTMLInputElement>('input[type="file"]')!;
  const png = new File([new Uint8Array([137, 80, 78, 71])], "shot.png", {
    type: "image/png",
  });
  await act(async () => {
    fireEvent.change(input, { target: { files: [png] } });
  });
  expect(await screen.findByText(/qwen3:14b · This computer does not accept images/)).toBeTruthy();
  expect(screen.queryByRole("list", { name: "Attachments" })).toBeNull();
  await choose(/Codex · GPT-6-Astra/);
  await act(async () => {
    fireEvent.change(input, { target: { files: [png] } });
  });
  expect(await screen.findByRole("button", { name: "Remove shot.png" })).toBeTruthy();
  await choose(/qwen3:14b · This computer/);
  expect(
    screen.getByText(/does not accept images. Remove the image or choose a model marked Vision/),
  ).toBeTruthy();
  expect(send()).toHaveProperty("disabled", true);
});

it("stages a model change during a running task for the next message", async () => {
  fake = installFakeBackend({ stepMs: 150 });
  await boot();
  await choose(/qwen3:14b · This computer/);
  fireEvent.change(prompt(), { target: { value: "Slow task" } });
  fireEvent.click(send());
  await screen.findByRole("button", { name: "Stop task" });
  await choose(/Codex · GPT-6-Astra/);
  expect(screen.getByText("Applies to your next message")).toBeTruthy();
});
