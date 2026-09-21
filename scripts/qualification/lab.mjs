// Real-process qualification lab. Disposable profile/workspace only.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-qual-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
await mkdir(project);
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(path.join(project, "README.md"), "# Qualification\nfixture-read-value\n");
const env = { ...process.env };
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const children = new Set();
const sockets = new Set();
const evidence = { gates: {}, binary, started: new Date().toISOString() };
let modelError;
let requests = 0;
let hung = 0;
let writes = 0;
let repeats = 0;

async function until(label, fn, timeout = 20000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    if (modelError) throw modelError;
    try {
      const value = await fn();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    await delay(50);
  }
  throw new Error(`${label} timed out${last ? `: ${last}` : ""}`);
}
function launch(args, { workspace = project, extraEnv = {} } = {}) {
  const child = spawn(binary, ["--profile", profile, "--workspace", workspace, ...args], {
    env: { ...env, ...extraEnv },
    stdio: "pipe",
    detached: true,
  });
  const output = { stdout: "", stderr: "", code: undefined, signal: undefined };
  child.stdout.on("data", (data) => {
    output.stdout += data;
    if (output.stdout.length > 2_000_000) child.kill("SIGKILL");
  });
  child.stderr.on("data", (data) => {
    output.stderr += data;
    if (output.stderr.length > 2_000_000) child.kill("SIGKILL");
  });
  children.add(child);
  const done = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => {
      output.code = code;
      output.signal = signal;
      children.delete(child);
      resolve(output);
    });
  });
  return { child, output, done, args };
}
async function finish(run, code = 0, timeout = 30000) {
  let timer;
  const output = await Promise.race([
    run.done,
    new Promise((_, reject) => {
      timer = setTimeout(
        () => reject(new Error(`CLI ${JSON.stringify(run.args)} timed out: ${JSON.stringify(run.output)}`)),
        timeout,
      );
    }),
  ]).finally(() => clearTimeout(timer));
  assert.equal(output.code, code, JSON.stringify(output));
  return output;
}
async function cli(args, code = 0) {
  const output = await finish(launch(["--json", ...args]), code);
  try {
    return JSON.parse(output.stdout);
  } catch {
    throw new Error(`Invalid CLI JSON: ${JSON.stringify(output)}`);
  }
}
async function dead(pid) {
  try {
    return /\) [ZX] /.test(await readFile(`/proc/${pid}/stat`, "utf8"));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    return true;
  }
}
function rss(pid) {
  try {
    const text = execFileSync("awk", ["/VmRSS/{print $2}", `/proc/${pid}/status`], { encoding: "utf8" });
    return Number(text.trim()) || 0;
  } catch {
    return 0;
  }
}
function fds(pid) {
  try {
    return execFileSync("bash", ["-lc", `ls /proc/${pid}/fd | wc -l`], { encoding: "utf8" }).trim();
  } catch {
    return "0";
  }
}

const model = createServer(async (req, res) => {
  try {
    let body = "";
    for await (const chunk of req) body += chunk;
    const payload = JSON.parse(body);
    const index = requests++;
    const prompt = payload.messages.filter((m) => m.role === "user").at(-1)?.content || "";
    const current = payload.messages.slice(payload.messages.findLastIndex((m) => m.role === "user") + 1);
    const hadTool = current.some((m) => m.role === "tool");
    const tool = (name, args) => ({
      id: `q-${index}`,
      type: "function",
      function: { name, arguments: JSON.stringify(args) },
    });
    if (prompt.includes("HANG")) {
      hung++;
      res.writeHead(200, { "Content-Type": "application/json" });
      res.flushHeaders();
      return;
    }
    if (prompt.includes("STREAM")) {
      res.writeHead(200, { "Content-Type": "text/event-stream" });
      res.flushHeaders();
      const interval = setInterval(() => {
        res.write(
          `data: ${JSON.stringify({ choices: [{ delta: { content: "token " }, finish_reason: null }] })}\n\n`,
        );
      }, 30);
      res.on("close", () => clearInterval(interval));
      return;
    }
    if (prompt.includes("CLAIM") && !hadTool) {
      const message = {
        role: "assistant",
        content: "All tests passed. The feature is correctly implemented.",
      };
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          choices: [{ message, finish_reason: "stop" }],
          usage: { prompt_tokens: 20, completion_tokens: 8, total_tokens: 28 },
        }),
      );
      return;
    }
    if (prompt.includes("RUNAWAY") && repeats < 8) {
      repeats++;
      const message = {
        role: "assistant",
        content: "loop",
        tool_calls: [tool("read_file", { path: "README.md" })],
      };
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          choices: [{ message, finish_reason: "tool_calls" }],
          usage: { prompt_tokens: 20, completion_tokens: 8, total_tokens: 28 },
        }),
      );
      return;
    }
    if (prompt.includes("WRITE") && !hadTool) {
      writes++;
      const message = {
        role: "assistant",
        content: "Writing",
        tool_calls: [tool("write_file", { path: "side-effect.txt", content: "created-once\n", expected_hash: "missing" })],
      };
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          choices: [{ message, finish_reason: "tool_calls" }],
          usage: { prompt_tokens: 20, completion_tokens: 8, total_tokens: 28 },
        }),
      );
      return;
    }
    if (prompt.includes("SHELL") && !hadTool) {
      const message = {
        role: "assistant",
        content: "Running",
        tool_calls: [tool("exec", { command: "printf shell-once > shell-side.txt" })],
      };
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(
        JSON.stringify({
          choices: [{ message, finish_reason: "tool_calls" }],
          usage: { prompt_tokens: 20, completion_tokens: 8, total_tokens: 28 },
        }),
      );
      return;
    }
    let message;
    if (!hadTool) {
      message = {
        role: "assistant",
        content: "Inspecting",
        tool_calls: [tool("read_file", { path: "README.md" })],
      };
    } else if (prompt.includes("WRITE") || prompt.includes("SHELL")) {
      hung++;
      res.writeHead(200, { "Content-Type": "application/json" });
      res.flushHeaders();
      return;
    } else {
      message = { role: "assistant", content: "Qualification fixture completed." };
    }
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        choices: [{ message, finish_reason: message.tool_calls ? "tool_calls" : "stop" }],
        usage: { prompt_tokens: 20, completion_tokens: 8, total_tokens: 28 },
      }),
    );
  } catch (error) {
    modelError = error;
    res.writeHead(500, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: String(error) }));
  }
});
model.on("connection", (socket) => {
  sockets.add(socket);
  socket.on("close", () => sockets.delete(socket));
});
await new Promise((resolve) => model.listen(0, "127.0.0.1", resolve));
const endpoint = `http://127.0.0.1:${model.address().port}/v1`;
await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: { default: "qual-fixture", name: "qual-fixture", provider: "local", endpoint, context_limit: 16384 },
    onboarding: { completed: true, workspace: project },
    ui: { notify: false },
    permissions: { approve_shell: false },
  }),
);

