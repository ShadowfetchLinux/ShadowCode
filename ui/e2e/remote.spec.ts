import { test, expect, type Page } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { startRemoteServer, type RemoteServer } from "./fakeRemoteServer";

// The web interface as a phone sees it: the same build loaded over HTTP from
// a remote access server (e2e/fakeRemoteServer.ts), no desktop bridge and no
// test transport installed, at a 375 px wide viewport.
const dist = fileURLToPath(new URL("../dist-e2e", import.meta.url));
let server: RemoteServer;

test.use({ viewport: { width: 375, height: 812 }, hasTouch: true });

test.beforeEach(async ({ page }) => {
  server = await startRemoteServer(dist, { stepMs: 60 });
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as Page & { errors?: string[] }).errors = errors;
});
test.afterEach(async ({ page }) => {
  expect((page as Page & { errors?: string[] }).errors).toEqual([]);
  await page.close();
  await server.close();
});

async function pairAndOpen(page: Page) {
  await page.goto(`${server.url}/#pair=${server.pairingCode()}`);
  await expect(
    page.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
}

const noSideScroll = (page: Page) =>
  page.evaluate(
    () => document.documentElement.scrollWidth <= window.innerWidth + 1,
  );

test("a browser asks to be paired, and a pairing link connects it", async ({
  page,
}) => {
  await page.goto(server.url);
  await expect(
    page.getByRole("heading", { name: "Pair this device" }),
  ).toBeVisible();
  expect(server.requests.some((r) => r.path.startsWith("/api/"))).toBe(false);
  // A used or unknown link is refused with a plain explanation.
  await page
    .getByLabel("Pairing link")
    .fill(`${server.url}/#pair=${"x".repeat(43)}`);
  await page.getByRole("button", { name: "Pair" }).click();
  await expect(page.getByRole("alert")).toContainText("not valid any more");
  // A fresh link pairs, drops the code from the address bar, and loads.
  await pairAndOpen(page);
  expect(new URL(page.url()).hash).toBe("");
  const token = await page.evaluate(() =>
    localStorage.getItem("shadow:remote:token"),
  );
  expect(token).toMatch(/^scr_/);
  const api = server.requests.filter((r) => r.path.startsWith("/api/"));
  expect(api.length).toBeGreaterThan(0);
  expect(api.every((r) => r.authorized)).toBe(true);
  expect(await noSideScroll(page)).toBe(true);
  await page.screenshot({ path: "test-results/remote-phone.png" });
});

test("approvals arrive over the event stream and are answered from the phone", async ({
  page,
}) => {
  await pairAndOpen(page);
  await page.waitForTimeout(300);
  server.fake.requestApproval({
    command: "npm run lint",
    reason: "Check the style",
  });
  const card = page.locator(".approval");
  await expect(card).toContainText("npm run lint", { timeout: 3000 });
  expect(await noSideScroll(page)).toBe(true);
  await card.getByRole("button", { name: "Allow" }).click();
  await expect(card).toHaveCount(0, { timeout: 3000 });
  const decided = server.fake.log.find(
    (r) => r.method === "POST" && r.path.startsWith("/api/approvals/"),
  );
  expect(decided?.body).toMatchObject({ decision: "approve" });
});

test("a task started from the phone streams to completion", async ({
  page,
}) => {
  await pairAndOpen(page);
  await page.getByRole("button", { name: /Model for this task/ }).click();
  const search = page.getByRole("combobox", { name: "Search models" });
  await search.fill("qwen3:14b");
  await search.press("Enter");
  await page
    .getByRole("textbox", { name: "Message ShadowCode" })
    .fill("Fix the add function");
  await page.getByRole("button", { name: "Send task" }).click();
  await expect(
    page.getByRole("region", { name: "Task summary" }).last(),
  ).toBeVisible({ timeout: 15000 });
  expect(await noSideScroll(page)).toBe(true);
  await page.screenshot({
    path: "test-results/remote-phone-task.png",
    fullPage: true,
  });
});

test("sidebar, settings and the drawer fit a phone", async ({ page }) => {
  await pairAndOpen(page);
  await page.getByRole("button", { name: "Show sidebar" }).click();
  const sidebar = page.getByRole("complementary", {
    name: "Projects and tasks",
  });
  await expect(sidebar).toBeVisible();
  expect(await noSideScroll(page)).toBe(true);
  await page.screenshot({ path: "test-results/remote-phone-sidebar.png" });
  await sidebar.getByRole("button", { name: "Settings" }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "Remote access" }).click();
  // Remote access is managed on the computer running ShadowCode.
  await expect(settings).toContainText(
    "You are using ShadowCode from another device",
  );
  expect(server.requests.some((r) => r.path.startsWith("/api/remote"))).toBe(
    false,
  );
  expect(await noSideScroll(page)).toBe(true);
  await page.screenshot({ path: "test-results/remote-phone-settings.png" });
  await settings.getByRole("button", { name: "Close" }).click();
  await page.getByRole("button", { name: "Review changes" }).first().click();
  await expect(
    page.getByRole("complementary", { name: "Drawer" }),
  ).toBeVisible();
  expect(await noSideScroll(page)).toBe(true);
  await page.screenshot({ path: "test-results/remote-phone-drawer.png" });
});

test("an unpaired device goes back to the pairing screen", async ({ page }) => {
  await pairAndOpen(page);
  await page.evaluate(() =>
    localStorage.setItem("shadow:remote:token", "scr_revoked_token_value_x"),
  );
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Pair this device" }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("shadow:remote:token")),
  ).toBeNull();
});
