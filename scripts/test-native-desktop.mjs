// Real Tauri/WebKit window test. Requires a display (xvfb-run works), DBus,
// tauri-driver, and WebKitWebDriver. No Python service or browser launcher.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { createWriteStream } from "node:fs";
import { mkdtemp, mkdir, readFile, writeFile, readlink, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const binaryArgs = JSON.parse(process.env.SHADOW_DESKTOP_ARGS || "[]");
assert.ok(Array.isArray(binaryArgs) && binaryArgs.every(arg => typeof arg === "string"), "SHADOW_DESKTOP_ARGS must be a JSON array of strings");
const artifacts = process.env.SHADOW_NATIVE_ARTIFACTS || path.join(root, "artifacts/native");
await mkdir(artifacts, { recursive: true });
for (const name of ["result.json", "failure.txt", "failure.png", "workspace-light.png", "workspace-dark.png", "command-approval.png", "task-complete.png", "compact.png", "goals.png", "routing.png", "background.png", "webdriver.log", "accessibility-light.json", "accessibility-dark.json", "accessibility-compact.json", "accessibility-goals.json", "accessibility-routing.json", "accessibility-background.json", "skills.png", "accessibility-skills.json"]) {
  await rm(path.join(artifacts, name), { force: true });
}
const axeSource = await readFile(path.join(root, "ui/node_modules/axe-core/axe.min.js"), "utf8");
for (const name of ["hooks.png", "accessibility-hooks.json"]) await rm(path.join(artifacts, name), { force: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-window-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
const defaultProfile = process.env.SHADOW_NATIVE_DEFAULT_PROFILE === "1";
const profileArgs = defaultProfile ? [] : ["--profile",profile];
const configDirectory = path.join(profile,defaultProfile ? "config/shadow-agent" : "config");
const stateDirectory = path.join(profile,defaultProfile ? "state/shadow-agent" : "state");
const nativeEnv = {...process.env, TMPDIR: path.join(scratch,"images"), ...(defaultProfile ? {
  XDG_CONFIG_HOME: path.join(profile,"config"), XDG_DATA_HOME: path.join(profile,"data"), XDG_STATE_HOME: path.join(profile,"state"),
} : {})};
delete nativeEnv.NO_CLEANUP;
await mkdir(nativeEnv.TMPDIR);
await mkdir(project); await mkdir(configDirectory, { recursive: true });
await writeFile(path.join(project, "README.md"), "# Native desktop test\nA disposable workspace.\n");
await mkdir(path.join(project, ".shadowcode/hooks"), { recursive: true });
await writeFile(path.join(project, ".shadowcode/hooks/verify.json"), JSON.stringify({
  name: "verify-result", events: ["on_complete"], timeout_sec: 10,
  description: "Verify the completed change before the task succeeds.",
  command: "test \"$(cat hello.txt)\" = native-window-ok && printf native-hook-ok > hook-result.txt && printf native-hook-check-passed",
}));
let modelError;
const delay = (ms) => new Promise(resolve => setTimeout(resolve, ms));
async function until(label, fn, timeout = 15000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    if (modelError) throw modelError;
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
const requestedModels = [];
let goalMode = false;
let workflowMode = false;
let workflowCalls = 0;
const milestoneCalls = new Map();
const sockets = new Set();
const model = createServer(async (req, res) => {
  try {
  let body = "";
  for await (const chunk of req) body += chunk;
  const payload = JSON.parse(body);
  assert.ok(["native-fixture", "native-build"].includes(payload.model));
  requestedModels.push(payload.model);
  const index = requests++;
  const tool = (name, args) => ({ id: `call-${index}`, type: "function", function: { name, arguments: JSON.stringify(args) } });
  if (workflowMode) {
    const system = payload.messages.find(m => m.role === "system").content;
    assert.ok(system.includes("WINDOW_SKILL: inspect README.md"));
    assert.ok(payload.tools.every(t => !["write_file", "exec"].includes(t.function.name)), "Selected review skill must have read-only tools");
    const first = workflowCalls++ === 0;
    const message = first ? {role: "assistant", content: "Inspecting selected skill context.", tool_calls: [tool("read_file", {path: "README.md"})]}
      : {role: "assistant", content: "Selected skill reviewed README.md."};
    res.writeHead(200, {"Content-Type": "application/json"});
    res.end(JSON.stringify({choices: [{message, finish_reason: first ? "tool_calls" : "stop"}], usage: {prompt_tokens: 30, completion_tokens: 10, total_tokens: 40}}));
    return;
  }
  if (goalMode) {
    const task = payload.messages.filter(m => m.role === "user").at(-1).content;
    const milestone = task.split("\n")[0];
    const count = milestoneCalls.get(milestone) || 0;
    milestoneCalls.set(milestone, count + 1);
    const call = milestone.startsWith("Inspect") ? tool("read_file", { path: "README.md" })
      : milestone.startsWith("Implement") ? tool("write_file", { path: "goal.txt", content: "goal-native-ok\n", expected_hash: "missing" })
      : tool("exec", { command: "test \"$(cat goal.txt)\" = goal-native-ok" });
    const message = count === 0 ? { role: "assistant", content: milestone, tool_calls: [call] }
      : { role: "assistant", content: `Milestone complete: ${milestone}.` };
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ choices: [{ message, finish_reason: count === 0 ? "tool_calls" : "stop" }], usage: { prompt_tokens: 30, completion_tokens: 10, total_tokens: 40 } }));
    return;
  }
  if (index >= 3) { res.writeHead(200, { "Content-Type": "application/json" }); res.flushHeaders(); return; }
  const message = index === 0
    ? { role: "assistant", content: "Writing the file.", tool_calls: [tool("write_file", { path: "hello.txt", content: "native-window-ok\n", expected_hash: "missing" })] }
    : index === 1
      ? { role: "assistant", content: "Checking the result.", tool_calls: [tool("exec", { command: "test \"$(cat hello.txt)\" = native-window-ok && printf native-window-verified" })] }
      : { role: "assistant", content: "Created hello.txt and verified its contents." };
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ choices: [{ message, finish_reason: index < 2 ? "tool_calls" : "stop" }], usage: { prompt_tokens: 30, completion_tokens: 10, total_tokens: 40 } }));
  } catch (error) {
    modelError = error;
    res.writeHead(500, {"Content-Type": "application/json"});
    res.end(JSON.stringify({error: String(error)}));
  }
});
model.on("connection", socket => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(configDirectory, "config.yaml"), JSON.stringify({
  model: { default: "native-fixture", name: "native-fixture", provider: "local", endpoint: `http://127.0.0.1:${model.address().port}/v1`, context_limit: 16384 },
  onboarding: { completed: true, workspace: project }, trusted_workspaces: [project], ui: { theme: "light", notify: false },
}));

