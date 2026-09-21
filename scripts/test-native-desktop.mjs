// Real Tauri/WebKit window test. Requires a display (xvfb-run works), DBus,
// tauri-driver, and WebKitWebDriver. No Python service or browser launcher.
import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { createServer } from "node:http";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { createWriteStream } from "node:fs";
import { mkdtemp, mkdir, readFile, writeFile, readlink, readdir, rm, rename } from "node:fs/promises";
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
for (const name of ["hooks.png", "accessibility-hooks.json", "mcp.png", "accessibility-mcp.json", "mcp-http.png", "accessibility-mcp-http.json", "inspection.png", "accessibility-inspection.json", "diagnostics.png", "accessibility-diagnostics.json", "plugins.png", "accessibility-plugins.json", "accessibility-plugins-dark.json", "accessibility-plugins-compact.json"]) await rm(path.join(artifacts, name), { force: true });
for (const theme of ["light","dark","compact"]) for (const name of [`queue-${theme}.png`,`accessibility-queue-${theme}.json`]) await rm(path.join(artifacts,name),{force:true});
for (const theme of ["light","dark","compact"]) for (const name of [`background-approval-${theme}.png`,`accessibility-background-approval-${theme}.json`]) await rm(path.join(artifacts,name),{force:true});
for (const view of ["worktree-copy", "worktree-return", "worktree-recovery", "worktree-repair", "history"]) for (const name of [`${view}.png`, `accessibility-${view}.json`, `accessibility-${view}-dark.json`, `accessibility-${view}-compact.json`]) await rm(path.join(artifacts,name),{force:true});
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-window-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
const defaultProfile = process.env.SHADOW_NATIVE_DEFAULT_PROFILE === "1";
const profileArgs = defaultProfile ? [] : ["--profile",profile];
const configDirectory = path.join(profile,defaultProfile ? "config/shadow-agent" : "config");
const dataDirectory = path.join(profile,defaultProfile ? "data/shadow-agent" : "data");
const stateDirectory = path.join(profile,defaultProfile ? "state/shadow-agent" : "state");
const nativeEnv = {...process.env, TMPDIR: path.join(scratch,"images"), ...(defaultProfile ? {
  XDG_CONFIG_HOME: path.join(profile,"config"), XDG_DATA_HOME: path.join(profile,"data"), XDG_STATE_HOME: path.join(profile,"state"),
} : {})};
delete nativeEnv.NO_CLEANUP;
nativeEnv.SHADOW_WINDOW_MCP_PID = path.join(scratch, "mcp-pids.json");
nativeEnv.SHADOW_WINDOW_MCP_REQUESTS = path.join(scratch, "mcp-requests.jsonl");
nativeEnv.SHADOW_WINDOW_MCP_SECRET = "private-window-mcp-credential";
nativeEnv.SHADOW_WINDOW_HTTP_SECRET = "http-private-fixture-key";
const httpRoot = path.join(scratch,"http-peer");
await mkdir(httpRoot);
let httpPeer;
await mkdir(nativeEnv.TMPDIR);
await mkdir(project); await mkdir(configDirectory, { recursive: true });
await writeFile(path.join(project, "README.md"), "# Native desktop test\nA disposable workspace.\n");
for (const args of [["init","-q"],["config","user.name","Desktop Test"],["config","user.email","test@example.invalid"],["add","README.md"],["commit","-qm","Desktop fixture base"]]) await promisify(execFile)("git",["-c","core.hooksPath=/dev/null","-c","user.name=Desktop Test","-c","user.email=test@example.invalid","-c","commit.gpgsign=false",...args],{cwd:project});
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
async function dead(pid) {
  try { return /\) [ZX] /.test(await readFile(`/proc/${pid}/stat`, "utf8")); } catch { return true; }
}
let requests = 0;
const requestedModels = [];
let goalMode = false;
let workflowMode = false;
let workflowCalls = 0;
let mcpMode = false, mcpCalls = 0, mcpHttpCalls = 0;
let queueMode = false;
const queueRequests = [], queueReplies = new Map();
let backgroundToolMode = false, backgroundToolCalls = 0, modelBackgroundId;
const modelBackgroundCommand = "sleep 60 & echo $! > model-background-child.pid; printf model-background-ready; wait";
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
  if (backgroundToolMode) {
    const call = backgroundToolCalls++;
    let message;
    if (backgroundToolMode === "start") {
      assert.ok(payload.tools.some(item=>item.function.name==="background_start"));
      if (call === 0) message = {role:"assistant",content:"Starting a managed project watcher.",tool_calls:[tool("background_start",{name:"model-watcher",command:modelBackgroundCommand})]};
      else if (call === 1) {
        const result = JSON.parse(payload.messages.filter(item=>item.role==="tool").at(-1).content);
        assert.equal(result.success,true);
        assert.equal(result.output.lifetime,"project");
        modelBackgroundId=result.output.id;
        await until("Model watcher child exists",async()=>Number((await readFile(path.join(project,"model-background-child.pid"),"utf8")).trim())>0);
        message={role:"assistant",content:"Reading the watcher log.",tool_calls:[tool("background_output",{id:modelBackgroundId})]};
      } else {
        assert.equal(call,2);
        const result = JSON.parse(payload.messages.filter(item=>item.role==="tool").at(-1).content);
        assert.equal(result.success,true);
        assert.ok(result.output.output.includes("model-background-ready"));
        message={role:"assistant",content:"The managed watcher reports model-background-ready and remains running."};
      }
    } else if (call === 0) message={role:"assistant",content:"Stopping the recorded project watcher.",tool_calls:[tool("background_stop",{id:modelBackgroundId})]};
    else {
      assert.equal(call,1);
      const result = JSON.parse(payload.messages.filter(item=>item.role==="tool").at(-1).content);
      assert.equal(result.success,true);
      assert.equal(result.output.status,"CANCELLED");
      message={role:"assistant",content:"The managed watcher stopped and cleanup finished."};
    }
    res.writeHead(200,{"Content-Type":"application/json"});
    res.end(JSON.stringify({choices:[{message,finish_reason:message.tool_calls?"tool_calls":"stop"}],usage:{prompt_tokens:30,completion_tokens:10,total_tokens:40}}));
    return;
  }
  if (queueMode) {
    const task = payload.messages.filter(message => message.role === "user").at(-1).content;
    assert.ok(["Queue probe: first", "Queue probe: second"].includes(task), "Cancelled queued tasks must not contact the model");
    queueRequests.push(task);
    if (task.endsWith("second")) {
      assert.equal(payload.model,"native-build");
      assert.ok(payload.tools.every(item => !["exec","write_file"].includes(item.function.name)), "Queued Review preserves read-only tools");
      assert.ok(payload.messages.some(message => message.role === "assistant" && message.content?.includes("First task stays visible")), "The follow-up receives the completed predecessor's conversation");
    }
    res.writeHead(200,{"Content-Type":"text/event-stream"});
    await delay(120);
    res.write(`data: ${JSON.stringify({choices:[{delta:{content:task.endsWith("first") ? "First task stays visible while follow-ups wait." : "Second queued task completed."}}]})}\n\n`);
    queueReplies.set(task,()=>res.end(`data: ${JSON.stringify({choices:[{delta:{},finish_reason:"stop"}],usage:{prompt_tokens:30,completion_tokens:10,total_tokens:40}})}\n\ndata: [DONE]\n\n`));
    return;
  }
  if (mcpMode) {
    const http = mcpMode === "http";
    const call = http ? mcpHttpCalls++ : mcpCalls++;
    const server = http ? "config:window-http" : "config:window-mcp";
    const expected = http ? "native-http-ok" : "native-mcp-ok";
    assert.ok(payload.tools.some(t => t.function.name === "mcp_call"));
    if (call === 2) {
      const result = JSON.parse(payload.messages.filter(m => m.role === "tool").at(-1).content);
      assert.equal(result.success, true);
      assert.equal(result.output.result.structuredContent.arguments.message, expected);
      if (http) assert.equal(result.output.result.structuredContent.headers.authorization, "Bearer [redacted]");
      else assert.equal(result.output.result.structuredContent.environment, "[redacted]");
      assert.equal(JSON.stringify(payload).includes(http ? nativeEnv.SHADOW_WINDOW_HTTP_SECRET : nativeEnv.SHADOW_WINDOW_MCP_SECRET), false);
    }
    const message = call === 0 ? {role:"assistant", content:"Inspecting the enabled MCP tool.", tool_calls:[tool("mcp_tools",{server,tool:"echo"})]}
      : call === 1 ? {role:"assistant", content:"Requesting the external tool call.", tool_calls:[tool("mcp_call",{server,tool:"echo",arguments:{message:expected,...(http?{action:"sse"}:{})}})]}
      : {role:"assistant",content:`External fixture returned ${expected}.`};
    res.writeHead(200,{"Content-Type":"application/json"});
    res.end(JSON.stringify({choices:[{message,finish_reason:call<2?"tool_calls":"stop"}],usage:{prompt_tokens:30,completion_tokens:10,total_tokens:40}}));
    return;
  }
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
  await until(`Button ready: ${text}`, () => execute("return [...document.querySelectorAll('button')].some(button => button.textContent.replace(/\\s+/g, ' ').trim() === arguments[0] && !button.disabled && button.getClientRects().length > 0)", [text]));
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
async function openSettings() {
  if (await execute("return !!document.querySelector('button[aria-label=\"Show sidebar\"]')")) await click('button[aria-label="Show sidebar"]');
  await until("Sidebar Settings available",()=>execute("return [...document.querySelectorAll('.sidebar button')].some(e=>e.querySelector('span')?.textContent==='Settings')"));
  await execute("[...document.querySelectorAll('.sidebar button')].find(e=>e.querySelector('span')?.textContent==='Settings').click()");
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
  await openSettings();
  await clickButton("Worktrees");
  await until("Worktree creation ready",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Create worktree' && !b.disabled)"));
  await clickButton("Create worktree");
  const worktree=await until("Managed worktree created",async()=>(await api("GET","/api/worktrees")).worktrees.find(w=>w.state==="ready"));
  assert.equal(await readFile(path.join(worktree.path,"README.md"),"utf8"),"# Native desktop test\nA disposable workspace.\n");
  await until("Worktree open button",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Open worktree'&&!b.disabled)"));await clickButton("Open worktree");
  await until("Explicit worktree trust",()=>execute("return [...document.querySelectorAll('h2')].some(e=>e.textContent==='Trust this folder?')"));await clickButton("Cancel");
  await openSettings();await clickButton("Worktrees");await until("Worktree inspect ready",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Inspect removal'&&!b.disabled)"));await clickButton("Inspect removal");
  await until("Worktree review focused",()=>execute("return document.activeElement?.classList.contains('worktree-review')"));
  await screenshot("worktrees");await accessibility("worktrees");
  await execute("document.documentElement.dataset.theme='dark'");await accessibility("worktrees-dark");await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`,{width:620,height:850});await accessibility("worktrees-compact");assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`,{width:1380,height:920});
  await clickButton("Remove clean worktree");await until("Managed worktree removed",async()=>!(await api("GET","/api/worktrees")).worktrees.length);
  assert.equal((await promisify(execFile)("git",["rev-parse",worktree.branch],{cwd:project})).stdout.trim(),worktree.base_commit);
  await until("Recovery fixture creation ready",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Create worktree'&&!b.disabled)"));
  await clickButton("Create worktree");
  const missingWorktree=await until("Recovery fixture created",async()=>(await api("GET","/api/worktrees")).worktrees.find(w=>w.state==="ready"));
  const originalRecord=path.join(dataDirectory,"managed-worktrees/records",`${missingWorktree.id}.json`);
  const originalRecordBytes=await readFile(originalRecord,"utf8");
  await rename(missingWorktree.path,path.join(scratch,"saved-recovery-checkout"));
  await until("Recovery review ready",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Review missing checkout'&&!b.disabled)"));
  await clickButton("Review missing checkout");
  await until("Recovery review focused",()=>execute("return document.activeElement?.getAttribute('aria-label')==='Review worktree recovery'"));
  assert.ok(await execute("return document.querySelector('[aria-label=\"Review worktree recovery\"]').textContent.includes('Missing uncommitted files are not reconstructed')"));
  await screenshot("worktree-recovery");await accessibility("worktree-recovery");
  await execute("document.documentElement.dataset.theme='dark'");await accessibility("worktree-recovery-dark");await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`,{width:620,height:850});await accessibility("worktree-recovery-compact");assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`,{width:1380,height:920});
  await clickButton("Restore in new worktree");
  const restoredWorktree=await until("Committed work restored",async()=>(await api("GET","/api/worktrees")).worktrees.find(w=>w.id!==missingWorktree.id&&w.state==="ready"));
  assert.equal(await readFile(path.join(restoredWorktree.path,"README.md"),"utf8"),"# Native desktop test\nA disposable workspace.\n");
  assert.equal(await readFile(originalRecord,"utf8"),originalRecordBytes);
  assert.equal((await promisify(execFile)("git",["rev-parse",missingWorktree.branch],{cwd:project})).stdout.trim(),missingWorktree.base_commit);
  await clickButton("Plugins");
  await until("Native plugin catalog",()=>execute("return !!document.querySelector('.plugin-settings')"));
  await clickButton("Review python-expert");
  await until("Builtin plugin preview",()=>execute("return !!document.querySelector('.plugin-preview')?.textContent.includes('ruff-format')"));
  await clickButton("Install python-expert");
  await until("Builtin plugin installed",async()=>(await api("GET","/api/plugins")).installed.some(p=>p.name==="python-expert"));
  assert.ok((await api("GET","/api/hooks")).hooks.filter(h=>h.name.startsWith("python-expert--")).every(h=>!h.enabled));
  await clickButton("Remove python-expert");
  await until("Builtin plugin removed",async()=>!(await api("GET","/api/plugins")).installed.length);
  await click('.plugin-import summary');
  const windowBundle={format:"shadowcode-plugin-v1",name:"window-bundle",version:"1.0.0",description:"Native window integration fixture",skills:{audit:{description:"Review the requested file",mode:"review",content:"WINDOW_SKILL: inspect $ARGUMENTS"}},hooks:[{name:"finished",events:["on_complete"],command:"printf plugin-window-ok"}]};
  await fill('.plugin-import textarea',JSON.stringify(windowBundle));
  await clickButton("Review imported bundle");
  await until("Imported plugin preview",()=>execute("return !!document.querySelector('.plugin-preview')?.textContent.includes('window-bundle')"));
  await until("Plugin review receives focus",()=>execute("return document.activeElement?.classList.contains('plugin-preview')"));
  await click('.plugin-preview details summary');
  await screenshot("plugins"); await accessibility("plugins");
  await execute("document.documentElement.dataset.theme='dark'"); await accessibility("plugins-dark");
  await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`, { width: 620, height: 850 });
  await accessibility("plugins-compact");
  assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`, { width: 1380, height: 920 });
  await clickButton("Install window-bundle");
  await until("Custom plugin installed",async()=>(await api("GET","/api/plugins")).installed.some(p=>p.name==="window-bundle"));
  await clickButton("Review plugin hooks");
  await until("Plugin hook refreshed",()=>execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Enable window-bundle--finished')"));
  await clickButton("Enable window-bundle--finished");
  await until("Plugin hook enabled",async()=>(await api("GET","/api/hooks")).hooks.find(h=>h.name==="window-bundle--finished")?.enabled);
  await clickButton("Disable window-bundle--finished");
  await until("Plugin hook disabled",async()=>!(await api("GET","/api/hooks")).hooks.find(h=>h.name==="window-bundle--finished")?.enabled);
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
  await openSettings();
  await clickButton("Hooks");
  await until("Hook disable control", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Disable verify-result')"));
  await clickButton("Disable verify-result");
  await until("Hook disabled", async () => !(await api("GET", "/api/hooks")).hooks[0].enabled);
  await clickButton("MCP");
  await until("Native MCP Settings", () => execute("return !!document.querySelector('.mcp-settings form')"));
  await type('.mcp-settings input', "window-mcp");
  await type('.mcp-settings textarea', JSON.stringify(["node", path.join(root,"native/core/tests/fixtures/mcp-server.mjs"),"normal"]));
  await fill('.mcp-settings textarea[rows="2"]', JSON.stringify({MCP_PID_FILE:"SHADOW_WINDOW_MCP_PID",MCP_REQUEST_FILE:"SHADOW_WINDOW_MCP_REQUESTS",MCP_LITERAL:"SHADOW_WINDOW_MCP_SECRET"}));
  await clickButton("Register MCP server");
  await until("MCP registration in Settings", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Enable window-mcp' && !e.disabled)"));
  assert.equal(await readFile(nativeEnv.SHADOW_WINDOW_MCP_PID).then(()=>true,()=>false),false);
  await clickButton("Enable window-mcp");
  await until("MCP activated", async () => (await api("GET","/api/mcp/servers")).servers[0].enabled);
  await screenshot("mcp");
  await accessibility("mcp");
  await clickButton("Close");
  mcpMode = true;
  await fill('textarea[aria-label="Message ShadowCode"]', "Call the enabled fixture tool with the message native-mcp-ok.");
  await click('button[aria-label="Send task"]');
  await until("External tool approval", () => execute("return !!document.querySelector('.approval')?.textContent.includes('window-mcp')"));
  assert.match(await execute("return document.querySelector('.approval .code').textContent"), /MCP config:window-mcp \/ echo[\s\S]*"message": "native-mcp-ok"/);
  assert.ok((await readFile(nativeEnv.SHADOW_WINDOW_MCP_REQUESTS,"utf8")).split("\n").filter(Boolean).map(line=>JSON.parse(line)).every(r=>r.method!=="tools/call"));
  await click(".approval button.primary");
  await until("External tool completed", () => execute("return document.querySelector('.msg-agent:last-of-type')?.textContent.includes('External fixture returned native-mcp-ok.') || [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('External fixture returned native-mcp-ok.'))"));
  await until("External task finished", async () => !(await api("GET","/api/jobs")).jobs.some(j=>j.status==="running" || j.status==="queued"));
  const mcpPids = JSON.parse(await readFile(nativeEnv.SHADOW_WINDOW_MCP_PID,"utf8"));
  for (const pid of mcpPids) {
    await until("MCP process cleanup", async () => { try { return /\) [ZX] /.test(await readFile(`/proc/${pid}/stat`,"utf8")); } catch (error) { if(error.code!=="ENOENT") throw error; return true; } });
  }
  assert.equal(mcpCalls,3);
  mcpMode = false;
  await openSettings();
  await clickButton("MCP");
  await until("MCP disable control", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Disable window-mcp' && !e.disabled)"));
  await clickButton("Disable window-mcp");
  await until("MCP disabled", async () => !(await api("GET","/api/mcp/servers")).servers[0].enabled);
  await until("MCP removal ready", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Remove window-mcp' && !e.disabled)"));
  await clickButton("Remove window-mcp");
  await until("MCP registration removed", async () => !(await api("GET","/api/mcp/servers")).servers.length);
  // Exercise HTTP registration and the same exact-argument approval in WebKit.
  httpPeer = spawn(process.execPath,[path.join(root,"native/core/tests/fixtures/mcp-http.mjs"),"auth",httpRoot],{stdio:["ignore","ignore","pipe"]});
  httpPeer.stderr.pipe(output, {end:false});
  let httpUrl;
  await until("HTTP peer ready", async()=> { httpUrl=JSON.parse(await readFile(path.join(httpRoot,"ready.json"),"utf8")).url; return httpUrl; });
  const httpRequests = async()=> (await readFile(path.join(httpRoot,"requests.jsonl"),"utf8").catch(e=>{if(e.code!=="ENOENT")throw e;return "";})).split("\n").filter(Boolean).map(line=>JSON.parse(line));
  await until("MCP registration form reopened", () => execute("return document.querySelector('.mcp-add').open"));
  await type('.mcp-settings input', "window-http");
  await execute("const select=document.querySelector('.mcp-settings select');select.value='http';select.dispatchEvent(new Event('change',{bubbles:true}));");
  await type('.mcp-settings input[type="url"]',httpUrl);
  await type('.mcp-settings input[placeholder="MY_MCP_TOKEN"]',"SHADOW_WINDOW_HTTP_SECRET");
  await clickButton("Register MCP server");
  await until("HTTP registration in Settings", () => execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Enable window-http' && !e.disabled)"));
  assert.equal((await httpRequests()).length,0);
  await clickButton("Enable window-http");
  await until("HTTP activated", async()=> (await api("GET","/api/mcp/servers")).servers[0].enabled);
  assert.equal((await httpRequests()).length,0);
  await screenshot("mcp-http");
  await accessibility("mcp-http");
  await clickButton("Close");
  mcpMode = "http";
  await fill('textarea[aria-label="Message ShadowCode"]',"Call the HTTP fixture with native-http-ok.");
  await click('button[aria-label="Send task"]');
  await until("HTTP tool approval",()=>execute("return !!document.querySelector('.approval')?.textContent.includes('window-http')"));
  assert.match(await execute("return document.querySelector('.approval .code').textContent"),/MCP config:window-http \/ echo[\s\S]*"message": "native-http-ok"/);
  assert.ok((await httpRequests()).every(r=>r.message?.method!=="tools/call"));
  await click(".approval button.primary");
  await until("HTTP tool completed",()=>execute("return [...document.querySelectorAll('.msg-agent')].some(e=>e.textContent.includes('External fixture returned native-http-ok.'))"));
  await until("HTTP task completed", async()=> (await api("GET","/api/jobs")).jobs[0].status==="completed");
  assert.equal(mcpHttpCalls,3);
  assert.equal((await httpRequests()).filter(r=>r.message?.method==="tools/call").length,1);
  await until("HTTP streams closed",async()=> (await (await fetch(httpUrl.replace("/mcp","/status"))).json()).streams===0);
  mcpMode=false;
  await openSettings();
  await clickButton("MCP");
  await until("HTTP removal ready",()=>execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Remove window-http' && !e.disabled)"));
  await clickButton("Remove window-http");
  await until("HTTP registration removed",async()=>!(await api("GET","/api/mcp/servers")).servers.length);
  await clickButton("Close");
  // A missing model in an older saved configuration must be visible when the
  // engine chooses the default, including after the conversation is reloaded.
  await api("PUT", "/api/config", { values: { routing: { coder: "removed-native-model" } } });
  const beforeFallback = requests;
  await type('textarea[aria-label="Message ShadowCode"]', "Explain the result again.");
  await click('button[aria-label="Send task"]');
  await until("Fallback model request", () => requests > beforeFallback);
  await until("Fallback visible", () => execute("return [...document.querySelectorAll('.msg-note.warning')].some(e=>e.textContent.includes('Using default: native-fixture'))"));
  assert.equal(requestedModels.at(-1), "native-fixture");
  await click('button[aria-label="Stop task"]');
  await until("Cancellation persisted", async () => (await api("GET", "/api/jobs")).jobs[0].status === "cancelled");
  await until("Cancellation visible", () => execute("return [...document.querySelectorAll('.msg-agent')].some(e=>/cancelled/i.test(e.textContent));"));
  await api("PUT", "/api/routing", { values: { enabled: false } });
  queueMode = true;
  await type('textarea[aria-label="Message ShadowCode"]', "Queue probe: first");
  await until("Idle composer after cancellation",()=>execute("return !!document.querySelector('button[aria-label=\"Send task\"]')"));
  await click('button[aria-label="Send task"]');
  await until("First queue task streaming",()=>execute("return [...document.querySelectorAll('.msg-agent')].some(item=>item.textContent.includes('First task stays visible'))"));
  const firstQueue = (await api("GET","/api/jobs")).jobs.find(job=>job.task==="Queue probe: first");
  await execute("const select=document.querySelector('select[aria-label=\"Agent mode\"]'); select.value='reviewer'; select.dispatchEvent(new Event('change',{bubbles:true})); const model=document.querySelector('select[aria-label=\"Model for this task\"]'); model.value=arguments[0]; model.dispatchEvent(new Event('change',{bubbles:true}));",[registered.model.default]);
  await type('textarea[aria-label="Message ShadowCode"]', "Queue probe: second");
  await click('button[aria-label="Queue follow-up"]');
  await until("Second task queued",()=>execute("return !!document.querySelector('.task-queue')?.textContent.includes('Queue probe: second')"));
  await execute("const select=document.querySelector('select[aria-label=\"Agent mode\"]');select.value='coder';select.dispatchEvent(new Event('change',{bubbles:true}));const model=document.querySelector('select[aria-label=\"Model for this task\"]');model.value='';model.dispatchEvent(new Event('change',{bubbles:true}));");
  await type('textarea[aria-label="Message ShadowCode"]', "Queue probe: cancel");
  await click('button[aria-label="Queue follow-up"]');
  await until("Two queued follow-ups",()=>execute("return document.querySelectorAll('.task-queue li').length===2"));
  assert.equal((await api("GET",`/api/jobs/current?session_id=${firstQueue.session_id}&include_finished=true`)).job.id,firstQueue.id);
  assert.equal(queueRequests.length,1);
  await click("button.new-task");
  await until("New conversation selected",()=>execute("return !!document.querySelector('.task-link[aria-current=\"page\"]') && document.querySelector('.task-link[aria-current=\"page\"]').dataset.sessionId!==arguments[0] && !document.querySelector('.loading-task')",[firstQueue.session_id]));
  await type('textarea[aria-label="Message ShadowCode"]', "Queue probe: other");
  await click('button[aria-label="Queue follow-up"]');
  await until("Project queue spans conversations",()=>execute("return document.querySelectorAll('.task-queue li').length===3"));
  const otherQueue = (await api("GET","/api/jobs")).jobs.find(job=>job.task==="Queue probe: other");
  assert.notEqual(otherQueue.session_id,firstQueue.session_id);
  await click(`.task-queue li[data-job-id="${otherQueue.id}"] .queue-cancel`);
  await until("Other conversation queue item cancelled",async()=>(await api("GET",`/api/jobs/${otherQueue.id}`)).status==="cancelled");
  await click(`button.task-link[data-session-id="${firstQueue.session_id}"]`);
  await until("Original stream retained after switching",()=>execute("return [...document.querySelectorAll('.msg-agent')].some(item=>item.textContent.includes('First task stays visible'))"));
  await wd("POST",`/session/${session}/refresh`,{});
  await until("Queue restored after reload",()=>execute("return document.querySelectorAll('.task-queue li').length===2 && !!document.querySelector('button[aria-label=\"Stop task\"]')"),25000);
  await until("Reload follows the running response",()=>execute("const item=[...document.querySelectorAll('.msg-agent')].find(item=>item.textContent.includes('First task stays visible'));if(!item)return false;const bounds=item.getBoundingClientRect();const queue=document.querySelector('.task-queue').getBoundingClientRect();return bounds.top>=55 && bounds.bottom<=queue.top"));
  assert.equal((await api("GET",`/api/jobs/current?session_id=${firstQueue.session_id}&include_finished=true`)).job.id,firstQueue.id);
  for (const theme of ["light","dark"]) {
    await execute("document.documentElement.dataset.theme=arguments[0]",[theme]);
    await screenshot(`queue-${theme}`);
    await accessibility(`queue-${theme}`);
  }
  await execute("document.documentElement.dataset.theme='light'");
  await wd("POST",`/session/${session}/window/rect`,{width:620,height:850});
  await until("Compact queue sidebar closes",()=>execute("return !document.querySelector('.sidebar')"));
  await screenshot("queue-compact");
  await accessibility("queue-compact");
  assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST",`/session/${session}/window/rect`,{width:1380,height:920});
  const cancelledQueue = (await api("GET","/api/jobs")).jobs.find(job=>job.task==="Queue probe: cancel");
  await click(`.task-queue li[data-job-id="${cancelledQueue.id}"] .queue-cancel`);
  await until("Queued cancellation persisted",async()=>(await api("GET",`/api/jobs/${cancelledQueue.id}`)).status==="cancelled");
  assert.equal((await api("GET",`/api/jobs/${firstQueue.id}`)).status,"running");
  assert.equal(queueRequests.length,1);
  queueReplies.get("Queue probe: first")();
  await until("Next queued model call",()=>queueReplies.has("Queue probe: second"));
  await until("Next queued response streams",()=>execute("return [...document.querySelectorAll('.msg-agent')].some(item=>item.textContent.includes('Second queued task completed'))"));
  queueReplies.get("Queue probe: second")();
  await until("Queue drains",async()=>!(await api("GET","/api/jobs")).jobs.some(job=>["queued","running","cancelling"].includes(job.status)));
  await until("Queue clears in desktop",()=>execute("return !document.querySelector('.task-queue') && !!document.querySelector('button[aria-label=\"Send task\"]')"));
  await until("Sidebar no longer marks completed tasks as running",()=>execute("return !document.querySelector('.task-link .running-dot')"));
  assert.deepEqual(queueRequests,["Queue probe: first","Queue probe: second"]);
  assert.equal(await execute("return [...document.querySelectorAll('.msg-user')].filter(item=>item.textContent==='Queue probe: second').length"),1);
  queueMode=false;
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
  // Return a committed worktree change after the source background process stops.
  const returnGit=async(cwd,args)=>(await promisify(execFile)("git",["-c","core.hooksPath=/dev/null","-c","user.name=Desktop Test","-c","user.email=test@example.invalid","-c","commit.gpgsign=false",...args],{cwd})).stdout.trim();
  await returnGit(project,["add","."]);await returnGit(project,["commit","-qm","Preserve source fixture state"]);
  await writeFile(path.join(restoredWorktree.path,"returned-window.txt"),"reviewed desktop return\n");
  await returnGit(restoredWorktree.path,["add","returned-window.txt"]);await returnGit(restoredWorktree.path,["commit","-qm","Reviewed worktree result"]);
  const sourceHeadBeforeReturn=await returnGit(project,["rev-parse","HEAD"]);
  await openSettings();await clickButton("Worktrees");
  await until("Return card ready",()=>execute("return !!document.querySelector(arguments[0])",[`[data-worktree-id="${restoredWorktree.id}"]`]));
  const returnButton=await wd("POST",`/session/${session}/element`,{using:"xpath",value:`//article[@data-worktree-id='${restoredWorktree.id}']//button[normalize-space(.)='Review return']`});
  await wd("POST",`/session/${session}/element/${returnButton["element-6066-11e4-a52e-4f735466cecf"]}/click`,{});
  await until("Return review focused",()=>execute("return document.activeElement?.getAttribute('aria-label')==='Review returned changes'"));
  assert.ok(await execute("return document.querySelector('[aria-label=\"Incoming worktree diff\"]').textContent.includes('+reviewed desktop return')"));
  await screenshot("worktree-return");await accessibility("worktree-return");
  await execute("document.documentElement.dataset.theme='dark'");await accessibility("worktree-return-dark");await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`,{width:620,height:850});await accessibility("worktree-return-compact");assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`,{width:1380,height:920});
  await clickButton("Prepare merge in source");
  await until("Returned merge pending",async()=>(await api("GET","/api/worktrees")).worktrees.some(w=>w.id===restoredWorktree.id&&w.state==="merge_pending"));
  assert.equal(await readFile(path.join(project,"returned-window.txt"),"utf8"),"reviewed desktop return\n");
  assert.equal(await returnGit(project,["rev-parse","HEAD"]),sourceHeadBeforeReturn);
  await until("Open source available",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Open source project'&&!b.disabled)"));
  await clickButton("Close");
  await returnGit(project,["merge","--abort"]);
  assert.equal(await returnGit(project,["rev-parse","HEAD"]),sourceHeadBeforeReturn);
  // Copy a reviewed dirty source through the desktop, preserving its staging split.
  const copyReadme = await readFile(path.join(project,"README.md"),"utf8");
  await writeFile(path.join(project,"README.md"),copyReadme+"\nStaged desktop copy\n");
  await returnGit(project,["add","README.md"]);
  await writeFile(path.join(project,"README.md"),copyReadme+"\nStaged desktop copy\nUnstaged desktop copy\n");
  await writeFile(path.join(project,"copy-untracked.txt"),"untracked desktop copy\n");
  const copySourceStatus=await returnGit(project,["status","--porcelain"]);
  const copySourceIndex=await returnGit(project,["show",":README.md"]);
  const previousCopyIds=new Set((await api("GET","/api/worktrees")).worktrees.map(w=>w.id));
  await openSettings();await clickButton("Worktrees");await clickButton("Review current edits");
  await until("Copy review focused",()=>execute("return document.activeElement?.getAttribute('aria-label')==='Review copied changes'"));
  assert.ok(await execute("return document.querySelector('[aria-label=\"Staged copy diff\"]').textContent.includes('+Staged desktop copy')"));
  assert.ok(await execute("return document.querySelector('[aria-label=\"Unstaged copy diff\"]').textContent.includes('+Unstaged desktop copy')"));
  await screenshot("worktree-copy");await accessibility("worktree-copy");
  await execute("document.documentElement.dataset.theme='dark'");await accessibility("worktree-copy-dark");await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`,{width:620,height:850});await accessibility("worktree-copy-compact");assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`,{width:1380,height:920});
  await clickButton("Copy into new worktree");
  const copiedWorktree=await until("Desktop changes copied",async()=>(await api("GET","/api/worktrees")).worktrees.find(w=>!previousCopyIds.has(w.id)&&w.state==="ready"));
  assert.equal(await returnGit(project,["status","--porcelain"]),copySourceStatus);
  assert.equal(await returnGit(project,["show",":README.md"]),copySourceIndex);
  assert.equal(await returnGit(copiedWorktree.path,["show",":README.md"]),copySourceIndex);
  assert.equal(await readFile(path.join(copiedWorktree.path,"README.md"),"utf8"),await readFile(path.join(project,"README.md"),"utf8"));
  assert.equal(await readFile(path.join(copiedWorktree.path,"copy-untracked.txt"),"utf8"),"untracked desktop copy\n");
  await clickButton("Close");
  const repairAdmin=path.join(copiedWorktree.common_directory,"worktrees",copiedWorktree.id);
  const beforeRepairIndex=await readFile(path.join(repairAdmin,"index"));
  await rm(path.join(copiedWorktree.path,".git"));
  await openSettings();await clickButton("Worktrees");
  await until("Repair card ready",()=>execute("return !!document.querySelector(arguments[0])",[`[data-worktree-id="${copiedWorktree.id}"]`]));
  const repairButton=await wd("POST",`/session/${session}/element`,{using:"xpath",value:`//article[@data-worktree-id='${copiedWorktree.id}']//button[normalize-space(.)='Review connection repair']`});
  await wd("POST",`/session/${session}/element/${repairButton["element-6066-11e4-a52e-4f735466cecf"]}/click`,{});
  await until("Connection repair focused",()=>execute("return document.activeElement?.getAttribute('aria-label')==='Review connection repair'"));
  await screenshot("worktree-repair");await accessibility("worktree-repair");
  await execute("document.documentElement.dataset.theme='dark'");await accessibility("worktree-repair-dark");await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`,{width:620,height:850});await accessibility("worktree-repair-compact");assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`,{width:1380,height:920});
  await clickButton("Restore Git connection");
  await until("Connection repair persisted",async()=>(await api("GET","/api/worktrees")).worktrees.some(w=>w.id===copiedWorktree.id&&w.state==="ready"&&w.detail.includes("connection restored")));
  assert.deepEqual(await readFile(path.join(repairAdmin,"index")),beforeRepairIndex);
  assert.equal(await returnGit(copiedWorktree.path,["show",":README.md"]),copySourceIndex);
  assert.equal(await readFile(path.join(copiedWorktree.path,"README.md"),"utf8"),await readFile(path.join(project,"README.md"),"utf8"));
  await clickButton("Close");
  backgroundToolMode="start";
  await type('textarea[aria-label="Message ShadowCode"]', "Start the managed project watcher and read its log.");
  await click('button[aria-label="Send task"]');
  await until("Model background start approval",()=>execute("return !!document.querySelector('.approval')?.textContent.includes('Start background process: model-watcher')"));
  assert.ok((await execute("return document.querySelector('.approval .code').textContent")).includes(modelBackgroundCommand));
  assert.ok((await execute("return document.querySelector('.approval').textContent")).includes("including cancellation"));
  assert.ok(!(await api("GET","/api/background")).tasks.some(task=>task.name==="model-watcher"));
  await until("New approval remains visible",()=>execute("const card=document.querySelector('.approval').getBoundingClientRect();const stream=document.querySelector('.chat-stream').getBoundingClientRect();return card.top>=stream.top && card.bottom<=stream.bottom"));
  for (const theme of ["light","dark"]) {
    await execute("document.documentElement.dataset.theme=arguments[0]",[theme]);
    await screenshot(`background-approval-${theme}`);
    await accessibility(`background-approval-${theme}`);
  }
  await execute("document.documentElement.dataset.theme='light'");
  await wd("POST",`/session/${session}/window/rect`,{width:620,height:850});
  if(await execute("return !!document.querySelector('.jump-latest')")) await click('.jump-latest');
  await until("Compact approval visible",()=>execute("const card=document.querySelector('.approval').getBoundingClientRect();const stream=document.querySelector('.chat-stream').getBoundingClientRect();return card.top>=stream.top && card.bottom<=stream.bottom"));
  await screenshot("background-approval-compact");
  await accessibility("background-approval-compact");
  assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST",`/session/${session}/window/rect`,{width:1380,height:920});
  await click(".approval button.primary");
  await until("Model watcher task complete",()=>execute("return [...document.querySelectorAll('.msg-agent')].some(item=>item.textContent.includes('reports model-background-ready')) && !!document.querySelector('button[aria-label=\"Send task\"]')"));
  const modelBackground=await api("GET",`/api/background/${modelBackgroundId}`);
  assert.equal(modelBackground.status,"RUNNING");
  assert.ok(modelBackground.origin_task_id);
  await click('button[aria-label="Terminal"]');
  await clickButton("Background");
  await until("Model watcher shared with Background panel",()=>execute("return [...document.querySelectorAll('.bg-task')].some(item=>item.textContent.includes('model-watcher') && item.textContent.includes('model-background-ready'))"));
  await click('button.drawer-close');
  backgroundToolMode="stop"; backgroundToolCalls=0;
  await type('textarea[aria-label="Message ShadowCode"]', "Stop the managed project watcher.");
  await click('button[aria-label="Send task"]');
  await until("Model background stop approval",()=>execute("return !!document.querySelector('.approval')?.textContent.includes('Stop background process: model-watcher')"));
  const stopPrompt=await execute("return document.querySelector('.approval .code').textContent");
  assert.ok(stopPrompt.includes(modelBackgroundId) && stopPrompt.includes(modelBackgroundCommand));
  await click(".approval button.primary");
  await until("Model watcher stopped",()=>execute("return [...document.querySelectorAll('.msg-agent')].some(item=>item.textContent.includes('watcher stopped and cleanup finished')) && !!document.querySelector('button[aria-label=\"Send task\"]')"));
  assert.equal((await api("GET",`/api/background/${modelBackgroundId}`)).status,"CANCELLED");
  await until("Model-started watcher child cleanup",async()=>dead((await readFile(path.join(project,"model-background-child.pid"),"utf8")).trim()));
  await until("Model-started watcher parent cleanup",()=>dead(modelBackground.pid));
  backgroundToolMode=false;
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
  workflowCalls = 0;
  await click('button[aria-label="Terminal"]'); await clickButton("Skills");
  await until("Installed plugin skill",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Use /window-bundle--audit')"));
  await execute("[...document.querySelectorAll('.drawer-body details')].find(d=>d.querySelector('summary')?.textContent.includes('window-bundle--audit')).querySelector('summary').click()");
  await clickButton("Use /window-bundle--audit");
  await type('textarea[aria-label="Message ShadowCode"]',"README.md");
  await click('button[aria-label="Send task"]');
  await until("Installed plugin skill executed",()=>execute("return [...document.querySelectorAll('.msg-note')].some(e=>e.textContent.includes('.shadowcode/skills/window-bundle--audit/SKILL.md')) && !!document.querySelector('button[aria-label=\"Send task\"]')"));
  assert.equal(workflowCalls,2);
  // Uninstall preserves a user's edited workflow while removing the unchanged hook.
  await writeFile(path.join(project,".shadowcode/skills/window-bundle--audit/SKILL.md"),"My retained plugin skill");
  await openSettings();
  await clickButton("Plugins");
  await until("Plugin removal ready",()=>execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Remove window-bundle')"));
  await clickButton("Remove window-bundle");
  await until("Preserved plugin edits visible",()=>execute("return !!document.querySelector('.plugin-settings [role=status]')?.textContent.includes('SKILL.md')"));
  assert.equal(await readFile(path.join(project,".shadowcode/skills/window-bundle--audit/SKILL.md"),"utf8"),"My retained plugin skill");
  assert.ok(!(await api("GET","/api/hooks")).hooks.some(h=>h.name==="window-bundle--finished"));
  await clickButton("Close");
  workflowMode = false;
  await type('textarea[aria-label="Message ShadowCode"]', "/status");
  await click('button[aria-label="Send task"]');
  await until("Durable command card", () => execute("return [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Permission mode'))"));
  const selectedMemory = (await api("POST", "/api/commands/run", {name:"memory", args:""})).metadata;
  assert.ok(selectedMemory.task_id, "The selected conversation has an exact task for notes");
  await type('textarea[aria-label="Message ShadowCode"]', `/memory --task ${selectedMemory.task_id} Keep the desktop fixture local.`);
  await click('button[aria-label="Send task"]');
  await until("Task memory card", () => execute("return [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Note saved') && e.textContent.includes('Keep the desktop fixture local.'))"));
  await type('textarea[aria-label="Message ShadowCode"]', "/understand");
  await click('button[aria-label="Send task"]');
  await until("Native project map card", () => execute("return [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Project map') && e.textContent.includes('Test candidates'))"));
  assert.equal(await execute("return [...document.querySelectorAll('.tool-card .markdown h1')].some(e=>e.textContent==='Project map')"), true);
  await type('textarea[aria-label="Message ShadowCode"]', "/doctor");
  await click('button[aria-label="Send task"]');
  await until("Native diagnostic card", () => execute("return [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Native diagnostics') && e.textContent.includes('SQLite quick_check passed'))"));
  await screenshot("inspection");
  await accessibility("inspection");
  await type('textarea[aria-label="Message ShadowCode"]', "/health");
  await click('button[aria-label="Send task"]');
  await until("Native diagnostics in Health", () => execute("return [...document.querySelectorAll('.diagnostic-check')].some(e=>e.textContent.includes('Actual model response') && e.textContent.includes('Not checked'))"));
  assert.equal(await execute("return [...document.querySelectorAll('.status-row code')].every(e=>e.getBoundingClientRect().right<=e.closest('.tool-card').getBoundingClientRect().right+1 && e.scrollWidth<=e.clientWidth+1)"), true, "Command details wrap within the narrowed conversation");
  assert.equal(await execute("return [...document.querySelectorAll('button')].some(e=>e.textContent.trim()==='Auto-fix')"), false);
  assert.equal(await execute("return [...document.querySelectorAll('.diagnostic-check')].some(e=>e.textContent.includes('Actual model response') && e.querySelector('.health-bad'))"), false);
  await screenshot("diagnostics");
  await accessibility("diagnostics");
  await wd("POST", `/session/${session}/refresh`, {});
  await until("Skill and command reload", () => execute("return [...document.querySelectorAll('.msg-note')].some(e=>e.textContent.includes('.shadow/skills/audit.md')) && [...document.querySelectorAll('.tool-card')].some(e=>e.textContent.includes('Permission mode'))"));
  goalMode = true;
  await click('button[aria-label="Terminal"]');
  await clickButton("Goals");
  await type('textarea[aria-label="Goal instruction"]', "Create goal.txt containing goal-native-ok and verify its contents.");
  await clickButton("Plan & run");
  await until("Goal command approval", () => execute("return !!document.querySelector('.approval button.primary')"));
  const goalApproval = (await api("GET", "/api/approvals")).approvals.find(a=>a.command.includes("goal-native-ok"));
  assert.ok(goalApproval, "The displayed goal has an exact pending command approval");
  const otherSelection = await api("POST", "/api/sessions", {workspace:project,title:"Approval selection regression"});
  await api("POST", `/api/sessions/${otherSelection.id}/activate`);
  await click(`.approval[data-approval-id="${goalApproval.id}"] button.primary`);
  await until("Goal approval resolved for its own conversation", async () => !(await api("GET", "/api/approvals")).approvals.some(a=>a.id===goalApproval.id));
  await api("POST", `/api/sessions/${goalApproval.session_id}/activate`);
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
  // Populate only this disposable profile with a history larger than the old cap.
  const historySession=await api("POST","/api/sessions",{workspace:project,title:"Long saved history"});
  const historyDb=new DatabaseSync(path.join(stateDirectory,"shadow-agent.db"));
  historyDb.exec("PRAGMA busy_timeout=5000; BEGIN");
  const putHistory=historyDb.prepare("INSERT INTO events(ts,type,session_id,task_id,payload) VALUES(?,'model.delta',?,NULL,?)");
  for(let index=0;index<12000;index++)putHistory.run(Date.now()/1000,historySession.id,JSON.stringify({text:`History message ${index}`,message_id:`history-${index}`}));
  historyDb.exec("COMMIT");historyDb.close();
  await execute("localStorage.setItem('shadow:selected',arguments[0])",[historySession.id]);
  await wd("POST",`/session/${session}/refresh`,{});
  await until("Bounded history snapshot",()=>execute("return document.querySelector('.chat-inner')?.textContent.includes('History message 11999') && !!document.querySelector('[aria-label=\"Conversation history\"]')"));
  assert.equal(await execute("return document.querySelectorAll('.msg-agent').length"),128);
  let historyPages=0;
  while(await execute("return [...document.querySelectorAll('button')].some(b=>b.textContent==='Older messages'&&!b.disabled)")) {
    const first=await execute("return document.querySelector('.msg-agent .markdown')?.textContent");
    await clickButton("Older messages");
    await until("Earlier saved page rendered",()=>execute("return document.querySelector('.msg-agent .markdown')?.textContent!==arguments[0] && !document.querySelector('[aria-label=\"Conversation history\"] [role=status]')",[first]));
    assert.ok(await execute("return document.querySelectorAll('.msg-agent').length<=128"));
    assert.ok(++historyPages<100,"History cursor must make progress");
  }
  assert.equal(await execute("return document.querySelector('.msg-agent .markdown').textContent.trim()"),"History message 0");
  assert.equal(historyPages,93);
  await screenshot("history");await accessibility("history");
  await execute("document.documentElement.dataset.theme='dark'");await accessibility("history-dark");await execute("document.documentElement.dataset.theme='light'");
  await wd("POST", `/session/${session}/window/rect`,{width:620,height:850});await accessibility("history-compact");assert.equal(await execute("return document.documentElement.scrollWidth<=window.innerWidth+1"),true);
  await wd("POST", `/session/${session}/window/rect`,{width:1380,height:920});
  await clickButton("Newer messages");
  await until("Newer saved page",()=>execute("return document.querySelector('.msg-agent .markdown')?.textContent.trim()==='History message 96'"));
  await clickButton("Latest activity");
  await until("Floating latest returns to live history",()=>execute("return document.querySelector('.msg-agent .markdown')?.textContent.trim()==='History message 11872'"));
  await clickButton("Older messages");
  await until("Earlier history reopened",()=>execute("return document.querySelector('.msg-agent .markdown')?.textContent.trim()==='History message 11744'"));
  await clickButton("Latest messages");
  await until("Latest history restored",()=>execute("return document.querySelector('.msg-agent .markdown')?.textContent.trim()==='History message 11872'"));
  assert.equal(await execute("return document.querySelectorAll('.msg-agent').length"),128);
  await click('button[aria-label="Terminal"]');
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
  await until("Native shutdown", () => dead(version.pid));
  await until("Terminal cleanup", () => dead(child));
  await until("Background child cleanup", () => dead(backgroundChild));
  await until("Background process cleanup", () => dead(shutdownBackground.pid));
  await writeFile(path.join(artifacts, "result.json"), JSON.stringify({ passed: true, version: version.version, runtime: version.runtime, modelRequests: requests, requestedModels, mcpCalls, mcpHttpCalls, queueRequests, checks: [...(defaultProfile ? ["repeated default-profile activation preserves the live window and its extraction"] : []), "native worktree creation, trust prompt, reviewed removal, missing checkout rescue, reviewed return without committing, preserved original metadata and light/dark/compact accessibility",
    "native built-in/custom plugin review and installation, separate hook activation, actual installed skill execution, removal with local edits preserved", "embedded interface", "Rust IPC", "native approval", "real file write and terminal verification", "durable reload", "queued follow-ups, project FIFO, cross-conversation cancellation, reload selection, model/mode snapshots and inherited results", "native routing controls and persisted model/fallback notices", "background start, live output, coexistence with tasks, stop, and child cleanup on quit", "model background tools, visible exact-command approvals, light/dark/compact approval accessibility, shared panel state and immediate stop cleanup", "selected skill execution, mode enforcement, provenance and durable command cards", "project inspection, native diagnostic cards and Health status distinctions", "task-note command persistence and goal approval after backend selection changes", "reviewed hook activation and disable in Settings, actual completion check, durable hook result", "shared CLI engine with independent project selection and background controls", "MCP registration, exact-argument approval, stdio and authenticated HTTP results, credential redaction, cleanup and removal", "cancellation", "compact layout", "native light/dark/compact/goals/routing/background/skills/hooks/mcp/mcp-http/inspection/diagnostics and queue accessibility", "goal creation, automatic milestone progression, verification, live transcript and pause", "12,000-event history with bounded DOM, all 94 pages, newer/latest navigation and three-layout accessibility", "managed native shutdown"] }, null, 2));
  console.log("Native desktop window passed: IPC, approval, file/terminal tools, routing, background processes, MCP, shared CLI isolation, replay, cancellation, layout, goals, accessibility, shutdown.");
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
  if(httpPeer && httpPeer.exitCode === null && httpPeer.signalCode === null) { const closed=new Promise(resolve=>httpPeer.once("close",resolve)); httpPeer.kill("SIGKILL"); await closed; }
  output.end();
  await rm(scratch, { recursive: true, force: true });
}
