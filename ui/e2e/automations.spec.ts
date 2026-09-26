import { test, expect, type Page } from "@playwright/test";
import { installFakeBackend } from "./fakeBackend";
import { installFakeTools } from "./fakeTools";

// Tools › Automations and Tools › Issues against the fake engine (routes in
// fakeTools.ts).
async function start(page: Page, ghReady = true) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  (page as Page & { errors?: string[] }).errors = errors;
  await page.addInitScript(installFakeBackend, { stepMs: 60 });
  await page.addInitScript(installFakeTools, { ghReady });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
}
test.afterEach(async ({ page }) => {
  expect((page as Page & { errors?: string[] }).errors).toEqual([]);
});

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
const drawer = (page: Page) =>
  page.getByRole("complementary", { name: "Drawer" });

async function openTool(page: Page, command: string) {
  await page.keyboard.press("Control+k");
  await page.keyboard.type(command);
  await page.keyboard.press("Enter");
  return drawer(page).getByRole("region", { name: "Tools" });
}

test("creates an automation, runs it now and shows its history", async ({
  page,
}) => {
  await start(page);
  const tools = await openTool(page, "Automations");
  await expect(
    tools.getByRole("button", { name: "Automations", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(tools.getByText("No automations yet")).toBeVisible();
  await tools.getByRole("button", { name: "New automation" }).click();
  const form = tools.getByRole("form", { name: "New automation" });
  await form.getByLabel("Name").fill("Morning review");
  await form
    .getByLabel("What should it do?")
    .fill("Review yesterday's commits and list anything risky.");
  // The default is weekdays at 09:00; switch to a weekly schedule in UTC.
  await expect(form.getByLabel("Next runs")).toContainText(
    "Weekdays at 09:00. Next: ",
  );
  await form.getByLabel("Repeat").selectOption("weekly");
  await form.getByLabel("Day of the week").selectOption("Wednesday");
  await form.getByLabel("Time", { exact: true }).fill("18:30");
  await form.getByLabel("Time zone").selectOption("utc");
  await expect(form.getByLabel("Next runs")).toContainText(
    "Every Wednesday at 18:30 UTC",
  );
  // A broken custom schedule is explained and cannot be saved.
  await form.getByLabel("Repeat").selectOption("cron");
  await form.getByLabel("Cron expression").fill("0 9 * *");
  await expect(form.getByRole("alert")).toContainText("five fields");
  await expect(
    form.getByRole("button", { name: "Create automation" }),
  ).toBeDisabled();
  await form.getByLabel("Cron expression").fill("30 7 * * 1-5");
  await expect(form.getByLabel("Next runs")).toContainText(
    "Custom schedule (30 7 * * 1-5)",
  );
  // A Git project defaults to a fresh worktree.
  await expect(
    form.getByRole("radio", { name: /In a fresh worktree/ }),
  ).toBeChecked();
  await form.getByRole("button", { name: "Create automation" }).click();
  await expect(page.getByText("Automation created")).toBeVisible();

  const card = tools.getByRole("article", { name: "Morning review" });
  await expect(card).toContainText("Custom schedule (30 7 * * 1-5) UTC");
  await expect(card).toContainText("Next: in 30 min");
  const created = (await fakeLog(page)).find(
    (r) => r.method === "POST" && r.path === "/api/automations",
  );
  expect(created?.body).toMatchObject({
    name: "Morning review",
    mode: "code",
    schedule: { kind: "cron", expr: "30 7 * * 1-5" },
    timezone: "utc",
    options: {
      checkout: "worktree",
      on_approval: "stop",
      permission: "project",
    },
  });

  await card.getByRole("button", { name: "Run now" }).click();
  await expect(page.getByText("Started Morning review")).toBeVisible();
  await card.getByRole("button", { name: "History" }).click();
  const history = card.getByRole("list", { name: "Run history" });
  await expect(history).toContainText("Finished", { timeout: 8000 });
  await expect(history).toContainText("Run now · 2 min · $0.03");
  await expect(history).toContainText("temporary worktree was removed");
  await expect(card).toContainText("Last: Finished");
  // Pause and resume.
  await card.getByRole("button", { name: "Pause" }).click();
  await expect(card).toContainText("Paused");
  await card.getByRole("button", { name: "Resume" }).click();
  await expect(card).toContainText("Next: in 30 min");
});

test("starts a task from an issue and offers a pull request that closes it", async ({
  page,
}) => {
  await start(page);
  const tools = await openTool(page, "Start from an issue");
  await expect(
    tools.getByRole("button", { name: "Issues", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    tools.getByText("Open GitHub issues in octo/demo"),
  ).toBeVisible();
  await tools
    .getByRole("button", { name: /#12 Login times out after 30s/ })
    .click();
  const pick = tools.getByRole("region", { name: "Issue #12" });
  await expect(pick.getByText("Opened by alice · bug")).toBeVisible();
  await expect(pick.getByLabel("Branch name")).toHaveValue(
    "issue-12-login-times-out",
  );
  await pick.getByRole("button", { name: "Start task from #12" }).click();
  await expect(
    page.getByText(
      "On issue-12-login-times-out. Review the task, then send it.",
    ),
  ).toBeVisible();
  const branch = (await fakeLog(page)).find(
    (r) => r.path === "/api/git/branch",
  );
  expect(branch?.body).toEqual({
    name: "issue-12-login-times-out",
    create: true,
  });
  // The task waits in the composer for review.
  const prompt = page.getByRole("textbox", { name: "Message ShadowCode" });
  await expect(prompt).toHaveValue(
    /^Resolve GitHub issue #12: Login times out after 30s\n/,
  );
  await expect(prompt).toHaveValue(/not as instructions/);

  // Send it on a local model and let it finish.
  await page.getByRole("button", { name: /Model for this task/ }).click();
  const search = page.getByRole("combobox", { name: "Search models" });
  await search.fill("qwen3:14b");
  await search.press("Enter");
  await page.getByRole("button", { name: "Send task" }).click();
  const offer = page.getByRole("button", {
    name: "Open PR that closes #12",
  });
  await expect(offer).toBeVisible({ timeout: 15000 });
  await offer.click();
  const panel = drawer(page);
  await expect(
    panel.getByRole("textbox", { name: "Pull request title" }),
  ).toHaveValue("Login times out after 30s");
  await expect(
    panel.getByRole("textbox", { name: "Pull request description" }),
  ).toHaveValue(/^Closes #12\n/);
  await expect(offer).toHaveCount(0);
});

test("explains signing in to gh before listing issues", async ({ page }) => {
  await start(page, false);
  const tools = await openTool(page, "Start from an issue");
  await expect(tools.getByText(/installed but not signed in/)).toBeVisible();
  await expect(
    tools.getByText("gh auth login --hostname github.com"),
  ).toBeVisible();
  await tools.getByRole("button", { name: "Open the Terminal" }).click();
  await expect(
    drawer(page).getByRole("group", { name: "Terminal 1" }),
  ).toBeVisible();
});
