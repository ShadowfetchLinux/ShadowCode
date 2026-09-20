import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.beforeEach(async ({ page, request }) => {
  const { workspace } = await (await request.get("/api/health")).json();
  const session = await (
    await request.post("/api/sessions", {
      data: { workspace, title: "New task" },
    })
  ).json();
  await page.addInitScript(
    (id) => localStorage.setItem("shadow:selected", id),
    session.id,
  );
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What are we building?" }),
  ).toBeVisible();
});

test("workspace layout, drafts, palette, files and terminal", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.screenshot({
    path: "../artifacts/workspace-light.png",
    fullPage: true,
  });
  const prompt = page.getByRole("textbox", { name: "Message ShadowCode" });
  await prompt.fill("A draft that survives reload");
  await page.waitForTimeout(300);
  await page.reload();
  await expect(prompt).toHaveValue("A draft that survives reload");
  await page.keyboard.press("Control+k");
  await page.getByPlaceholder("Type a command…").fill("settings");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("heading", { name: "Settings", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Browse files", exact: true }).click();
  await page.getByRole("button", { name: "· README.md", exact: true }).click();
  await expect(page.locator(".file-view")).toContainText("A safe workspace");
  await page
    .getByRole("button", { name: "Terminal", exact: true })
    .first()
    .click();
  await page
    .getByRole("textbox", { name: "Terminal command" })
    .fill("printf shadow-terminal-ok");
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await expect(page.locator(".terminal-result pre")).toContainText(
    "shadow-terminal-ok",
  );
  expect(errors).toEqual([]);
});

test("runs a real offline task, restores history, and avoids old event replay", async ({
  page,
}) => {
  const prompt = page.getByRole("textbox", { name: "Message ShadowCode" });
  await prompt.fill("Create a Python hello-world project and run it");
  await page.getByRole("button", { name: "Send task", exact: true }).click();
  await expect(prompt).toHaveValue("");
  await expect(
    page.getByRole("button", { name: "Stop task", exact: true }),
  ).toHaveCount(0, { timeout: 30000 });
  await expect(page.locator(".msg-user")).toHaveCount(1);
  await expect(page.locator(".msg-agent").last()).toBeVisible();
  await page.screenshot({
    path: "../artifacts/task-complete.png",
    fullPage: true,
  });
  await page.reload();
  await expect(page.locator(".msg-user")).toHaveCount(1);
  await prompt.fill("Explain what you created");
  await page.getByRole("button", { name: "Send task", exact: true }).click();
  await expect(page.locator(".msg-user")).toHaveCount(2);
  await expect(
    page.getByRole("button", { name: "Stop task", exact: true }),
  ).toHaveCount(0, { timeout: 30000 });
  await expect(page.locator(".msg-user")).toHaveCount(2);
});

test("light and dark home screens pass accessibility checks", async ({
  page,
}) => {
  for (const theme of ["light", "dark"]) {
    await page.evaluate(
      (theme) => (document.documentElement.dataset.theme = theme),
      theme,
    );
    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(results.violations).toEqual([]);
    await page.screenshot({
      path: `../artifacts/workspace-${theme}.png`,
      fullPage: true,
    });
  }
});

test("compact layout keeps composer and controls reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 600, height: 850 });
  if (
    await page
      .getByRole("button", { name: "Hide sidebar", exact: true })
      .isVisible()
  )
    await page
      .getByRole("button", { name: "Hide sidebar", exact: true })
      .click();
  await expect(
    page.getByRole("textbox", { name: "Message ShadowCode" }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: "../artifacts/workspace-compact.png",
    fullPage: true,
  });
});

test("a dropped event connection recovers without duplicating the task", async ({
  page,
}) => {
  let first = true;
  await page.route("**/api/jobs/*/events?*", (route) => {
    if (first) {
      first = false;
      return route.abort("connectionreset");
    }
    return route.continue();
  });
  await page
    .getByRole("textbox", { name: "Message ShadowCode" })
    .fill("Explain this workspace");
  await page.getByRole("button", { name: "Send task", exact: true }).click();
  await expect(page.locator(".msg-user")).toHaveCount(1, { timeout: 20000 });
  await expect(
    page.getByRole("button", { name: "Stop task", exact: true }),
  ).toHaveCount(0, { timeout: 20000 });
  await expect(page.locator(".msg-agent").last()).toBeVisible();
});

test("settings dialogs have labeled controls and trap keyboard focus", async ({
  page,
}) => {
  await page.keyboard.press("Control+,");
  const dialog = page.getByRole("dialog", { name: "Settings", exact: true });
  await expect(dialog).toBeVisible();
  for (const tab of [
    "Model",
    "Permissions",
    "Appearance",
    "Hooks",
    "MCP",
    "Plugins",
  ]) {
    await dialog.getByRole("button", { name: tab, exact: true }).click();
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
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("new-task shortcuts and deletion keep task selection usable", async ({
  page,
  request,
}) => {
  const first = await page.evaluate(() =>
    localStorage.getItem("shadow:selected"),
  );
  await request.patch(`/api/sessions/${first}`, {
    data: { workspace: "", title: "Draft parent" },
  });
  await page.reload();
  const prompt = page.getByRole("textbox", { name: "Message ShadowCode" });
  await expect(prompt).toBeVisible();
  await prompt.fill("Preserve this unsent draft");
  await page.keyboard.press("Control+n");
  await expect(prompt).toHaveValue("");
  await page.getByRole("button", { name: "Draft parent", exact: true }).click();
  await expect(prompt).toHaveValue("Preserve this unsent draft");
  await prompt.fill("/new");
  await page.getByRole("button", { name: "Send task", exact: true }).click();
  await expect(prompt).toHaveValue("");
  const toDelete = await page.evaluate(() =>
    localStorage.getItem("shadow:selected"),
  );
  expect(toDelete).not.toBe(first);
  await expect(page.locator(".loading-task")).toHaveCount(0);
  await page.keyboard.press("Control+k");
  await page
    .getByRole("textbox", { name: "Search commands" })
    .fill("manage tasks");
  await page.keyboard.press("Enter");
  page.once("dialog", (dialog) => dialog.accept());
  await page
    .locator(".drawer .item.active")
    .getByRole("button", { name: "Delete", exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("shadow:selected")))
    .not.toBe(toDelete);
  await expect(prompt).toBeEnabled();
});