const port = await unusedPort(), nativePort = await unusedPort();
const output = createWriteStream(path.join(artifacts, "webdriver.log"));
const args = ["--port", String(port), "--native-port", String(nativePort)];
if (process.env.SHADOW_WEBKIT_DRIVER) args.push("--native-driver", process.env.SHADOW_WEBKIT_DRIVER);
const driver = spawn(process.env.SHADOW_TAURI_DRIVER || "tauri-driver", args, {
  detached: true, stdio: ["ignore", "pipe", "pipe"], env: { ...nativeEnv, WEBKIT_DISABLE_DMABUF_RENDERER: "1" },
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
async function clickButton(text) {
  const found = await wd("POST", `/session/${session}/element`, { using: "xpath", value: `//button[normalize-space(.)='${text}']` });
  await wd("POST", `/session/${session}/element/${found["element-6066-11e4-a52e-4f735466cecf"]}/click`, {});
}
async function type(selector, text) {
  await wd("POST", `/session/${session}/element/${await element(selector)}/value`, { text: text.replaceAll("\n", "\uE006") });
}
async function fill(selector, text) {
  await click(selector);
  // WebKit's element-clear may skip the input event React needs. Clear through
  // the browser's native value setter and emit input, then type with WebDriver.
  await execute("const el=document.querySelector(arguments[0]);const prototype=el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(prototype,'value').set.call(el,'');el.dispatchEvent(new Event('input',{bubbles:true}));", [selector]);
  await type(selector, text);
  assert.equal(await execute("return document.querySelector(arguments[0]).value", [selector]), text);
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
  const created = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: binary, args: [...binaryArgs, ...profileArgs, "--workspace", project] } } } });
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
  if (defaultProfile) {
    // Run under a private DBus session and disposable XDG roots. Repeated
    // activation must reuse this real window and preserve its extracted files.
    await Promise.all(Array.from({length:3},()=>promisify(execFile)(binary,[...binaryArgs,"--workspace",project],{env:nativeEnv,timeout:20000,maxBuffer:2_000_000})));
    assert.equal(path.basename(await readlink(`/proc/${version.pid}/exe`)),"shadowcode","Activation must not unlink the running executable");
    assert.equal((await api("GET","/api/version")).pid,version.pid);
    await wd("POST",`/session/${session}/refresh`,{});
    await until("Workspace after repeated activation",()=>execute("return !!document.querySelector('textarea[aria-label=\"Message ShadowCode\"]') && !document.querySelector('textarea[aria-label=\"Message ShadowCode\"]').disabled;"),25000);
    if (binaryArgs.includes("--appimage-extract-and-run")) assert.equal((await readdir(nativeEnv.TMPDIR)).filter(name=>name.startsWith("appimage_extracted_")).length,1,"Only the live window's extraction remains");
  }
  await screenshot("workspace-light");
  await accessibility("light");
  await execute("document.documentElement.dataset.theme='dark'"); await screenshot("workspace-dark");
  await accessibility("dark");
  await execute("document.documentElement.dataset.theme='light'");
  const registered = await api("POST", "/api/models/register", { provider: "local", endpoint: `http://127.0.0.1:${model.address().port}/v1`, name: "native-build" });
  await click('button[aria-label="Terminal"]');
  await clickButton("Health");
  await until("Native routing controls", () => execute("return !!document.querySelector('#routing-coder')"));
  await execute("const select=document.querySelector('#routing-coder');select.value=arguments[0];select.dispatchEvent(new Event('change',{bubbles:true}));", [registered.model.default]);
  await until("Build route saved", async () => (await api("GET", "/api/routing")).config.coder === registered.model.default);
  await until("Router ready", () => execute("return !document.querySelector('#routing-coder').disabled"));
  await clickButton("Enable routing");
  await until("Routing enabled", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Disable routing' && !e.disabled)"));
  await screenshot("routing");
  await accessibility("routing");
  await click('button.drawer-close');
  assert.match(await execute("return document.querySelector('select[aria-label=\"Model for this task\"]').selectedOptions[0].textContent"), /Automatic/);
  await click('button[aria-label="Terminal"]');
  await clickButton("Background");
  assert.equal(await execute("return document.querySelector('#background-name').value"), "dev");
  await type('#background-command', "printf 'native-background-ready\\n'; trap 'printf graceful-stop; exit 0' TERM; while :; do sleep 1; done");
  await clickButton("Start process");
  const background = await until("Background process output", async () => {
    const task = (await api("GET", "/api/background")).tasks[0];
    return task?.status === "RUNNING" && task.output.includes("native-background-ready") && task;
  });
  await until("Background output in drawer", () => execute("return !!document.querySelector('.background-output pre')?.textContent.includes('native-background-ready')"));
  await clickButton("Read retained output");
  await until("Retained background output", () => execute("return !!document.querySelector('.background-output summary')?.textContent.includes('Retained output snapshot')"));
  await clickButton("Return to live output");
  await screenshot("background");
  await accessibility("background");
  await click('button.drawer-close');
  await execute("[...document.querySelectorAll('.sidebar button')].find(e=>e.querySelector('span')?.textContent==='Settings').click()");
  await clickButton("Hooks");
  await until("Hook definition in Settings", () => execute("return !!document.querySelector('.hook-command')?.textContent.includes('native-hook-check-passed')"));
  await clickButton("Refresh hooks");
  await until("Hook refresh completed", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Enable verify-result' && !e.disabled)"));
  await clickButton("Enable verify-result");
  await until("Reviewed hook enabled", async () => (await api("GET", "/api/hooks")).hooks[0].enabled);
  await screenshot("hooks");
  await accessibility("hooks");
  await clickButton("Close");
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
  assert.equal(await readFile(path.join(project, "hook-result.txt"), "utf8"), "native-hook-ok");
  assert.deepEqual(requestedModels, ["native-build", "native-build", "native-build"]);
  await until("Selected model visible", () => execute("return [...document.querySelectorAll('.msg-note')].some(e=>e.textContent.includes('native-build · local · coder'))"));
  await until("Completion in transcript", () => execute("return [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('Created hello.txt and verified')) && !document.querySelector('button[aria-label=\"Stop task\"]');"));
  assert.equal(await execute("return document.querySelectorAll('.msg-user').length"), 1);
  assert.equal(await execute("return [...document.querySelectorAll('.msg-agent')].filter(e=>e.textContent.includes('Created hello.txt and verified')).length"), 1);
  await screenshot("task-complete");
  await wd("POST", `/session/${session}/refresh`, {});
  await until("Persisted conversation", () => execute("return document.querySelectorAll('.msg-user').length===1 && [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('Created hello.txt and verified'));"));
  assert.equal(await execute("return [...document.querySelectorAll('.msg-note')].filter(e=>e.textContent.includes('native-build')).length"), 1);
  await until("One persisted hook result", () => execute("return [...document.querySelectorAll('.op-card')].filter(e=>e.textContent.includes('Hook · verify-result')).length===1"));
  await execute("[...document.querySelectorAll('.sidebar button')].find(e=>e.querySelector('span')?.textContent==='Settings').click()");
  await clickButton("Hooks");
  await until("Hook disable control", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Disable verify-result')"));
  await clickButton("Disable verify-result");
  await until("Hook disabled", async () => !(await api("GET", "/api/hooks")).hooks[0].enabled);
  await clickButton("Close");
  // A missing model in an older saved configuration must be visible when the
  // engine chooses the default, including after the conversation is reloaded.
  await api("PUT", "/api/config", { values: { routing: { coder: "removed-native-model" } } });
  await type('textarea[aria-label="Message ShadowCode"]', "Explain the result again.");
  await click('button[aria-label="Send task"]');
  await until("Second model request", () => requests >= 4);
  await until("Fallback visible", () => execute("return [...document.querySelectorAll('.msg-note.warning')].some(e=>e.textContent.includes('Using default: native-fixture'))"));
  assert.equal(requestedModels.at(-1), "native-fixture");
  await click('button[aria-label="Stop task"]');
  await until("Cancellation persisted", async () => (await api("GET", "/api/jobs")).jobs[0].status === "cancelled");
  await until("Cancellation visible", () => execute("return [...document.querySelectorAll('.msg-agent')].some(e=>/cancelled/i.test(e.textContent));"));
  await api("PUT", "/api/routing", { values: { enabled: false } });
  assert.equal((await api("GET", `/api/background/${background.id}`)).status, "RUNNING");
  await click('button[aria-label="Terminal"]');
  await clickButton("Background");
  await until("Background stop control", () => execute("return !!document.querySelector('button[aria-label=\"Stop dev\"]')"));
  await click('button[aria-label="Stop dev"]');
  await until("Background cancellation persisted", async () => {
    const task = await api("GET", `/api/background/${background.id}`);
    return task.status === "CANCELLED" && task.output.includes("graceful-stop");
  });
  await until("Background cancellation visible", () => execute("return !!document.querySelector('.bg-task .st-cancelled')"));
  await click('button.drawer-close');
  await wd("POST", `/session/${session}/window/rect`, { width: 620, height: 850 });
  await until("Compact sidebar collapsed", () => execute("return !document.querySelector('.sidebar')"));
  await screenshot("compact");
  await accessibility("compact");
  assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"), true);
  await wd("POST", `/session/${session}/window/rect`, { width: 1380, height: 920 });
  await click('button[aria-label="Terminal"]');
  await clickButton("Skills");
  await fill('#skill-name', "audit");
  await type('#skill-body', "---\nmode: review\n---\nWINDOW_SKILL: inspect $ARGUMENTS");
  assert.equal(await execute("return document.querySelector('#skill-body').value"), "---\nmode: review\n---\nWINDOW_SKILL: inspect $ARGUMENTS");
  await clickButton("Save skill");
  await until("Saved skill usable", () => execute("return [...document.querySelectorAll('button')].some(button=>button.textContent==='Use /audit')"));
  await click(".drawer-body .list details summary");
  await screenshot("skills");
  await accessibility("skills");
  workflowMode = true;
  await clickButton("Use /audit");
  assert.equal(await execute("return document.querySelector('textarea[aria-label=\"Message ShadowCode\"]').value"), "/skill audit ");
  await type('textarea[aria-label="Message ShadowCode"]', "README.md");
  await click('button[aria-label="Send task"]');
  await until("Selected skill result", () => execute("return [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('Selected skill reviewed README.md.')) && !document.querySelector('button[aria-label=\"Stop task\"]')"));
  assert.equal(workflowCalls, 2);
  await until("Skill provenance", () => execute("return [...document.querySelectorAll('.msg-note')].some(e=>e.textContent.includes('.shadow/skills/audit.md') && e.textContent.includes('review'))"));
  workflowMode = false;
  await type('textarea[aria-label="Message ShadowCode"]', "/status");
  await click('button[aria-label="Send task"]');
  await until("Durable command card", () => execute("return [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Permission mode'))"));
  await wd("POST", `/session/${session}/refresh`, {});
  await until("Skill and command reload", () => execute("return [...document.querySelectorAll('.msg-note')].some(e=>e.textContent.includes('.shadow/skills/audit.md')) && [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Permission mode'))"));
  goalMode = true;
  await click('button[aria-label="Terminal"]');
  await clickButton("Goals");
  await type('textarea[aria-label="Goal instruction"]', "Create goal.txt containing goal-native-ok and verify its contents.");
  await clickButton("Plan & run");
  await until("Goal command approval", () => execute("return !!document.querySelector('.approval button.primary')"));
  await click(".approval button.primary");
  await until("Goal completion", async () => {
    const goal = (await api("GET", "/api/goals")).goals[0];
    assert.notEqual(goal?.status, "blocked", goal?.run_detail);
    return goal?.status === "completed" && !goal.running;
  });
  await until("Goal checklist and transcript", () => execute("return document.querySelectorAll('.milestones li.done').length===3 && [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('Milestone complete: Run the acceptance checks'));"));
  assert.equal(await readFile(path.join(project, "goal.txt"), "utf8"), "goal-native-ok\n");
  assert.equal(milestoneCalls.size, 3);
  assert.deepEqual([...milestoneCalls.values()], [2, 2, 2]);
  await until("Goal progress bar", () => execute("const bar=document.querySelector('.goal.completed .bar');return bar && bar.querySelector('i').getBoundingClientRect().width >= bar.getBoundingClientRect().width*0.99;"));
  await screenshot("goals");
  await accessibility("goals");
  goalMode = false;
  const beforePause = requests;
  await type('textarea[aria-label="Goal instruction"]', "Explain how this project is organized.");
  await clickButton("Plan & run");
  await until("Running goal to pause", () => requests > beforePause);
  await until("Pause control", () => execute("return [...document.querySelectorAll('.goal button')].some(e=>e.textContent.trim()==='Pause' && !e.disabled);"));
  await clickButton("Pause");
  await until("Goal paused", async () => {
    const goal = (await api("GET", "/api/goals")).goals[0];
    return goal?.status === "paused" && !goal.running && goal.milestones.every(m => m.status === "pending");
  });
  const pausedGoal = (await api("GET", "/api/goals")).goals[0];
  await api("DELETE", `/api/sessions/${pausedGoal.session_id}`);
  await until("Resume control", () => execute("return [...document.querySelectorAll('.goal button')].some(e=>e.textContent.trim()==='Run' && !e.disabled);"));
  await clickButton("Run");
  await until("Goal resumed in a fresh conversation", async () => {
    const goal = (await api("GET", `/api/goals/${pausedGoal.id}`));
    return requests > beforePause + 1 && goal.running && goal.session_id !== pausedGoal.session_id;
  });
  await until("Resumed pause control", () => execute("return [...document.querySelectorAll('.goal button')].some(e=>e.textContent.trim()==='Pause' && !e.disabled);"));
  await clickButton("Pause");
  await until("Resumed goal paused", async () => {
    const goal = (await api("GET", `/api/goals/${pausedGoal.id}`));
    return goal.status === "paused" && !goal.running;
  });
  await clickButton("Background");
  await fill('#background-name', "shutdown-server");
  await type('#background-command', "trap '' TERM; sleep 60 & echo $! > background-child.pid; printf shutdown-ready; wait");
  await clickButton("Start process");
  const shutdownBackground = await until("Background process for shutdown", async () => {
    const task = (await api("GET", "/api/background")).tasks[0];
    return task?.name === "shutdown-server" && task.output.includes("shutdown-ready") && task;
  });
  const backgroundChild = (await readFile(path.join(project, "background-child.pid"), "utf8")).trim();
  // A separate CLI in another project shares this actual desktop's engine.
  // It must not change the visible or remembered project/session selection.
  const cliProject = path.join(scratch, "cli-project"); await mkdir(cliProject);
  const cliEnv = {...nativeEnv}; delete cliEnv.DISPLAY; delete cliEnv.WAYLAND_DISPLAY;
  const cli = async args => {
    const result = await promisify(execFile)(binary, [...binaryArgs.filter(arg => arg !== "ui"), ...profileArgs, "--workspace", cliProject, "--json", ...args], {env: cliEnv, timeout: 20000, maxBuffer: 2_000_000});
    return JSON.parse(result.stdout);
  };
  const remembered = await readFile(path.join(stateDirectory, "last-workspace.txt"), "utf8");
  assert.equal((await cli(["health"])).workspace, cliProject);
  await cli(["trust"]);
  assert.ok((await cli(["command", "new"])).metadata.session_id);
  const cliProcess = await cli(["background", "start", "--name", "cli-owner", "--command", "printf cli-ready; sleep 60"]);
  await until("CLI logs through desktop owner", async () => (await cli(["background", "logs", cliProcess.id])).output.includes("cli-ready"));
  await cli(["background", "stop", cliProcess.id]);
  assert.equal((await api("GET", "/api/workspace/status")).workspace, project);
  assert.equal(await readFile(path.join(stateDirectory, "last-workspace.txt"), "utf8"), remembered);
  const desktopProcesses = (await api("GET", "/api/background")).tasks;
  assert.ok(desktopProcesses.some(p => p.id === shutdownBackground.id && p.status === "RUNNING"));
  assert.ok(desktopProcesses.every(p => p.id !== cliProcess.id));
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
  await until("Background child cleanup", () => dead(backgroundChild));
  await until("Background process cleanup", () => dead(shutdownBackground.pid));
  await writeFile(path.join(artifacts, "result.json"), JSON.stringify({ passed: true, version: version.version, runtime: version.runtime, modelRequests: requests, requestedModels, checks: [...(defaultProfile ? ["repeated default-profile activation preserves the live window and its extraction"] : []), "embedded interface", "Rust IPC", "native approval", "real file write and terminal verification", "durable reload", "native routing controls and persisted model/fallback notices", "background start, live output, coexistence with tasks, stop, and child cleanup on quit", "selected skill execution, mode enforcement, provenance and durable command cards", "reviewed hook activation and disable in Settings, actual completion check, durable hook result", "shared CLI engine with independent project selection and background controls", "cancellation", "compact layout", "native light/dark/compact/goals/routing/background/skills/hooks accessibility", "goal creation, automatic milestone progression, verification, live transcript and pause", "managed native shutdown"] }, null, 2));
  console.log("Native desktop window passed: IPC, approval, file/terminal tools, routing, background processes, shared CLI isolation, replay, cancellation, layout, goals, accessibility, shutdown.");
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
