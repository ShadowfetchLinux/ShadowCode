import { test, expect, type Page } from "@playwright/test";
import { installFakeBackend } from "./fakeBackend";
import { installFakeVoice, type FakeVoiceOptions } from "./fakeVoice";

// Dictation against the fake engine with fake voice routes (fakeVoice.ts):
// the mic button, the listening pill, insertion at the cursor, and setting up
// a model in Settings › Voice.
async function start(page: Page, voice: FakeVoiceOptions = {}) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as Page & { errors?: string[] }).errors = errors;
  await page.addInitScript(installFakeBackend, { stepMs: 90 });
  await page.addInitScript(installFakeVoice, voice);
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
}
test.afterEach(async ({ page }) => {
  expect((page as Page & { errors?: string[] }).errors).toEqual([]);
});

const composer = (page: Page) =>
  page.getByRole("textbox", { name: "Message ShadowCode" });
const mic = (page: Page) => page.locator(".mic-btn");
const fakeLog = (page: Page) =>
  page.evaluate(
    () =>
      (
        window as unknown as {
          __SHADOW_FAKE__: {
            log: { method: string; path: string; body: any }[];
          };
        }
      ).__SHADOW_FAKE__.log,
  );

test("click to dictate inserts the transcript at the cursor without sending", async ({
  page,
}) => {
  await start(page, {
    installed: true,
    transcript: "add a unit test for the parser",
  });
  await composer(page).fill("Please now");
  // Cursor after "Please".
  await composer(page).evaluate((el: HTMLTextAreaElement) =>
    el.setSelectionRange(6, 6),
  );
  await expect(mic(page)).toHaveAttribute("aria-pressed", "false");
  await mic(page).click();
  await expect(mic(page)).toHaveAttribute("aria-pressed", "true");
  const pill = page.locator(".voice-status");
  await expect(pill).toContainText("Listening");
  // Live words and the level meter while listening.
  await expect(pill.locator(".voice-partial")).toContainText("add");
  await expect(mic(page).locator(".voice-meter span")).toHaveCount(5);
  await mic(page).click();
  await expect(composer(page)).toHaveValue(
    "Please add a unit test for the parser now",
  );
  await expect(mic(page)).toHaveAttribute("aria-pressed", "false");
  await expect(pill).toHaveCount(0);
  // Inserted, never sent.
  const log = await fakeLog(page);
  expect(log.some((r) => r.path === "/api/jobs" && r.method === "POST")).toBe(
    false,
  );
  expect(log.filter((r) => r.path === "/api/voice/stop")).toHaveLength(1);
  // Undo removes exactly the dictated words.
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(composer(page)).toHaveValue("Please now");
});

test("Ctrl+Shift+Space toggles dictation and Escape cancels", async ({
  page,
}) => {
  await start(page, { installed: true, transcript: "hello from the keyboard" });
  await composer(page).click();
  await page.keyboard.press("Control+Shift+Space");
  await expect(mic(page)).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(mic(page)).toHaveAttribute("aria-pressed", "false");
  await expect(composer(page)).toHaveValue("");
  // Tap to start, tap to stop.
  await page.keyboard.press("Control+Shift+Space");
  await expect(page.locator(".voice-status")).toContainText("Listening");
  await page.keyboard.press("Control+Shift+Space");
  await expect(composer(page)).toHaveValue("hello from the keyboard");
  const log = await fakeLog(page);
  expect(log.filter((r) => r.path === "/api/voice/cancel")).toHaveLength(1);
});

test("without a model the mic opens Settings › Voice, where a model installs on request", async ({
  page,
}) => {
  await start(page);
  await mic(page).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings).toBeVisible();
  await expect(
    settings.getByRole("button", { name: "Voice", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(settings.getByText(/No voice model is installed/)).toBeVisible();
  // Nothing downloaded until the click.
  let log = await fakeLog(page);
  expect(log.some((r) => r.path === "/api/voice/models/install")).toBe(false);
  const tiny = settings.getByRole("article", {
    name: "Whisper tiny (English)",
  });
  await tiny.getByRole("button", { name: /Install/ }).click();
  await expect(tiny.getByRole("button", { name: "Remove" })).toBeVisible({
    timeout: 8000,
  });
  await expect(tiny.getByText("In use")).toBeVisible();
  await expect(settings.getByText(/No voice model is installed/)).toHaveCount(
    0,
  );
  log = await fakeLog(page);
  expect(log.find((r) => r.path === "/api/voice/models/install")?.body).toEqual(
    { model: "tiny.en" },
  );
  // OpenRouter stays off without a key.
  await expect(settings.getByLabel(/OpenRouter/)).toBeDisabled();
  await settings.getByRole("button", { name: "Close" }).last().click();
  // Now dictation works.
  await mic(page).click();
  await expect(mic(page)).toHaveAttribute("aria-pressed", "true");
  await mic(page).click();
  await expect(composer(page)).toHaveValue("Add a unit test for the parser.");
});
