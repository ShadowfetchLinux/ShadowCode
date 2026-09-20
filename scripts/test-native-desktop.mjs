// Real Tauri/WebKit window test. Requires a display (xvfb-run works), DBus,
// tauri-driver, and WebKitWebDriver. No Python service or browser launcher.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { mkdtemp, mkdir, readFile, writeFile, readlink, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const artifacts = path.join(root, "artifacts/native");
await mkdir(artifacts, { recursive: true });
for (const name of ["result.json", "failure.txt", "failure.png", "workspace-light.png", "workspace-dark.png", "command-approval.png", "task-complete.png", "compact.png", "webdriver.log", "accessibility-light.json", "accessibility-dark.json", "accessibility-compact.json"]) {
  await rm(path.join(artifacts, name), { force: true });
}
const axeSource = await readFile(path.join(root, "ui/node_modules/axe-core/axe.min.js"), "utf8");
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-window-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
await mkdir(project); await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(path.join(project, "README.md"), "# Native desktop test\nA disposable workspace.\n");
const delay = (ms) => new Promise(resolve => setTimeout(resolve, ms));
async function until(label, fn, timeout = 15000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try { const value = await fn(); if (value) return value; } catch (error) { last = error; }
    await delay(80);
  }
  throw new Error(`${label} timed out${last ? `: ${last}` : ""}`);
}
async function unusedPort() {
  const server = createServer();
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}
let requests = 0;
const sockets = new Set();
const model = createServer(async (req, res) => {
  let body = "";
  for await (const chunk of req) body += chunk;
  const payload = JSON.parse(body);
  assert.equal(payload.model, "native-fixture");
  const index = requests++;
  if (index >= 3) { res.writeHead(200, { "Content-Type": "application/json" }); res.flushHeaders(); return; }
  const tool = (name, args) => ({ id: `call-${index}`, type: "function", function: { name, arguments: JSON.stringify(args) } });
  const message = index === 0
    ? { role: "assistant", content: "Writing the file.", tool_calls: [tool("write_file", { path: "hello.txt", content: "native-window-ok\n", expected_hash: "missing" })] }
    : index === 1
      ? { role: "assistant", content: "Checking the result.", tool_calls: [tool("exec", { command: "test \"$(cat hello.txt)\" = native-window-ok && printf native-window-verified" })] }
      : { role: "assistant", content: "Created hello.txt and verified its contents." };
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ choices: [{ message, finish_reason: index < 2 ? "tool_calls" : "stop" }], usage: { prompt_tokens: 30, completion_tokens: 10, total_tokens: 40 } }));
});
model.on("connection", socket => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(profile, "config/config.yaml"), JSON.stringify({
  model: { default: "native-fixture", name: "native-fixture", provider: "local", endpoint: `http://127.0.0.1:${model.address().port}/v1`, context_limit: 16384 },
  onboarding: { completed: true, workspace: project }, trusted_workspaces: [project], ui: { theme: "light", notify: false },
}));

