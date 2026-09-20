// Actual native executable protocol probe. No display or Python is required.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
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
const profile = path.join(scratch, "profile");
await mkdir(project);
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(path.join(project, "README.md"), "# MCP project\n");
const env = { ...process.env };
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
function launch(args) {
  const child = spawn(
    binary,
    [...binaryArgs, "--profile", profile, "--workspace", project, ...args],
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
  return { child, done };
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
    const message =
      index < 3
        ? {
            role: "assistant",
            content: "Performing the next check",
            tool_calls: [calls[index]],
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
          { message, finish_reason: index < 3 ? "tool_calls" : "stop" },
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
    trusted_workspaces: [project],
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
  assert.equal(catalog.tools.length, 12);
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
  assert.equal((await c.rpc("resources/list")).resources.length, 3);
  const plan = await c.rpc("resources/read", { uri: "shadow://plan" });
  assert.ok(plan.contents.length);
  await c.close();
  // The owning server closed its profile cleanly; another invocation can reopen.
  const reopened = await finish(launch(["--json", "status"]));
  assert.equal(reopened.code, 0, reopened.stderr);
  await writeFile(
    path.join(artifacts, "result.json"),
    JSON.stringify(
      {
        passed: true,
        modelRequests: requests,
        tools: 12,
        resources: 3,
        checks: [
          "standalone headless native MCP process",
          "JSON-RPC-only stdout",
          "registration uses stable executable/project/profile",
          "read-only default",
          "native task with exact approval",
          "real write, terminal verification, checkpoint and rollback",
          "resource reads",
          "EOF cleanup and profile restart",
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
