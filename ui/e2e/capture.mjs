// Run manually against scripts/serve-test-ui.py for sanitized documentation images.
import { chromium } from "@playwright/test";
import { mkdir } from "node:fs/promises";
const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 1440, height: 960 },
  deviceScaleFactor: 1,
});
await page.goto("http://127.0.0.1:17430");
await page.getByRole("heading", { name: "What are we building?" }).waitFor();
await mkdir("../docs/images", { recursive: true });
await page.screenshot({ path: "../docs/images/workspace-light.png" });
await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
await page.screenshot({ path: "../docs/images/workspace-dark.png" });
await page
  .getByRole("textbox", { name: "Message ShadowCode" })
  .fill("Create a Python hello-world project and run it");
await page.getByRole("button", { name: "Send task", exact: true }).click();
await page
  .getByText("Created hello.py, ran it, and verified output: Hello, World!", {
    exact: true,
  })
  .waitFor();
await page.getByText("4 of 4", { exact: true }).waitFor();
await page.screenshot({ path: "../docs/images/task-complete.png" });
await browser.close();