const port = await unusedPort(), nativePort = await unusedPort();
const output = createWriteStream(path.join(artifacts, "webdriver.log"));
const args = ["--port", String(port), "--native-port", String(nativePort)];
if (process.env.SHADOW_WEBKIT_DRIVER) args.push("--native-driver", process.env.SHADOW_WEBKIT_DRIVER);
const driver = spawn(process.env.SHADOW_TAURI_DRIVER || "tauri-driver", args, {
  detached: true, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, WEBKIT_DISABLE_DMABUF_RENDERER: "1" },
});
driver.stdout.pipe(output); driver.stderr.pipe(output);
let spawnError;
driver.on("error", error => { spawnError = error; });
let session;
async function wd(method, endpoint, body) {
  const response = await fetch(`http://127.0.0.1:${port}${endpoint}`, { method, headers: { "Content-Type": "application/json" }, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(30000) });
  const data = await response.json();
  if (!response.ok || data.value?.error) throw new Error(JSON.stringify(data.value));
  return data.value;
}
const execute = (script, args = []) => wd("POST", `/session/${session}/execute/sync`, { script, args });
async function native(command, args = {}) {
  const response = await wd("POST", `/session/${session}/execute/async`, {
    script: "const done=arguments[arguments.length-1]; window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(value=>done({value}),error=>done({error:String(error)}));", args: [command, args],
  });
  if (response.error) throw new Error(response.error);
  return response.value;
}
const api = (method, endpoint, body = null) => native("api", { request: { method, path: endpoint, body } });
async function element(selector) {
  return (await wd("POST", `/session/${session}/element`, { using: "css selector", value: selector }))["element-6066-11e4-a52e-4f735466cecf"];
}
async function click(selector) {
  await wd("POST", `/session/${session}/element/${await element(selector)}/click`, {});
}
async function type(selector, text) {
  await wd("POST", `/session/${session}/element/${await element(selector)}/value`, { text });
}
async function screenshot(name) {
  await writeFile(path.join(artifacts, `${name}.png`), Buffer.from(await wd("GET", `/session/${session}/screenshot`), "base64"));
}
async function accessibility(name) {
  const report = await wd("POST", `/session/${session}/execute/async`, {
    script: `${axeSource}\nconst done=arguments[arguments.length-1];window.axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}}).then(result=>done({violations:result.violations}),error=>done({error:String(error)}));`, args: [],
  });
  await writeFile(path.join(artifacts, `accessibility-${name}.json`), JSON.stringify(report, null, 2));
  assert.equal(report.error, undefined);
  assert.deepEqual(report.violations, [], `${name} native accessibility`);
}
try {
  await until("WebDriver startup", async () => { if (spawnError) throw spawnError; return wd("GET", "/status"); });
  const created = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: binary, args: ["--profile", profile, "--workspace", project] } } } });
  session = created.sessionId;
  await wd("POST", `/session/${session}/timeouts`, { script: 20000, implicit: 0, pageLoad: 30000 });
  await until("Native workspace", () => execute("return !!document.querySelector('textarea[aria-label=\"Message ShadowCode\"]') && !document.querySelector('textarea[aria-label=\"Message ShadowCode\"]').disabled;"), 25000);
  const version = await api("GET", "/api/version");
  assert.equal(version.runtime, "rust"); assert.equal(version.transport, "native");
  const expectedScript = (await readFile(path.join(root,"ui/dist/index.html"),"utf8")).match(/src="([^"]+\.js)"/)[1];
  assert.equal(await execute("return new URL(document.querySelector('script[type=module]').src).pathname"), expectedScript, "Desktop binary must embed the current compiled interface");
  assert.equal(path.basename(await readlink(`/proc/${version.pid}/exe`)), "shadowcode");
  assert.match(await execute("return location.href"), /^(tauri:\/\/localhost|https?:\/\/tauri.localhost)/);
  assert.equal((await readFile(`/proc/${version.pid}/maps`, "utf8")).includes("libpython"), false);
  await screenshot("workspace-light");
  await accessibility("light");
  await execute("document.documentElement.dataset.theme='dark'"); await screenshot("workspace-dark");
  await accessibility("dark");
  await execute("document.documentElement.dataset.theme='light'");
  await type('textarea[aria-label="Message ShadowCode"]', "Create hello.txt containing native-window-ok and verify its contents with the terminal.");
  await click('button[aria-label="Send task"]');
  await until("Command approval", () => execute("return !!document.querySelector('.approval button.primary')"));
  await screenshot("command-approval");
  await click(".approval button.primary");
  await until("Verified task", async () => {
    const { jobs } = await api("GET", "/api/jobs");
    assert.notEqual(jobs[0]?.status, "failed", jobs[0]?.summary);
    return jobs[0]?.status === "completed";
  });
  assert.equal(await readFile(path.join(project, "hello.txt"), "utf8"), "native-window-ok\n");
  await until("Completion in transcript", () => execute("return [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('Created hello.txt and verified')) && !document.querySelector('button[aria-label=\"Stop task\"]');"));
  assert.equal(await execute("return document.querySelectorAll('.msg-user').length"), 1);
  assert.equal(await execute("return [...document.querySelectorAll('.msg-agent')].filter(e=>e.textContent.includes('Created hello.txt and verified')).length"), 1);
  await screenshot("task-complete");
  await wd("POST", `/session/${session}/refresh`, {});
  await until("Persisted conversation", () => execute("return document.querySelectorAll('.msg-user').length===1 && [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('Created hello.txt and verified'));"));
  await type('textarea[aria-label="Message ShadowCode"]', "Explain the result again.");
  await click('button[aria-label="Send task"]');
  await until("Second model request", () => requests >= 4);
  await click('button[aria-label="Stop task"]');
  await until("Cancellation persisted", async () => (await api("GET", "/api/jobs")).jobs[0].status === "cancelled");
  await until("Cancellation visible", () => execute("return [...document.querySelectorAll('.msg-agent')].some(e=>/cancelled/i.test(e.textContent));"));
  await wd("POST", `/session/${session}/window/rect`, { width: 620, height: 850 });
  await until("Compact sidebar collapsed", () => execute("return !document.querySelector('.sidebar')"));
  await screenshot("compact");
  await accessibility("compact");
  assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"), true);
  // A terminal command is still running when the actual native quit command is
  // invoked. The process and its child must be gone before shutdown completes.
  await execute("window.__TAURI_INTERNALS__.invoke('api',{request:{method:'POST',path:'/api/workspace/exec',body:{command:'sleep 60 & echo $! > child.pid; wait',timeout:120}}}).catch(()=>{});return true;");
  const child = await until("Terminal child", async () => (await readFile(path.join(project, "child.pid"), "utf8")).trim());
  await execute("setTimeout(()=>window.__TAURI_INTERNALS__.invoke('desktop_quit'),30);return true;");
  const dead = async (pid) => {
    try { return /\) [ZX] /.test(await readFile(`/proc/${pid}/stat`, "utf8")); } catch { return true; }
  };
  await until("Native shutdown", () => dead(version.pid));
  await until("Terminal cleanup", () => dead(child));
  await writeFile(path.join(artifacts, "result.json"), JSON.stringify({ passed: true, version: version.version, runtime: version.runtime, modelRequests: requests, checks: ["embedded interface", "Rust IPC", "native approval", "real file write and terminal verification", "durable reload", "cancellation", "compact layout", "native light/dark/compact accessibility", "managed native shutdown"] }, null, 2));
  console.log("Native desktop window passed: IPC, approval, file/terminal tools, replay, cancellation, layout, shutdown.");
} catch (error) {
  if (session) {
    await screenshot("failure").catch(() => {});
    await execute("return document.body.innerText").then(text => writeFile(path.join(artifacts, "failure.txt"), text)).catch(() => {});
  }
  throw error;
} finally {
  if (session) await wd("DELETE", `/session/${session}`).catch(() => {});
  try { process.kill(-driver.pid, "SIGTERM"); } catch { /* Already exited. */ }
  for (const socket of sockets) socket.destroy();
  model.close();
  await delay(300);
  try { process.kill(-driver.pid, "SIGKILL"); } catch { /* Already exited. */ }
  output.end();
  await rm(scratch, { recursive: true, force: true });
}
