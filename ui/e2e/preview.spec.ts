import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test, expect, type Page } from "@playwright/test";
import { installFakeBackend } from "./fakeBackend";
import { installFakePreview } from "./fakePreview";

// The drawer's Preview tab against the fake engine. The previewed page is a
// tiny static site served by Playwright at PROXY, standing in for the
// engine's loopback proxy: its HTML carries the picker <script> the proxy
// adds, and the picker is the real script from the engine.
const PROXY = "http://127.0.0.1:4191";
const PICKER = readFileSync(
  fileURLToPath(
    new URL("../../native/core/src/preview/picker.js", import.meta.url),
  ),
  "utf8",
);

const PAGES: Record<string, string> = {
  "/settings": `<!doctype html><html><head>
<script src="/__shadowcode_preview__/picker.js"></script>
<title>Settings · Demo</title>
<style>.btn{padding:6px 14px;border-radius:6px}.primary{background:#2563eb;color:#fff}</style>
</head><body>
<main id="app"><h1>Settings</h1>
<form class="settings" onsubmit="document.body.dataset.submitted='yes';return false">
  <label>Name <input name="name" value="Demo"></label>
  <button type="button" class="btn">Cancel</button>
  <button type="submit" class="btn primary">Save</button>
</form>
<a href="/about">About</a>
</main>
<script>console.error("boom from the page"); console.warn("slow render");</script>
</body></html>`,
  "/about": `<!doctype html><html><head>
<script src="/__shadowcode_preview__/picker.js"></script>
<title>About</title></head><body><h1>About this demo</h1></body></html>`,
};

async function start(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as Page & { errors?: string[] }).errors = errors;
  await page.route(`${PROXY}/**`, (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/__shadowcode_preview__/picker.js")
      // Like the engine: the app origin the proxy was opened for, baked in.
      return route.fulfill({
        contentType: "text/javascript; charset=utf-8",
        body: PICKER.replace(
          '/*APP_ORIGIN*/""',
          JSON.stringify(new URL(page.url()).origin),
        ),
      });
    const html = PAGES[path];
    return html
      ? route.fulfill({ contentType: "text/html; charset=utf-8", body: html })
      : route.fulfill({ status: 404, body: "missing" });
  });
  await page.addInitScript(installFakeBackend, { stepMs: 60 });
  await page.addInitScript(installFakePreview, { proxyOrigin: PROXY });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
}
test.afterEach(async ({ page }) => {
  expect((page as Page & { errors?: string[] }).errors).toEqual([]);
});

const drawer = (page: Page) =>
  page.getByRole("complementary", { name: "Drawer" });
const preview = (page: Page) => page.getByRole("region", { name: "Preview" });
const frame = (page: Page) => page.frameLocator("iframe.preview-frame");
const chips = (page: Page) => page.getByRole("list", { name: "Attachments" });
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

async function openPreview(page: Page) {
  await page.getByRole("button", { name: "Review changes" }).click();
  await drawer(page)
    .locator(".drawer-tabs")
    .getByRole("button", { name: "Preview" })
    .click();
  await preview(page)
    .getByRole("button", { name: /localhost:5173/ })
    .click();
  await expect(
    frame(page).getByRole("heading", { name: "Settings" }),
  ).toBeVisible();
  await expect(
    preview(page).getByRole("textbox", { name: "Preview address" }),
  ).toHaveValue("http://localhost:5173/settings");
}

