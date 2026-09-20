// Actual native executable protocol probe. No display or Python is required.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import {
  mkdtemp,
  mkdir,
  readFile,
  readlink,
  writeFile,
  rm,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
const binary =
  process.env.SHADOW_DESKTOP_BINARY ||
  path.join(root, "target/debug/shadowcode");
const binaryArgs = JSON.parse(process.env.SHADOW_CLI_ARGS || "[]");
assert.ok(
  Array.isArray(binaryArgs) && binaryArgs.every((v) => typeof v === "string"),
);
const artifacts = path.resolve(
  process.env.SHADOW_MCP_ARTIFACTS ||
    path.join(root, "artifacts/native-mcp-server"),
);
await mkdir(artifacts, { recursive: true });
await rm(path.join(artifacts, "result.json"), { force: true });
await rm(path.join(artifacts, "failure.txt"), { force: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-mcp-server-"));
const project = path.join(scratch, "project");
const otherProject = path.join(scratch, "unrelated");
const profile = path.join(scratch, "profile");
await mkdir(project);
await mkdir(otherProject);
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(path.join(project, "README.md"), "# MCP project\n");
const env = { ...process.env, TMPDIR: scratch };
delete env.DISPLAY;
delete env.WAYLAND_DISPLAY;
const children = new Set(),
  sockets = new Set();
let modelError,
  requests = 0;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(label, fn, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (modelError) throw modelError;
    const value = await fn();
    if (value) return value;
    await delay(30);
  }
  throw new Error(`${label} timed out`);
}
function launch(args, workspace = project) {
  const child = spawn(
    binary,
    [...binaryArgs, "--profile", profile, "--workspace", workspace, ...args],
    { env, stdio: "pipe", detached: true },
  );
  children.add(child);
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  let stdout = "",
    stderr = "";
  child.stdout.on("data", (b) => {
    stdout += b;
    if (stdout.length > 8_000_000) child.kill("SIGKILL");
  });
  child.stderr.on("data", (b) => {
    stderr += b;
    if (stderr.length > 1_000_000) child.kill("SIGKILL");
  });
  const done = new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code, signal) => {
      children.delete(child);
      resolve({ code, signal, stdout, stderr });
    });
  });
  return {
    child,
    done,
    get stderr() {
      return stderr;
    },
  };
}
async function finish(run) {
  let timer;
  return Promise.race([
    run.done,
    new Promise((_, reject) => {
      timer = setTimeout(
        () => reject(new Error("MCP process did not stop")),
        15000,
      );
    }),
  ]).finally(() => clearTimeout(timer));
}
async function nativePid(child) {
  // AppImage has a launcher process. Follow only this invocation's descendants
  // and kill its actual native executable, not merely the forwarding wrapper.
  const pending = [child.pid];
  while (pending.length) {
    const pid = pending.pop();
    const executable = await readlink(`/proc/${pid}/exe`).catch(() => "");
    if (path.basename(executable) === "shadowcode") return pid;
    const descendants = await readFile(
      `/proc/${pid}/task/${pid}/children`,
      "utf8",
    ).catch(() => "");
    pending.push(
      ...descendants.trim().split(/\s+/).filter(Boolean).map(Number),
    );
  }
  throw new Error("Could not locate the native MCP gateway process");
}
async function connect(args = []) {
  const run = launch(["mcp", "serve", ...args]);
  let buffer = "",
    next = 1;
  const pending = new Map();
  run.child.stdout.on("data", (b) => {
    buffer += b.toString();
    while (buffer.includes("\n")) {
      const at = buffer.indexOf("\n");
      const line = buffer.slice(0, at);
      buffer = buffer.slice(at + 1);
      try {
        const value = JSON.parse(line);
        assert.equal(value.jsonrpc, "2.0");
        const p = pending.get(value.id);
        if (p) {
          pending.delete(value.id);
          clearTimeout(p.timer);
          if (value.error) p.reject(new Error(JSON.stringify(value.error)));
          else p.resolve(value.result);
        }
      } catch (error) {
        modelError = error;
      }
    }
  });
  const rpc = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = next++;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`No MCP response: ${method}`));
      }, 10000);
      pending.set(id, { resolve, reject, timer });
      run.child.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
      );
    });
  const info = await rpc("initialize", {
    protocolVersion: "2025-11-25",
    capabilities: {},
    clientInfo: { name: "shadowcode-binary-probe", version: "1" },
  });
  assert.equal(info.serverInfo.name, "ShadowCode");
  run.child.stdin.write(
    `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`,
  );
  return {
    ...run,
    rpc,
    call: async (name, args = {}) => {
      const result = await rpc("tools/call", { name, arguments: args });
      assert.deepEqual(
        JSON.parse(result.content[0].text),
        result.structuredContent,
      );
      return result;
    },
    close: async () => {
      run.child.stdin.end();
      const result = await finish(run);
      assert.equal(result.code, 0, JSON.stringify(result));
      assert.equal(buffer, "");
    },
  };
}
const model = createServer(async (req, res) => {
  try {
    let body = "";
    for await (const b of req) body += b;
    const payload = JSON.parse(body);
    assert.equal(payload.model, "mcp-native-fixture");
    const index = requests++;
    const prompt = JSON.stringify(payload.messages);
    if (prompt.includes("LEASE_UNRELATED")) return;
    const tool = (name, args) => ({
      id: `mcp-fixture-${index}`,
      type: "function",
      function: { name, arguments: JSON.stringify(args) },
    });
    const calls = [
      tool("read_file", { path: "README.md" }),
      tool("write_file", {
        path: "result.txt",
        content: "native-mcp-server-ok\n",
        expected_hash: "missing",
      }),
      tool("exec", {
        command: 'test "$(cat result.txt)" = native-mcp-server-ok',
      }),
    ];
    if (prompt.includes("LEASE_TERMINAL")) {
      calls[0] = tool("exec", {
        command: "sleep 60 & echo $! > mcp-child.pid; wait",
      });
    }
    const message =
      index < 3 || prompt.includes("LEASE_TERMINAL")
        ? {
            role: "assistant",
            content: "Performing the next check",
            tool_calls: [calls[prompt.includes("LEASE_TERMINAL") ? 0 : index]],
          }
        : {
            role: "assistant",
            content:
              "Created result.txt and verified it with a successful command.",
          };
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        choices: [
          {
            message,
            finish_reason: message.tool_calls ? "tool_calls" : "stop",
          },
        ],
        usage: { prompt_tokens: 30, completion_tokens: 10, total_tokens: 40 },
      }),
    );
  } catch (error) {
    modelError = error;
    res.writeHead(500);
    res.end();
  }
});
model.on("connection", (socket) => {
  sockets.add(socket);
  socket.on("close", () => sockets.delete(socket));
});
await new Promise((resolve) => model.listen(0, "127.0.0.1", resolve));
await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: {
      default: "mcp-native-fixture",
      name: "mcp-native-fixture",
      provider: "local",
      endpoint: `http://127.0.0.1:${model.address().port}/v1`,
      context_limit: 16384,
    },
    trusted_workspaces: [project, otherProject],
    agent: { max_steps: 6, model_retries: 0 },
  }),
);
try {
  const registration = await finish(launch(["mcp", "register"]));
  assert.equal(registration.code, 0, registration.stderr);
  const spec = JSON.parse(registration.stdout).mcpServers.shadowcode;
  assert.equal(spec.command, path.resolve(binary));
  assert.ok(spec.args.includes(project));
  assert.ok(spec.args.includes(profile));
  assert.ok(!spec.args.includes("--allow-write"));
  const malformed = launch(["--json", "mcp", "serve"]);
  malformed.child.stdin.end();
  const rejected = await finish(malformed);
  assert.equal(rejected.code, 1);
  assert.equal(rejected.stdout, "");
  const readonly = await connect();
  const catalog = await readonly.rpc("tools/list");
  assert.equal(catalog.tools.length, 16);
  const inspected = await readonly.call("shadow_understand");
  assert.equal(inspected.structuredContent.saved, false);
  assert.equal(
    (await readonly.call("shadow_doctor")).structuredContent.report.runtime,
    "rust",
  );
  assert.equal((await readonly.rpc("prompts/list")).prompts.length, 2);
  const status = await readonly.call("shadow_status");
  assert.equal(status.structuredContent.status.workspace, project);
  assert.equal(
    (
      await readonly.call("shadow_memory", {
        action: "append",
        note: "forbidden",
      })
    ).isError,
    true,
  );
  await readonly.close();
  const c = await connect(["--allow-write", "--allow-approvals"]);
  const started = await c.call("shadow_run", {
    task: "Inspect README, create result.txt containing native-mcp-server-ok and run a verification command.",
    permission_level: "workspace",
  });
  assert.equal(started.isError, false, JSON.stringify(started));
  const job = started.structuredContent.job;
  let approved = false;
  const done = await until("Owned MCP task completion", async () => {
    const snapshot = (await c.call("shadow_jobs", { job_id: job.id }))
      .structuredContent;
    for (const approval of snapshot.approvals) {
      assert.match(approval.command, /result.txt/);
      assert.equal(
        (
          await c.call("shadow_approve", {
            approval_id: approval.id,
            decision: "approve",
          })
        ).isError,
        false,
      );
      approved = true;
    }
    return (
      !["running", "queued", "cancelling"].includes(snapshot.job.status) &&
      snapshot.job
    );
  });
  assert.equal(done.status, "completed", JSON.stringify(done));
  assert.ok(approved);
  assert.equal(
    await readFile(path.join(project, "result.txt"), "utf8"),
    "native-mcp-server-ok\n",
  );
  const point = await c.call("shadow_checkpoint", { task_id: job.task_id });
  assert.equal(point.structuredContent.checkpoint.changes, 1);
  assert.equal(
    (await c.call("shadow_rollback", { task_id: job.task_id, confirm: true }))
      .isError,
    false,
  );
  assert.equal(
    await readFile(path.join(project, "result.txt")).then(
      () => true,
      () => false,
    ),
    false,
  );
  assert.equal((await c.rpc("resources/list")).resources.length, 4);
  assert.ok(
    (await c.rpc("resources/read", { uri: "shadow://project" })).contents
      .length,
  );
  const plan = await c.rpc("resources/read", { uri: "shadow://plan" });
  assert.ok(plan.contents.length);
  await c.close();
  const testClient = await connect(["--allow-write", "--allow-approvals"]);
  const testJob = (
    await testClient.call("shadow_test", {
      command: "printf standalone-test-ok",
    })
  ).structuredContent.job;
  assert.ok(testJob?.id);
  const testApproval = await until(
    "Native test approval",
    async () =>
      (await testClient.call("shadow_jobs", { job_id: testJob.id }))
        .structuredContent.approvals[0],
  );
  assert.equal(testApproval.command, "printf standalone-test-ok");
  assert.equal(
    (
      await testClient.call("shadow_approve", {
        approval_id: testApproval.id,
        decision: "approve",
      })
    ).isError,
    false,
  );
  const testResult = await until("Native test completion", async () => {
    const job = (await testClient.call("shadow_jobs", { job_id: testJob.id }))
      .structuredContent.job;
    return !["queued", "running", "cancelling"].includes(job.status) && job;
  });
  assert.equal(testResult.status, "completed");
  assert.equal(testResult.result.command.stdout, "standalone-test-ok");
  assert.equal(testResult.result.command.exit_code, 0);
  const savedNotes = await testClient.call("shadow_memory", {action:"append", scope:"task", task_id:testJob.task_id, note:"Preserve the exact standalone test result."});
  assert.equal(savedNotes.isError, false);
  const taskNotes = await testClient.call("shadow_memory", {action:"read", scope:"task", task_id:testJob.task_id});
  assert.match(taskNotes.structuredContent.task, /exact standalone test result/);
  await testClient.close();
  // The owning server closed its profile cleanly; another invocation can reopen.
  const reopened = await finish(launch(["--json", "status"]));
  assert.equal(reopened.code, 0, reopened.stderr);
  assert.equal(requests, 4);
  const engine = launch(["serve"]);
  await until("Persistent engine startup", () =>
    engine.stderr.includes("serving"),
  );
  const cli = async (args, workspace = project) => {
    const result = await finish(launch(["--json", ...args], workspace));
    assert.equal(result.code, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  const unrelated = await cli(
    ["run", "LEASE_UNRELATED keep this task running", "--detach"],
    otherProject,
  );
  const gateway = await connect(["--allow-write", "--allow-approvals"]);
  const owned = (
    await gateway.call("shadow_run", {
      task: "LEASE_TERMINAL start the long-running verification",
      permission_level: "workspace",
    })
  ).structuredContent.job;
  assert.ok(owned?.id);
  const pending = await until(
    "Owned terminal approval",
    async () =>
      (await gateway.call("shadow_jobs", { job_id: owned.id }))
        .structuredContent.approvals[0],
  );
  assert.match(pending.command, /sleep 60/);
  assert.equal(
    (
      await gateway.call("shadow_approve", {
        approval_id: pending.id,
        decision: "approve",
      })
    ).isError,
    false,
  );
  const childPid = await until("Owned terminal child", async () =>
    readFile(path.join(project, "mcp-child.pid"), "utf8").catch(() => null),
  );
  const queued = (
    await gateway.call("shadow_run", {
      task: "This queued task must never call the model",
      queue: true,
    })
  ).structuredContent.job;
  assert.ok(queued?.id);
  process.kill(await nativePid(gateway.child), "SIGKILL");
  const killed = await finish(gateway);
  assert.ok(
    killed.signal === "SIGKILL" || killed.code === 137,
    JSON.stringify(killed),
  );
  await until("Killed gateway tasks cancelled", async () => {
    const a = await cli(["jobs", owned.id]),
      b = await cli(["jobs", queued.id]);
    return a.status === "cancelled" && b.status === "cancelled";
  });
  await until("Owned terminal child stopped", async () => {
    const stat = await readFile(`/proc/${childPid.trim()}/stat`, "utf8").catch(
      () => "",
    );
    return !stat || stat.includes(") Z ");
  });
  assert.equal(
    (await cli(["jobs", unrelated.id], otherProject)).status,
    "running",
  );
  await cli(["jobs", unrelated.id, "--cancel"], otherProject);
  process.kill(-engine.child.pid, "SIGTERM");
  assert.equal((await finish(engine)).code, 0);
  assert.equal(
    requests,
    6,
    "The cancelled queued task must not start another model request",
  );
  await writeFile(
    path.join(artifacts, "result.json"),
    JSON.stringify(
      {
        passed: true,
        modelRequests: requests,
        tools: 16,
        resources: 4,
        checks: [
          "standalone headless native MCP process",
          "JSON-RPC-only stdout",
          "registration uses stable executable/project/profile",
          "read-only default",
          "native task with exact approval",
          "real write, terminal verification, checkpoint and rollback",
          "resource reads",
          "native project inspection, diagnostics, approved test execution and task notes without model calls",
          "EOF cleanup and profile restart",
          "SIGKILL gateway cleanup on a shared engine, including queued tasks and an active command child",
          "unrelated detached task survives gateway death",
        ],
      },
      null,
      2,
    ),
  );
  console.log(`Native MCP server passed with ${requests} model requests.`);
} catch (error) {
  await writeFile(
    path.join(artifacts, "failure.txt"),
    String(error.stack || error),
  );
  throw error;
} finally {
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {}
  }
  for (const socket of sockets) socket.destroy();
  model.close();
  await delay(300);
  await rm(scratch, { recursive: true, force: true });
}
