// Manual release probe against an installed Ollama model, through the actual
// native window. Everything it can edit is in a disposable project/profile.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { execFile, spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import {
  mkdir,
  mkdtemp,
  readFile,
  readlink,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const root = fileURLToPath(new URL("../", import.meta.url));
const model = process.argv[2] || "gpt-oss:20b";
const binary =
  process.env.SHADOW_DESKTOP_BINARY ||
  path.join(root, "target/release/shadowcode");
const binaryArgs = JSON.parse(process.env.SHADOW_DESKTOP_ARGS || "[]");
assert.ok(
  Array.isArray(binaryArgs) &&
    binaryArgs.every((arg) => typeof arg === "string"),
);
const artifacts = path.resolve(
  process.env.SHADOW_NATIVE_ARTIFACTS ||
    path.join(
      root,
      "artifacts/native-model",
      model.replace(/[^a-z0-9._-]/gi, "_"),
    ),
);
await mkdir(artifacts, { recursive: true });
for (const file of [
  "result.json",
  "failure.txt",
  "failure.png",
  "coding.png",
  "continuation.png",
  "cancelled.png",
  "jobs.json",
  "conversation.json",
  "independent-test.txt",
])
  await rm(path.join(artifacts, file), { force: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-real-window-"));
const project = path.join(scratch, "project"),
  profile = path.join(scratch, "profile");
await mkdir(path.join(project, "src"), { recursive: true });
await mkdir(path.join(profile, "config"), { recursive: true });
const manifest =
  '[package]\nname = "native-window-probe"\nversion = "0.1.0"\nedition = "2021"\n';
const original =
  "pub fn add(a: i32, b: i32) -> i32 { a - b }\n\n#[cfg(test)]\nmod tests {\n    #[test] fn adds() { assert_eq!(super::add(2, 3), 5); assert_eq!(super::add(-2, 1), -1); }\n}\n";
await writeFile(path.join(project, "Cargo.toml"), manifest);
await writeFile(path.join(project, "src/lib.rs"), original);
await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: {
      default: model,
      name: model,
      provider: "ollama",
      endpoint: "http://127.0.0.1:11434/v1",
      api_key_env: "SHADOWCODE_PROBE_API_KEY",
      context_limit: 8192,
    },
    agent: { max_steps: 16, max_task_tokens: 120000, tool_timeout_sec: 60 },
    onboarding: { completed: true, workspace: project },
    trusted_workspaces: [project],
    ui: { theme: "light", notify: false },
  }),
);
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(label, test, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await test();
    if (value) return value;
    await delay(150);
  }
  throw new Error(`${label} timed out`);
}
async function port() {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const value = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return value;
}
const driverPort = await port(),
  nativePort = await port();
const output = createWriteStream(path.join(artifacts, "webdriver.log"));
const driverArgs = [
  "--port",
  String(driverPort),
  "--native-port",
  String(nativePort),
];
if (process.env.SHADOW_WEBKIT_DRIVER)
  driverArgs.push("--native-driver", process.env.SHADOW_WEBKIT_DRIVER);