test("pick an element and a console error, then send them with the prompt", async ({
  page,
}) => {
  await start(page);
  await openPreview(page);
  const bar = preview(page);

  // The wider drawer leaves the composer room for its controls.
  const composerBox = await page.locator("form.composer").boundingBox();
  const sendBox = await page
    .getByRole("button", { name: "Send task" })
    .boundingBox();
  expect(sendBox!.x + sendBox!.width).toBeLessThanOrEqual(
    composerBox!.x + composerBox!.width,
  );

  // Console errors captured by the picker show in the strip.
  const console = bar.getByRole("region", { name: "Console errors" });
  await expect(console).toContainText("boom from the page");
  await expect(console).toContainText("1 error, 1 warning");

  // Picking: the page's own click handler never runs.
  const pick = bar.getByRole("button", { name: "Pick element" });
  await expect(pick).toBeEnabled();
  await pick.click();
  await expect(
    bar.getByRole("button", { name: /Picking… click an element/ }),
  ).toHaveAttribute("aria-pressed", "true");
  await frame(page).getByRole("button", { name: "Save" }).click();
  await expect(chips(page)).toContainText('button "Save"');
  await expect(
    bar.getByRole("button", { name: "Pick element" }),
  ).toHaveAttribute("aria-pressed", "false");
  expect(
    await frame(page)
      .locator("body")
      .evaluate((body) => body.dataset.submitted ?? "no"),
  ).toBe("no");

  // One console error as a chip too.
  await console
    .getByRole("button", { name: "Attach: boom from the page" })
    .click();
  await expect(chips(page)).toContainText("Console error");

  // A chip can be removed and the element picked again.
  await chips(page)
    .getByRole("button", { name: 'Remove button "Save"' })
    .click();
  await expect(chips(page)).not.toContainText('button "Save"');
  await bar.getByRole("button", { name: "Pick element" }).click();
  await frame(page).getByRole("button", { name: "Save" }).click();
  await expect(chips(page)).toContainText('button "Save"');

  // Sending puts both into the prompt as structured context.
  await page.getByRole("button", { name: /Model for this task/ }).click();
  const search = page.getByRole("combobox", { name: "Search models" });
  await search.fill("qwen3:14b");
  await search.press("Enter");
  await page
    .getByRole("textbox", { name: "Message ShadowCode" })
    .fill("Make the save button green");
  await page.getByRole("button", { name: "Send task" }).click();
  await expect(chips(page)).toHaveCount(0);
  const job = (await fakeLog(page)).find(
    (r) => r.method === "POST" && r.path === "/api/jobs",
  );
  // The message stays the user's text; the picked items go as `context`
  // (the engine appends them after the message).
  expect(job?.body.task).toBe("Make the save button green");
  const context: { kind: string; label: string; text: string }[] =
    job?.body.context ?? [];
  expect(context.map((c) => [c.kind, c.label])).toEqual([
    ["console", "Console error"],
    ["element", 'button "Save"'],
  ]);
  const task = context.map((c) => c.text).join("\n\n");
  expect(task).toContain(
    "Console messages from http://localhost:5173/settings:\n```text\n[error] boom from the page",
  );
  expect(task).toContain(
    'Element on http://localhost:5173/settings (“Settings · Demo”):\n<button class="btn primary" type="submit"> "Save"',
  );
  expect(task).toContain("- Selector: `button.btn.primary`");
  expect(task).toContain("- Accessibility: role button");
  expect(task).toContain("background-color: rgb(37, 99, 235)");
  expect(task).toContain(
    '```html\n<button type="submit" class="btn primary">Save</button>\n```',
  );
  // The conversation shows the message with its context.
  await expect(
    page.getByText(/Context from the app preview/).first(),
  ).toBeVisible();
});

test("the address bar follows navigation and refuses other computers", async ({
  page,
}) => {
  await start(page);
  await openPreview(page);
  const bar = preview(page);
  const address = bar.getByRole("textbox", { name: "Preview address" });

  await frame(page).getByRole("link", { name: "About" }).click();
  await expect(
    frame(page).getByRole("heading", { name: "About this demo" }),
  ).toBeVisible();
  await expect(address).toHaveValue("http://localhost:5173/about");
  await bar.getByRole("button", { name: "Back" }).click();
  await expect(
    frame(page).getByRole("heading", { name: "Settings" }),
  ).toBeVisible();
  await expect(address).toHaveValue("http://localhost:5173/settings");

  // Device widths change the frame's layout width.
  await bar.getByRole("button", { name: "Phone" }).click();
  await expect(page.locator("iframe.preview-frame")).toHaveCSS(
    "width",
    "390px",
  );
  expect(
    await frame(page)
      .locator("body")
      .evaluate(() => window.innerWidth),
  ).toBe(390);

  await address.fill("https://example.com");
  await address.press("Enter");
  await expect(bar.getByRole("alert")).toContainText(
    "plain http:// addresses on this computer",
  );
  await address.fill("192.168.1.5:3000");
  await address.press("Enter");
  await expect(bar.getByRole("alert")).toContainText(
    "only opens servers on this computer",
  );
  // Nothing was asked of the engine for those.
  const opens = (await fakeLog(page)).filter(
    (r) => r.path === "/api/preview/open",
  );
  expect(opens.map((r) => r.body.url)).toEqual([
    "http://localhost:5173/settings",
  ]);
});