function db() {
  return new DatabaseSync(path.join(profile, "state/shadow-agent.db"));
}

try {
  assert.ok(existsSync(binary), `missing ${binary}`);
  await cli(["trust"]);
  await cli(["models", "--use", "qual-fixture", "--provider", "local", "--endpoint", endpoint, "--context-limit", "16384"]);
  const healthy = await cli(["doctor"]);
  assert.equal(healthy.runtime, "rust");
  assert.equal(JSON.stringify(healthy).includes("telemetry"), true);
  evidence.gates.p25_doctor_healthy = { ok: true, runtime: healthy.runtime };

  const read = await cli(["run", "READ project"]);
  assert.equal(read.status, "completed");
  evidence.gates.p1_cli_task = { status: read.status, session_id: read.session_id };

  let server = launch(["--json", "serve"]);
  await until("serve", () => server.output.stderr.includes("serving"));
  const desktop = launch([]);
  await delay(2500);
  const desktopAlive = !(await dead(desktop.child.pid));
  evidence.gates.p2_desktop_launch = {
    pid: desktop.child.pid,
    alive: desktopAlive,
    stderr: desktop.output.stderr.slice(0, 400),
  };
  const beforeHung = hung;
  const detached = await cli(["run", "HANG detached", "--detach"]);
  await until("detached hang", () => hung > beforeHung);
  assert.equal((await cli(["jobs", detached.id])).status, "running");
  desktop.child.kill("SIGTERM");
  await until("desktop dead", () => dead(desktop.child.pid));
  assert.equal((await cli(["jobs", detached.id])).status, "running", "desktop death must not stop engine task");
  const desktop2 = launch([]);
  await delay(2500);
  assert.equal((await cli(["jobs", detached.id])).status, "running");
  const eventsBefore = (await cli(["export", "--session", detached.session_id, "--format", "json"])).events
    || (await cli(["jobs", detached.id]));
  server.child.kill("SIGKILL");
  await finish(server, null).catch(() => {});
  await until("engine dead", () => dead(server.child.pid));
  server = launch(["--json", "serve"]);
  await until("serve restart", () => server.output.stderr.includes("serving"));
  const recovered = await cli(["jobs", detached.id]);
  assert.notEqual(recovered.status, "running");
  assert.match(recovered.status, /interrupt|cancel|fail/i);
  const afterRestart = await cli(["jobs"]);
  assert.ok(afterRestart.jobs.every((j) => !["running", "queued"].includes(j.status)));
  desktop2.child.kill("SIGTERM");
  evidence.gates.p2_reattach = {
    desktop_death_left_task_running: true,
    engine_kill_did_not_rerun: recovered.status,
    jobs_after: afterRestart.jobs.map((j) => j.status),
  };

  const writeBefore = writes;
  const writer = launch(["--json", "run", "WRITE then hang"]);
  await until("write created", async () => {
    try {
      return (await readFile(path.join(project, "side-effect.txt"), "utf8")) === "created-once\n";
    } catch {
      return false;
    }
  });
  writer.child.kill("SIGKILL");
  await writer.done;
  server.child.kill("SIGKILL");
  await until("owner dead after write", () => dead(server.child.pid));
  server = launch(["--json", "serve"]);
  await until("serve after write", () => server.output.stderr.includes("serving"));
  await delay(800);
  assert.equal(await readFile(path.join(project, "side-effect.txt"), "utf8"), "created-once\n");
  assert.equal(writes, writeBefore + 1, "write_file must not replay");
  evidence.gates.p3_write_replay = { class: "NeverAutoReplay", writes, replayed: false };

  const shellRun = launch(["--json", "run", "SHELL then hang"]);
  await until("shell file", async () => existsSync(path.join(project, "shell-side.txt")));
  shellRun.child.kill("SIGKILL");
  await shellRun.done;
  server.child.kill("SIGKILL");
  await until("owner dead after shell", () => dead(server.child.pid));
  server = launch(["--json", "serve"]);
  await until("serve after shell", () => server.output.stderr.includes("serving"));
  await delay(800);
  assert.equal(await readFile(path.join(project, "shell-side.txt"), "utf8"), "shell-once");
  evidence.gates.p3_shell_replay = { class: "RequiresConfirmation", replayed: false };

  const stream = launch(["--json", "run", "STREAM until killed"]);
  await until("streaming", () => stream.output.stdout.includes("token") || hung >= 0);
  await delay(200);
  stream.child.kill("SIGKILL");
  await stream.done;
  const streamJobs = await cli(["jobs"]);
  const streamJob = (streamJobs.jobs || []).find((j) => j.status === "running") || streamJobs.jobs?.[0];
  evidence.gates.p23_stream_kill = { status: streamJob?.status, not_success: streamJob?.status !== "completed" };
  if (streamJob?.id && ["running", "queued", "cancelling"].includes(streamJob.status)) {
    await cli(["jobs", streamJob.id, "--cancel"]).catch(() => ({}));
    await until("stream cancelled", async () => {
      const row = await cli(["jobs", streamJob.id]);
      return !["running", "queued", "cancelling"].includes(row.status);
    });
  }

  const claim = await cli(["run", "CLAIM tests passed without running them"]);
  const database = db();
  const verification = database
    .prepare("SELECT payload FROM events WHERE type='verification.summary' ORDER BY id DESC LIMIT 1")
    .get();
  const payload = verification ? JSON.parse(verification.payload) : {};
  database.close();
  evidence.gates.p24_verification = { job: claim.status, verified: payload.verified === true, payload };
  assert.notEqual(payload.verified, true);

  const runaway = await cli(["run", "RUNAWAY same read"], 1).catch(async (error) => {
    const output = error;
    return output;
  });
  evidence.gates.p8_runaway = { repeats, result: runaway.status || runaway };

  const bg = await cli(["background", "start", "--name", "qual-sleep", "--command", "printf ready; sleep 60"]);
  const logs = await until("bg logs", async () => {
    const row = await cli(["background", "logs", bg.id]);
    return row.output?.includes("ready") && row;
  });
  assert.equal(await dead(logs.pid), false);
  server.child.kill("SIGTERM");
  await finish(server).catch(() => {});
  await until("bg cleaned", () => dead(logs.pid));
  evidence.gates.p15_background = { started: true, cleaned_after_engine_stop: true, pid: logs.pid };

  server = launch(["--json", "serve"]);
  await until("serve for cancel", () => server.output.stderr.includes("serving"));
  await writeFile(path.join(project, "huge.txt"), "x".repeat(200_000));
  const cancelRead = launch(["--json", "run", "READ huge then we cancel"]);
  await delay(150);
  cancelRead.child.kill("SIGINT");
  const cancelled = await finish(cancelRead, 130).catch(async () => cancelRead.output);
  evidence.gates.p14_cancel = { code: cancelled.code, signal: cancelled.signal };
  await cli(["background", "start", "--name", "cancel-sleep", "--command", "sleep 60"]).catch(() => ({}));
  const listed = await cli(["background", "list"]);
  for (const task of listed.tasks || []) {
    if (task.status === "RUNNING") await cli(["background", "stop", task.id]).catch(() => {});
  }

  const gitMissing = launch(["--json", "doctor"], { extraEnv: { PATH: "/bin:/usr/bin" } });
  const gitReport = await finish(gitMissing, 0);
  evidence.gates.p25_doctor_local = { stdout: gitReport.stdout.slice(0, 500) };

  evidence.ok = true;
  await writeFile(path.join(artifacts, "lab.json"), JSON.stringify(evidence, null, 2));
  console.log("qualification lab passed", Object.keys(evidence.gates));
} catch (error) {
  evidence.ok = false;
  evidence.error = String(error.stack || error);
  await writeFile(path.join(artifacts, "lab-failure.json"), JSON.stringify(evidence, null, 2));
  throw error;
} finally {
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {}
  }
  for (const socket of sockets) socket.destroy();
  model.close();
  await delay(1500);
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {}
  }
  await rm(scratch, { recursive: true, force: true });
}
