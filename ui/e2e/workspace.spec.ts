import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { installFakeBackend } from "./fakeBackend";

// Every test drives the production UI build against the deterministic fake
// engine; nothing here talks to a real vendor CLI or model.
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as Page & { errors?: string[] }).errors = errors;
  await page.addInitScript(installFakeBackend, { stepMs: 90 });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
});
test.afterEach(async ({ page }) => {
  expect((page as Page & { errors?: string[] }).errors).toEqual([]);
});

const trigger = (page: Page) =>
  page.getByRole("button", { name: /Model for this task/ });
const prompt = (page: Page) =>
  page.getByRole("textbox", { name: "Message ShadowCode" });
const send = (page: Page) => page.getByRole("button", { name: "Send task" });
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

async function chooseBySearch(page: Page, text: string) {
  await trigger(page).click();
  const search = page.getByRole("combobox", { name: "Search models" });
  await expect(search).toBeFocused();
  await search.fill(text);
  await search.press("Enter");
  await expect(page.getByRole("listbox")).toHaveCount(0);
}

async function runLocalTask(page: Page, text: string) {
  await chooseBySearch(page, "qwen");
  await prompt(page).fill(text);
  await send(page).click();
  await expect(
    page.getByRole("region", { name: "Task summary" }).last(),
  ).toBeVisible({
    timeout: 15000,
  });
}

test("picks a subscription row with the keyboard", async ({ page }) => {
  await expect(trigger(page)).toContainText("Choose a model");
  await prompt(page).fill("Explain the build");
  await expect(send(page)).toBeDisabled();
  await trigger(page).click();
  await expect(
    page.getByRole("group", { name: "Subscriptions" }),
  ).toBeVisible();
  await expect(
    page.getByRole("group", { name: "On this computer" }),
  ).toBeVisible();
  const search = page.getByRole("combobox", { name: "Search models" });
  // Details for the active row open with the right arrow.
  await search.press("ArrowRight");
  await expect(page.locator(".unified-picker-details")).toContainText(
    "Weekly · 2% left",
  );
  await search.press("ArrowLeft");
  await search.press("Home");
  await search.press("Enter");
  await expect(trigger(page)).toContainText("Codex · GPT-6-Astra");
  await expect(trigger(page)).toContainText("Cloud");
  await expect(trigger(page)).toBeFocused();
  await expect(send(page)).toBeEnabled();
  const log = await fakeLog(page);
  expect(
    log.some(
      (r) =>
        r.path === "/api/sessions/s1/target" &&
        r.body.target_id === "cli:codex:gpt-6-astra",
    ),
  ).toBe(true);
  // The choice belongs to the conversation and survives a reload.
  await page.reload();
  await expect(trigger(page)).toContainText("Codex · GPT-6-Astra");
});

test("picks a local row; web and permission controls follow the row", async ({
  page,
}) => {
  await expect(
    page.getByRole("button", { name: "Web lookups for this task" }),
  ).toHaveCount(0);
  await chooseBySearch(page, "qwen");
  await expect(trigger(page)).toHaveAttribute(
    "aria-label",
    /qwen3:14b · This computer/,
  );
  await expect(trigger(page)).toHaveText("Localqwen3:14b");
  const web = page.getByRole("button", { name: "Web lookups for this task" });
  await expect(web).toHaveAttribute("aria-pressed", "false");
  await web.click();
  await expect(web).toHaveAttribute("aria-pressed", "true");
  await page
    .getByRole("button", { name: /Permissions: Ask before actions/ })
    .click();
  await page.getByRole("radio", { name: /Allow project edits/ }).click();
  await expect(
    page.getByRole("button", { name: /Permissions: Allow project edits/ }),
  ).toBeVisible();
  const log = await fakeLog(page);
  expect(
    log.find((r) => r.path === "/api/config" && r.method === "PUT")?.body
      .values,
  ).toEqual({ permissions: { mode: "allow_edits" } });
});

test("runs a task with streamed events and reviews the changes", async ({
  page,
}) => {
  await chooseBySearch(page, "qwen");
  await prompt(page).fill("Fix the add function");
  await send(page).click();
  const live = page.locator(".working .activity-timeline");
  await expect(live).toBeVisible();
  await expect(live.getByText("Reading project")).toBeVisible();
  const summary = page.getByRole("region", { name: "Task summary" });
  await expect(summary).toBeVisible({ timeout: 15000 });
  await expect(summary).toContainText("src/app.ts");
  await expect(summary).toContainText("+1");
  await expect(summary).toContainText("npm test");
  await expect(summary).toContainText("exit 0");
  // The finished timeline sits above the answer (or in the summary block
  // when the task gave no answer text).
  const timeline = page
    .locator(
      ".msg-with-activity .activity-timeline, .msg-summary .activity-timeline",
    )
    .last();
  for (const step of ["Reading project", "Editing files", "Running checks"])
    await expect(timeline.getByText(step, { exact: true })).toBeVisible();
  await expect(timeline.getByText("Finished", { exact: true })).toHaveCount(0);
  await expect(summary).toContainText("Finished");
  // Each step expands to the real tool call and its output.
  await timeline.getByText("Running checks", { exact: true }).click();
  await expect(timeline.getByText("Tests  4 passed (4)")).toBeVisible();
  await summary.getByRole("button", { name: "Review changes" }).click();
  const drawer = page.getByRole("complementary", { name: "Drawer" });
  await expect(drawer).toBeVisible();
  await drawer.getByText("src/app.ts").first().click();
  await expect(drawer).toContainText("export const add = (a, b) => a + b;");
  await page.screenshot({
    path: "test-results/task-complete.png",
    fullPage: true,
  });
});

