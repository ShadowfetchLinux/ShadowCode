// Active resource soak. Periodic work against a live `shadowcode serve`.
// Cache warmup is not a leak. Records a time series; does not overwrite
// ~/Applications or the primary profile.
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const seconds = Number(process.env.QUAL_P6_SECONDS || 21600);
const sampleMs = Number(process.env.QUAL_P6_SAMPLE_MS || 30000);
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
if (!existsSync(binary)) throw new Error(`missing ${binary}`);

const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-p6-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
await mkdir(project, { recursive: true });
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(path.join(project, "README.md"), "# soak\nfixture-read-value\n");
await writeFile(path.join(project, "notes.txt"), "bounded file for soak reads\n");

let modelError;
let modelHits = 0;
const model = createServer(async (req, res) => {
  try {
    let body = "";
    for await (const chunk of req) body += chunk;
    JSON.parse(body || "{}");
    modelHits += 1;
    const prompt = body;
    const hang = prompt.includes("HANG");
    if (hang) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.flushHeaders();
      return;
    }
    const read = !prompt.includes("\"role\":\"tool\"");
    const message = read
      ? {
          role: "assistant",
          content: "Inspecting",
          tool_calls: [
            {
              id: `soak-${modelHits}`,
              type: "function",
              function: {
                name: "read_file",
                arguments: JSON.stringify({ path: "README.md" }),
              },
            },
          ],
        }
      : { role: "assistant", content: "Soak fixture completed." };
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        choices: [
          {
            message,
            finish_reason: message.tool_calls ? "tool_calls" : "stop",
          },
        ],
        usage: { prompt_tokens: 16, completion_tokens: 8, total_tokens: 24 },
      }),
    );
  } catch (error) {
    modelError = error;
    res.writeHead(500);
    res.end();
  }
});
await new Promise((resolve) => model.listen(0, "127.0.0.1", resolve));
const endpoint = `http://127.0.0.1:${model.address().port}/v1`;
await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: {
      default: "soak-fixture",
      name: "soak-fixture",
      provider: "local",
      endpoint,
      context_limit: 16384,
    },
    onboarding: { completed: true, workspace: project },
    ui: { notify: false },
    permissions: { approve_shell: false },
  }),
);

const children = new Set();
function launch(args, extraEnv = {}) {
  const child = spawn(binary, ["--profile", profile, "--workspace", project, ...args], {
    env: { ...process.env, ...extraEnv },
    stdio: "pipe",
    detached: true,
  });
  const output = { stdout: "", stderr: "" };
  child.stdout.on("data", (data) => {
    output.stdout += data;
    if (output.stdout.length > 1_000_000) output.stdout = output.stdout.slice(-200_000);
  });
  child.stderr.on("data", (data) => {
    output.stderr += data;
    if (output.stderr.length > 1_000_000) output.stderr = output.stderr.slice(-200_000);
  });
  children.add(child);
  const done = new Promise((resolve) => {
    child.once("close", (code, signal) => {
      children.delete(child);
      resolve({ code, signal, ...output });
    });
  });
  return { child, output, done };
}

function readStatus(pid) {
  const text = execFileSync("cat", [`/proc/${pid}/status`], { encoding: "utf8" });
  const num = (key) => Number((text.match(new RegExp(`${key}:\\s+(\\d+)`)) || [])[1] || 0);
  return {
    rss_kb: num("VmRSS"),
    vm_kb: num("VmSize"),
    threads: num("Threads"),
  };
}

