import { test, expect, type Page } from "@playwright/test";
import { installFakeBackend } from "./fakeBackend";

// Settings › Remote access in the desktop window (fake engine).
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as Page & { errors?: string[] }).errors = errors;
  await page.addInitScript(installFakeBackend, { stepMs: 60 });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
});
test.afterEach(async ({ page }) => {
  expect((page as Page & { errors?: string[] }).errors).toEqual([]);
});

test("turns remote access on, pairs with a QR code and warns about plain HTTP", async ({
  page,
}) => {
  await page
    .getByRole("complementary", { name: "Projects and tasks" })
    .getByRole("button", { name: "Settings" })
    .click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Remote access" }).click();
  const toggle = settings.getByLabel("Turn on remote access");
  await expect(toggle).not.toBeChecked();
  await expect(
    settings.getByLabel("Allow terminals over remote access"),
  ).not.toBeChecked();
  await toggle.check();
  await expect(settings.getByText("http://127.0.0.1:7390")).toBeVisible();
  await settings.getByRole("button", { name: "Pair a device" }).click();
  await expect(
    settings.getByRole("img", { name: "Pairing QR code" }),
  ).toBeVisible();
  await expect(settings).toContainText("#pair=");
  await page.screenshot({ path: "test-results/remote-settings.png" });
  // A local network address is plain HTTP: the page says so.
  await settings
    .getByLabel("Address", { exact: true })
    .selectOption("192.168.1.20");
  await settings.getByRole("button", { name: "Apply" }).click();
  await expect(settings.getByText(/Plain HTTP is not encrypted/)).toBeVisible();
  // Tailscale is encrypted already.
  await settings
    .getByLabel("Address", { exact: true })
    .selectOption("100.90.1.2");
  await settings.getByRole("button", { name: "Apply" }).click();
  await expect(settings.getByText(/Plain HTTP is not encrypted/)).toHaveCount(
    0,
  );
  // Devices can be unpaired.
  await settings.getByRole("button", { name: "Unpair", exact: true }).click();
  await expect(settings.getByText("No devices are paired.")).toBeVisible();
  // Phone notifications stay off until a server and topic are entered.
  await expect(
    settings.getByRole("button", { name: "Send a test" }),
  ).toBeDisabled();
  await settings.getByLabel("Server").fill("https://ntfy.example");
  await settings.getByRole("button", { name: "Generate" }).click();
  await settings.getByRole("button", { name: "Save" }).last().click();
  await expect(
    settings.getByRole("button", { name: "Send a test" }),
  ).toBeEnabled();
  const log = await page.evaluate(
    () =>
      (
        window as unknown as {
          __SHADOW_FAKE__: {
            log: { method: string; path: string; body: any }[];
          };
        }
      ).__SHADOW_FAKE__.log,
  );
  const saved = log.find((r) => r.path === "/api/remote/ntfy");
  expect(saved?.body.server).toBe("https://ntfy.example");
  expect(saved?.body.topic).toMatch(/^shadowcode-/);
  await page.screenshot({
    path: "test-results/remote-settings-ntfy.png",
    fullPage: true,
  });
});