const driver = spawn(
  process.env.SHADOW_TAURI_DRIVER || "tauri-driver",
  driverArgs,
  {
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, WEBKIT_DISABLE_DMABUF_RENDERER: "1" },
  },
);
driver.stdout.pipe(output);
driver.stderr.pipe(output);
let spawnError, session, appPid;
driver.on("error", (error) => {
  spawnError = error;
});
async function wd(method, route, body) {
  const response = await fetch(`http://127.0.0.1:${driverPort}${route}`, {
    method,
    headers: { "Content-Type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(30000),
  });
  const data = await response.json();
  if (!response.ok || data.value?.error)
    throw new Error(JSON.stringify(data.value));
  return data.value;
}
const execute = (script, args = []) =>
  wd("POST", `/session/${session}/execute/sync`, { script, args });
async function native(command, args = {}) {
  const result = await wd("POST", `/session/${session}/execute/async`, {
    script:
      "const done=arguments[arguments.length-1];window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(value=>done({value}),error=>done({error:String(error)}));",
    args: [command, args],
  });
  if (result.error) throw new Error(result.error);
  return result.value;
}
const api = (method, route, body = null) =>
  native("api", { request: { method, path: route, body } });
async function element(selector) {
  return (
    await wd("POST", `/session/${session}/element`, {
      using: "css selector",
      value: selector,
    })
  )["element-6066-11e4-a52e-4f735466cecf"];
}
async function click(selector) {
  await wd(
    "POST",
    `/session/${session}/element/${await element(selector)}/click`,
    {},
  );
}
async function screenshot(name) {
  await writeFile(
    path.join(artifacts, `${name}.png`),
    Buffer.from(await wd("GET", `/session/${session}/screenshot`), "base64"),
  );
}
async function submit(prompt) {
  const old = new Set(
    (await api("GET", "/api/jobs")).jobs.map((job) => job.id),
  );
  await until("Ready composer", () =>
    execute(
      "return !!document.querySelector('textarea[aria-label=\"Message ShadowCode\"]') && !document.querySelector('textarea[aria-label=\"Message ShadowCode\"]').disabled && !document.querySelector('button[aria-label=\"Stop task\"]');",
    ),
  );
  await wd(
    "POST",
    `/session/${session}/element/${await element('textarea[aria-label="Message ShadowCode"]')}/value`,
    { text: prompt },
  );
  await click('button[aria-label="Send task"]');
  return until("New native task", async () =>
    (await api("GET", "/api/jobs")).jobs.find((job) => !old.has(job.id)),
  );
}
const decisions = [];
async function complete(job) {
  let cursor = job.event_cursor;
  return until(
    "Local-model task",
    async () => {
      const page = await api(
        "GET",
        `/api/jobs/${job.id}/events?after=${cursor}`,
      );
      for (const event of page.events) {
        cursor = event.id;
        if (
          ["tool.started", "tool.completed", "model.retry"].includes(event.type)
        )
          console.log(event.type, JSON.stringify(event.payload));
      }
      for (const approval of (
        await api("GET", `/api/approvals?session_id=${job.session_id}`)
      ).approvals) {
        const args = approval.arguments;
        const allow =
          approval.tool === "exec" &&
          args.command === "cargo test --offline --lib" &&
          [undefined, "", ".", project].includes(args.cwd);
        decisions.push({
          tool: approval.tool,
          arguments: args,
          allowed: allow,
        });
        console.log("approval", allow, approval.command);
        const selector = `[data-approval-id="${approval.id}"] button.${allow ? "primary" : "secondary"}`;
        if (allow) {
          await until("Visible terminal approval", () =>
            execute("return !!document.querySelector(arguments[0])", [
              selector,
            ]),
          );
          await click(selector);
        } else
          await api("POST", `/api/approvals/${approval.id}`, {
            session_id: job.session_id,
            decision: "deny",
          });
      }
      const current = page.job;
      if (["failed", "cancelled", "interrupted"].includes(current.status))
        throw new Error(`${current.status}: ${current.summary}`);
      return current.status === "completed" ? current : false;
    },
    300000,
  );
}
const dead = async (pid) => {
  try {
    return /\) [ZX] /.test(await readFile(`/proc/${pid}/stat`, "utf8"));
  } catch {
    return true;
  }
};
const began = Date.now();
try {
  await until("WebDriver startup", async () => {
    if (spawnError) throw spawnError;
    try {
      return await wd("GET", "/status");
    } catch {
      return false;
    }
  });
  session = (
    await wd("POST", "/session", {
      capabilities: {
        alwaysMatch: {
          "tauri:options": {
            application: binary,
            args: [...binaryArgs, "--profile", profile, "--workspace", project],
          },
        },
      },
    })
  ).sessionId;
  await wd("POST", `/session/${session}/timeouts`, {
    script: 20000,
    implicit: 0,
    pageLoad: 30000,
  });
  await until("Native workspace", () =>
    execute(
      "return !!document.querySelector('textarea[aria-label=\"Message ShadowCode\"]')",
    ),
  );
  const version = await api("GET", "/api/version");
  appPid = version.pid;
  assert.equal(version.runtime, "rust");
  assert.equal(
    path.basename(await readlink(`/proc/${appPid}/exe`)),
    "shadowcode",
  );
  assert.equal(
    (await readFile(`/proc/${appPid}/maps`, "utf8")).includes("libpython"),
    false,
  );
  const first = await submit(
    "Fix the add function in src/lib.rs so it adds its two inputs. Inspect the file using tools, make the smallest change, then run exactly `cargo test --offline --lib` using exec with no cwd parameter. Preserve the existing tests and package files. Finish only after seeing the test result.",
  );
  const fixed = await complete(first);
  assert.equal(
    await readFile(path.join(project, "src/lib.rs"), "utf8"),
    original.replace("a - b", "a + b"),
  );
  assert.equal(
    await readFile(path.join(project, "Cargo.toml"), "utf8"),
    manifest,
  );
  const evidence = await api("GET", `/api/sessions/${first.session_id}`);
  assert.ok(
    evidence.events.some(
      (event) =>
        event.type === "tool.completed" &&
        event.payload.tool === "exec" &&
        event.payload.success === true,
    ),
  );
  const verified = await promisify(execFile)(
    "cargo",
    ["test", "--offline", "--lib"],
    { cwd: project, timeout: 60000 },
  );
  await writeFile(
    path.join(artifacts, "independent-test.txt"),
    verified.stdout + verified.stderr,
  );
  await until("Finished transcript", () =>
    execute(
      "return !document.querySelector('button[aria-label=\"Stop task\"]') && document.querySelectorAll('.msg-agent').length>0",
    ),
  );
  await screenshot("coding");
  await wd("POST", `/session/${session}/refresh`, {});
  await until("Saved conversation", () =>
    execute(
      "return document.querySelectorAll('.msg-user').length===1 && document.querySelectorAll('.msg-agent').length>0",
    ),
  );
  await click('select[aria-label="Agent mode"] option[value="reviewer"]');
  const second = await submit(
    "Read src/lib.rs again and tell me what add(-2, 1) now returns. Do not change files or run commands.",
  );
  const continued = await complete(second);
  assert.equal(continued.mode, "review");
  assert.equal(continued.session_id, fixed.session_id);
  assert.ok(continued.summary.includes("-1"));
  const continuedEvidence = await api(
    "GET",
    `/api/sessions/${first.session_id}`,
  );
  assert.ok(
    continuedEvidence.events.some(
      (event) =>
        event.task_id === second.task_id &&
        event.type === "tool.completed" &&
        event.payload.tool === "read_file" &&
        event.payload.success === true,
    ),
  );
  assert.equal(
    await readFile(path.join(project, "src/lib.rs"), "utf8"),
    original.replace("a - b", "a + b"),
  );
  await screenshot("continuation");
  const third = await submit(
    "Write a detailed explanation of Rust integer addition and ownership, at least 2500 words. Only write prose. Do not run tools or modify files.",
  );
  await until(
    "Actual model stream",
    async () => {
      const page = await api(
        "GET",
        `/api/jobs/${third.id}/events?after=${third.event_cursor}`,
      );
      assert.ok(
        !["completed", "failed", "cancelled"].includes(page.job.status),
        "Probe needs an in-flight stream to exercise Stop",
      );
      return page.events.some(
        (event) =>
          event.task_id === third.task_id && event.type === "model.stream",
      );
    },
    180000,
  );
  const cancelBegan = Date.now();
  await click('button[aria-label="Stop task"]');
  await until(
    "Durable cancellation",
    async () =>
      (await api("GET", `/api/jobs/${third.id}`)).status === "cancelled",
  );
  await until("Visible cancellation", () =>
    execute(
      "return [...document.querySelectorAll('.msg-agent')].some(node=>/cancelled/i.test(node.textContent)) && !document.querySelector('button[aria-label=\"Stop task\"]')",
    ),
  );
  const cancellationMs = Date.now() - cancelBegan;
  await screenshot("cancelled");
  await api("POST", `/api/checkpoints/tasks/${first.task_id}/restore`, {});
  assert.equal(
    await readFile(path.join(project, "src/lib.rs"), "utf8"),
    original,
  );
  await writeFile(
    path.join(artifacts, "conversation.json"),
    JSON.stringify(
      await api("GET", `/api/sessions/${first.session_id}`),
      null,
      2,
    ),
  );
  await execute(
    "setTimeout(()=>window.__TAURI_INTERNALS__.invoke('desktop_quit'),30);return true;",
  );
  await until("Native process shutdown", () => dead(appPid));
  const result = {
    passed: true,
    model,
    binary,
    version: version.version,
    durationMs: Date.now() - began,
    cancellationMs,
    steps: fixed.steps + continued.steps,
    usage: { coding: fixed.usage, continuation: continued.usage },
    decisions,
    checks: [
      "native-window submission to Ollama",
      "real minimal file edit",
      "visible scoped terminal approval",
      "independent unchanged tests",
      "reload and read-only continuation with fresh file read",
      "Stop during actual model streaming",
      "byte-exact checkpoint rewind",
      "native process shutdown",
    ],
  };
  await writeFile(
    path.join(artifacts, "result.json"),
    JSON.stringify(result, null, 2),
  );
  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  if (session) {
    await screenshot("failure").catch(() => {});
    await execute("return document.body.innerText")
      .then((text) => writeFile(path.join(artifacts, "failure.txt"), text))
      .catch(() => {});
    await api("GET", "/api/jobs")
      .then((value) =>
        writeFile(
          path.join(artifacts, "jobs.json"),
          JSON.stringify(value, null, 2),
        ),
      )
      .catch(() => {});
  }
  throw error;
} finally {
  if (session) {
    await execute(
      "setTimeout(()=>window.__TAURI_INTERNALS__.invoke('desktop_quit'),30);return true;",
    ).catch(() => {});
    if (appPid)
      await until("Probe cleanup", () => dead(appPid), 10000).catch(() => {});
    await wd("DELETE", `/session/${session}`).catch(() => {});
  }
  try {
    process.kill(-driver.pid, "SIGTERM");
  } catch {
    /* Already exited. */
  }
  await delay(300);
  try {
    process.kill(-driver.pid, "SIGKILL");
  } catch {
    /* Already exited. */
  }
  output.end();
  await rm(scratch, { recursive: true, force: true });
}