function samplePid(pid) {
  const status = readStatus(pid);
  const fds = Number(
    execFileSync("bash", ["-lc", `ls /proc/${pid}/fd | wc -l`], { encoding: "utf8" }).trim(),
  );
  const childCount = Number(
    execFileSync("bash", ["-lc", `ps --ppid ${pid} -o pid= | wc -l`], {
      encoding: "utf8",
    }).trim(),
  );
  let cpu_pct = 0;
  try {
    cpu_pct = Number(
      execFileSync("ps", ["-p", String(pid), "-o", "%cpu="], { encoding: "utf8" }).trim(),
    );
  } catch {}
  const dbPath = path.join(profile, "state/shadow-agent.db");
  let db_bytes = 0;
  let sqlite_conns = 0;
  try {
    db_bytes = Number(execFileSync("stat", ["-c", "%s", dbPath], { encoding: "utf8" }).trim());
  } catch {}
  try {
    const lsof = execFileSync("bash", ["-lc", `lsof -p ${pid} 2>/dev/null | grep -c shadow-agent.db || true`], {
      encoding: "utf8",
    }).trim();
    sqlite_conns = Number(lsof) || 0;
  } catch {}
  return { ...status, fds, children: childCount, cpu_pct, db_bytes, sqlite_conns };
}

const serve = launch(["--json", "serve"]);
await new Promise((resolve) => setTimeout(resolve, 1500));
if (!serve.child.pid || serve.child.exitCode != null) {
  throw new Error(`serve failed: ${JSON.stringify(serve.output)}`);
}
launch(["--json", "trust"]);
launch(["--json", "models", "--use", "soak-fixture", "--provider", "local", "--endpoint", endpoint, "--context-limit", "16384"]);
await new Promise((resolve) => setTimeout(resolve, 400));

const samples = [];
const activity = [];
const started = Date.now();
let cycles = 0;
let sessionId;

async function cycle() {
  cycles += 1;
  const errors = [];
  const step = async (name, fn) => {
    try {
      await fn();
      activity.push({ elapsed_s: Math.round((Date.now() - started) / 1000), name, ok: true });
    } catch (error) {
      errors.push(`${name}: ${error}`);
      activity.push({
        elapsed_s: Math.round((Date.now() - started) / 1000),
        name,
        ok: false,
        error: String(error),
      });
    }
  };
  await step("task_create", async () => {
    const run = launch(["--json", "run", `Soak cycle ${cycles}: inspect README.md`]);
    const result = await Promise.race([
      run.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("run timeout")), 20000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || result.stdout || `exit ${result.code}`);
    try {
      const parsed = JSON.parse(result.stdout);
      sessionId = parsed.session_id || parsed.session || sessionId;
    } catch {}
  });
  await step("history", async () => {
    const run = launch(["--json", "sessions"]);
    const result = await Promise.race([
      run.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("sessions timeout")), 10000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || `exit ${result.code}`);
  });
  await step("jobs_events", async () => {
    const run = launch(["--json", "jobs"]);
    const result = await Promise.race([
      run.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("jobs timeout")), 10000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || `exit ${result.code}`);
  });
  await step("file_read_task", async () => {
    const run = launch(["--json", "run", "Read notes.txt and README.md"]);
    const result = await Promise.race([
      run.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("read timeout")), 20000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || `exit ${result.code}`);
  });
  await step("bounded_exec", async () => {
    const run = launch(["--json", "exec", "printf soak-ok", "--timeout", "5"]);
    const result = await Promise.race([
      run.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("exec timeout")), 10000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || `exit ${result.code}`);
  });
  await step("cancel", async () => {
    const hung = launch(["--json", "run", "HANG this soak task", "--detach"]);
    await Promise.race([
      hung.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("detach timeout")), 15000)),
    ]);
    let jobId;
    try {
      jobId = JSON.parse(hung.output.stdout).id;
    } catch {}
    if (!jobId) throw new Error(`no job id: ${hung.output.stdout}`);
    const cancel = launch(["--json", "jobs", jobId, "--cancel"]);
    const result = await Promise.race([
      cancel.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("cancel timeout")), 10000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || `exit ${result.code}`);
  });
  await step("background", async () => {
    const start = launch(["--json", "background", "start", "--name", `soak-${cycles}`, "--command", "printf ready; sleep 8"]);
    const startedJob = await Promise.race([
      start.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("bg start timeout")), 10000)),
    ]);
    if (startedJob.code !== 0) throw new Error(startedJob.stderr || `exit ${startedJob.code}`);
    let id;
    try {
      id = JSON.parse(startedJob.stdout).id;
    } catch {}
    const list = launch(["--json", "background", "list"]);
    await list.done;
    if (id) {
      const stop = launch(["--json", "background", "stop", id]);
      await Promise.race([
        stop.done,
        new Promise((_, reject) => setTimeout(() => reject(new Error("bg stop timeout")), 10000)),
      ]);
    }
  });
  await step("connect_disconnect", async () => {
    const a = launch(["--json", "status"]);
    const b = launch(["--json", "doctor"]);
    await Promise.all([a.done, b.done]);
  });
  await step("compact_pressure", async () => {
    const blob = `Compact soak ${cycles}: ${"history ".repeat(400)}inspect README.md`;
    const run = launch(["--json", "run", blob]);
    const result = await Promise.race([
      run.done,
      new Promise((_, reject) => setTimeout(() => reject(new Error("compact timeout")), 25000)),
    ]);
    if (result.code !== 0) throw new Error(result.stderr || `exit ${result.code}`);
  });
  if (modelError) errors.push(`model: ${modelError}`);
  return errors;
}