test("asks for consent before sending local context to a cloud row", async ({
  page,
}) => {
  await runLocalTask(page, "Start on this computer");
  await trigger(page).click();
  await page.getByRole("option", { name: /Codex · GPT-6-Astra/ }).click();
  await prompt(page).fill("Continue in the cloud");
  await send(page).click();
  const dialog = page.getByRole("dialog", {
    name: "Send to a cloud provider?",
  });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Send to Codex · GPT-6-Astra?");
  await expect(dialog).toContainText("2,400 characters");
  await expect(
    new AxeBuilder({ page })
      .include('[role="dialog"]')
      .withTags(["wcag2a", "wcag2aa"])
      .analyze(),
  ).resolves.toMatchObject({ violations: [] });
  await dialog.getByRole("button", { name: "Send" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Task summary" })).toHaveCount(
    2,
    {
      timeout: 15000,
    },
  );
  const posts = (await fakeLog(page)).filter(
    (r) => r.path === "/api/jobs" && r.method === "POST",
  );
  expect(
    posts.map((r) => [r.body.model, Boolean(r.body.handoff_consent)]),
  ).toEqual([
    ["local:gguf:qwen", false],
    ["cli:codex:gpt-6-astra", false],
    ["cli:codex:gpt-6-astra", true],
  ]);
});

test("a Sign in row opens Accounts and Connect streams the official login", async ({
  page,
}) => {
  await trigger(page).click();
  await page.getByRole("option", { name: /Claude Code · Default/ }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings).toBeVisible();
  const claude = settings.getByRole("article", { name: "Claude Code" });
  const connect = claude.getByRole("button", { name: "Connect" });
  await expect(connect).toBeFocused();
  await connect.click();
  await expect(
    claude.getByRole("link", { name: /claude\.ai\/oauth/ }),
  ).toBeVisible();
  await expect(claude.locator("code.device-code")).toHaveText("WXYZ-1234");
  await expect(claude.getByText("Signed in.")).toBeVisible({ timeout: 10000 });
  await expect(claude.getByText("Ready", { exact: true })).toBeVisible();
  await settings.getByRole("button", { name: "Close" }).last().click();
  await trigger(page).click();
  await expect(
    page.getByRole("option", { name: /Claude Code · Default/ }),
  ).toContainText("Ready");
});

test("loads a local model from Settings", async ({ page }) => {
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Local models" }).click();
  await expect(settings.getByText(/Ready · Vulkan/)).toBeVisible();
  await expect(settings.getByText("No model loaded")).toBeVisible();
  const qwen = settings.getByRole("article", { name: "qwen3:14b" });
  await qwen.getByRole("button", { name: "Load" }).click();
  await expect(settings.getByText(/qwen3:14b · Vulkan0/)).toBeVisible();
  await expect(qwen.getByRole("button", { name: "Unload" })).toBeVisible();
  const gptoss = settings.getByRole("article", { name: "gpt-oss:20b" });
  await expect(gptoss).toContainText("unknown model architecture: gptoss");
  await expect(
    new AxeBuilder({ page })
      .include('[role="dialog"]')
      .withTags(["wcag2a", "wcag2aa"])
      .analyze(),
  ).resolves.toMatchObject({ violations: [] });
});

test("light and dark themes pass accessibility checks, picker open", async ({
  page,
}) => {
  for (const theme of ["light", "dark"]) {
    await page.evaluate(
      (t) => (document.documentElement.dataset.theme = t),
      theme,
    );
    await trigger(page).click();
    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(results.violations).toEqual([]);
    await page.screenshot({ path: `test-results/picker-${theme}.png` });
    await page.keyboard.press("Escape");
    await expect(page.getByRole("listbox")).toHaveCount(0);
  }
});

test("settings sections are accessible and trap focus", async ({ page }) => {
  await page.keyboard.press("Control+,");
  const dialog = page.getByRole("dialog", { name: "Settings" });
  for (const section of [
    "Accounts",
    "Local models",
    "Permissions & network",
    "Appearance",
    "Advanced",
  ]) {
    await dialog.getByRole("button", { name: section, exact: true }).click();
    const results = await new AxeBuilder({ page })
      .include('[role="dialog"]')
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(results.violations).toEqual([]);
  }
  await dialog.locator("button").last().focus();
  await page.keyboard.press("Tab");
  expect(
    await dialog.evaluate((el) => el.contains(document.activeElement)),
  ).toBe(true);
  // Global shortcuts stay inactive behind a dialog.
  await page.keyboard.press("Control+b");
  await expect(page.locator(".sidebar")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("the window works at its 520 px minimum width", async ({ page }) => {
  await page.setViewportSize({ width: 520, height: 800 });
  await expect(page.locator(".sidebar")).toHaveCount(0);
  await expect(prompt(page)).toBeVisible();
  await expect(trigger(page)).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await trigger(page).click();
  const menu = page.locator(".unified-picker-menu");
  const box = await menu.boundingBox();
  expect(box && box.x >= 0 && box.x + box.width <= 520).toBe(true);
  await page.screenshot({ path: "test-results/compact.png", fullPage: true });
});