function record(extra = {}) {
  try {
    samples.push({
      elapsed_s: Math.round((Date.now() - started) / 1000),
      ...samplePid(serve.child.pid),
      model_hits: modelHits,
      cycles,
      ...extra,
    });
  } catch (error) {
    samples.push({
      elapsed_s: Math.round((Date.now() - started) / 1000),
      error: String(error),
      cycles,
    });
  }
}

record({ phase: "warmup" });
const sampleTimer = setInterval(() => record({ phase: "sample" }), sampleMs);
let stopping = false;
const stop = async (reason) => {
  if (stopping) return;
  stopping = true;
  clearInterval(sampleTimer);
  record({ phase: "final", reason });
  for (const child of [...children]) {
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {}
  }
  await new Promise((resolve) => setTimeout(resolve, 800));
  for (const child of [...children]) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {}
  }
  model.close();
  const first = samples.find((s) => s.rss_kb) || {};
  const last = [...samples].reverse().find((s) => s.rss_kb) || {};
  const report = {
    duration_s: Math.round((Date.now() - started) / 1000),
    requested_s: seconds,
    six_hour: seconds >= 21600,
    scratch,
    samples,
    activity_ok: activity.filter((a) => a.ok).length,
    activity_err: activity.filter((a) => !a.ok).length,
    cycles,
    model_hits: modelHits,
    rss_delta_kb: (last.rss_kb || 0) - (first.rss_kb || 0),
    vm_delta_kb: (last.vm_kb || 0) - (first.vm_kb || 0),
    fd_delta: (last.fds || 0) - (first.fds || 0),
    thread_delta: (last.threads || 0) - (first.threads || 0),
    leak_claimed: false,
    note: "Cache growth and SQLite file growth after activity are not leaks unless they stay monotonic after idle.",
  };
  await writeFile(path.join(artifacts, "p6-active-soak.json"), JSON.stringify(report, null, 2));
  await rm(scratch, { recursive: true, force: true }).catch(() => {});
};

process.on("SIGTERM", () => stop("sigterm"));
process.on("SIGINT", () => stop("sigint"));

const deadline = Date.now() + seconds * 1000;
while (Date.now() < deadline && !stopping) {
  await cycle();
  record({ phase: "after_cycle" });
  const remaining = deadline - Date.now();
  if (remaining <= 0) break;
  await new Promise((resolve) => setTimeout(resolve, Math.min(20000, remaining)));
}
await stop("completed");
console.log(
  JSON.stringify({
    duration_s: Math.round((Date.now() - started) / 1000),
    requested_s: seconds,
    cycles,
    artifact: path.join(artifacts, "p6-active-soak.json"),
  }),
);
