import assert from "node:assert/strict";
import { spawn, execFile } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import {
  Client as ClientV2,
  StreamableHTTPClientTransport as HttpV2,
} from "@modelcontextprotocol/client";
import { StdioClientTransport as StdioV2 } from "@modelcontextprotocol/client/stdio";
import { Client as ClientV1 } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport as HttpV1 } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { StdioClientTransport as StdioV1 } from "@modelcontextprotocol/sdk/client/stdio.js";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary =
  process.env.SHADOW_DESKTOP_BINARY ||
  path.join(root, "target/debug/shadowcode");
const binaryArgs = JSON.parse(process.env.SHADOW_CLI_ARGS || "[]");
assert.ok(
  Array.isArray(binaryArgs) &&
    binaryArgs.every((arg) => typeof arg === "string"),
);
const model = process.env.SHADOW_MCP_MODEL;
const selected = process.env.SHADOW_MCP_PAIR;
const pairs = ["v1-stdio", "v1-http", "v2-stdio", "v2-http"];
assert.ok(!selected || pairs.includes(selected));
const artifacts = path.resolve(
  process.env.SHADOW_MCP_PEER_ARTIFACTS ||
    path.join(root, "artifacts/native-mcp-peer"),
);
await mkdir(artifacts, { recursive: true });
await rm(path.join(artifacts, "result.json"), { force: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-mcp-peer-"));
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const run = promisify(execFile);
async function timed(label, promise, ms = 15000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} timed out`)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
async function until(label, check, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const result = await check();
    if (result) return result;
    await delay(50);
  }
  throw new Error(`${label} timed out`);
}
const original =
  "export function total(items) { return items.reduce((sum, item) => sum + item.price, 0); }\n";
const tests = `import assert from 'node:assert/strict';
import { test } from 'node:test';
import { total } from './totals.mjs';
test('multiplies quantity', () => assert.equal(total([{price: 3, quantity: 2}, {price: 4, quantity: 2}]), 14));
test('zero quantity', () => assert.equal(total([{price: 9, quantity: 0}]), 0));
test('empty items', () => assert.equal(total([]), 0));
`;

async function probe(pair) {
  const [version, kind] = pair.split("-");
  const [Client, Stdio, Http] =
    version === "v1"
      ? [ClientV1, StdioV1, HttpV1]
      : [ClientV2, StdioV2, HttpV2];
  const project = path.join(scratch, pair, "project");
  const profile = path.join(scratch, pair, "profile");
  await mkdir(project, { recursive: true });
  await mkdir(path.join(profile, "config"), { recursive: true });
  await writeFile(path.join(project, "totals.mjs"), original);
  await writeFile(path.join(project, "totals.test.mjs"), tests);
  await writeFile(
    path.join(profile, "config/config.yaml"),
    JSON.stringify({
      model: {
        provider: "ollama",
        endpoint: model ? "http://127.0.0.1:11434/v1" : "http://127.0.0.1:1/v1",
        name: model || "unused-fixture",
        default: model || "unused-fixture",
        context_limit: 16384,
      },
      trusted_workspaces: [project],
      permissions: { approve_shell: true },
      agent: { max_steps: 14, max_task_tokens: 100000, model_retries: 0 },
    }),
  );
  const args = [
    ...binaryArgs,
    "--profile",
    profile,
    "--workspace",
    project,
    "mcp",
    "serve",
    "--allow-write",
    "--allow-approvals",
  ];
  const token = randomBytes(32).toString("hex");
  const env = { ...process.env, TMPDIR: scratch, SHADOW_MCP_PEER_TOKEN: token };
  delete env.DISPLAY;
  delete env.WAYLAND_DISPLAY;
  let gateway,
    exited,
    stderr = "",
    stdout = "",
    transport,
    client,
    stdioPid;
  const events = [],
    approvals = [];
  let report;
  try {
    if (kind === "http") {
      gateway = spawn(
        binary,
        [
          ...args,
          "--http",
          "127.0.0.1:0",
          "--token-env",
          "SHADOW_MCP_PEER_TOKEN",
        ],
        { env, stdio: ["ignore", "pipe", "pipe"], detached: true },
      );
      exited = new Promise((resolve, reject) => {
        gateway.once("error", reject);
        gateway.once("close", (code, signal) => resolve({ code, signal }));
      });
      // Observe early spawn errors while readiness is still being checked.
      void exited.catch(() => {});
      gateway.stdout.on("data", (data) => {
        stdout += data;
      });
      gateway.stderr.on("data", (data) => {
        stderr = (stderr + data).slice(-100000);
      });
      const url = await until(
        "HTTP readiness",
        () =>
          /MCP HTTP listening on (http:\/\/127\.0\.0\.1:\d+\/mcp);/.exec(
            stderr,
          )?.[1],
      );
      transport = new Http(new URL(url), {
        requestInit: { headers: { Authorization: `Bearer ${token}` } },
        reconnectionOptions: { maxRetries: 0 },
      });
    } else {
      transport = new Stdio({
        command: binary,
        args,
        env,
        cwd: project,
        stderr: "pipe",
      });
      transport.stderr.on("data", (data) => {
        stderr = (stderr + data).slice(-100000);
      });
    }
    client = new Client({ name: `shadowcode-peer-${pair}`, version: "1" });
    await timed("SDK connection", client.connect(transport));
    if (kind === "stdio") stdioPid = transport.pid;
    const call = async (name, args = {}) => {
      const result = await timed(
        name,
        client.callTool({ name, arguments: args }),
      );
      assert.equal(result.isError, false, JSON.stringify(result));
      assert.deepEqual(
        JSON.parse(result.content[0].text),
        result.structuredContent,
      );
      return result.structuredContent;
    };
    assert.equal((await client.listTools()).tools.length, 17);
    assert.equal((await client.listResources()).resources.length, 4);
    assert.equal((await client.listPrompts()).prompts.length, 2);
    assert.ok(
      (await client.readResource({ uri: "shadow://project" })).contents.length,
    );
    assert.equal((await call("shadow_status")).status.workspace, project);
    const command = model
      ? "node --test totals.test.mjs"
      : `printf ${pair}-verified`;
    const started = model
      ? await call("shadow_run", {
          task: "Inspect totals.mjs and totals.test.mjs. Fix total to add price multiplied by quantity for every item, with zero for empty items. Change only totals.mjs. Do not change the tests or create files. Verify with exactly node --test totals.test.mjs and report the observed result.",
          permission_level: "workspace",
        })
      : await call("shadow_test", { command });
    let after = 0;
    const finished = await until(
      "Owned task completion",
      async () => {
        const snapshot = await call("shadow_jobs", {
          job_id: started.job.id,
          after,
          limit: 100,
        });
        for (const event of snapshot.events || []) {
          events.push(event);
          after = Math.max(after, event.id);
        }
        for (const approval of snapshot.approvals) {
          const allowed = approval.command === command;
          approvals.push({ command: approval.command, allowed });
          await call("shadow_approve", {
            approval_id: approval.id,
            decision: allowed ? "approve" : "deny",
          });
        }
        return (
          !["queued", "running", "cancelling"].includes(snapshot.job.status) &&
          snapshot.job
        );
      },
      model ? 300000 : 15000,
    );
    assert.equal(finished.status, "completed", JSON.stringify(finished));
    assert.ok(approvals.some((a) => a.allowed));
    if (model) {
      assert.equal(
        await readFile(path.join(project, "totals.test.mjs"), "utf8"),
        tests,
      );
      const verified = await run(
        process.execPath,
        ["--test", "totals.test.mjs"],
        { cwd: project, timeout: 10000 },
      );
      assert.match(verified.stdout, /# pass 3/);
      const checkpoint = await call("shadow_checkpoint", {
        task_id: finished.task_id,
      });
      assert.equal(checkpoint.checkpoint.changes, 1);
      await call("shadow_rollback", {
        task_id: finished.task_id,
        confirm: true,
      });
      assert.equal(
        await readFile(path.join(project, "totals.mjs"), "utf8"),
        original,
      );
    } else {
      assert.equal(finished.result.command.stdout, `${pair}-verified`);
      assert.equal(finished.result.command.exit_code, 0);
    }
    report = {
      pair,
      passed: true,
      model: model || null,
      job: finished,
      approvals,
      events,
      checkpointRestored: Boolean(model),
    };
  } finally {
    let closeError;
    await timed(
      "SDK close",
      client?.close() || transport?.close() || Promise.resolve(),
    ).catch((error) => {
      closeError = error;
    });
    if (gateway) {
      try {
        process.kill(-gateway.pid, "SIGTERM");
      } catch {}
      try {
        const result = await timed("Gateway close", exited);
        assert.equal(result.code, 0, stderr);
      } catch (error) {
        try {
          process.kill(-gateway.pid, "SIGKILL");
        } catch {}
        await exited;
        throw error;
      }
      assert.equal(stdout, "");
    }
    if (stdioPid) {
      await until("Stdio owner process exit", async () => {
        const stat = await readFile(`/proc/${stdioPid}/stat`, "utf8").catch(
          (error) => {
            if (error.code === "ENOENT") return "";
            throw error;
          },
        );
        return !stat || /\) [ZX] /.test(stat);
      });
    }
    await writeFile(
      path.join(artifacts, `${pair}.json`),
      JSON.stringify({ ...report, events, approvals, stderr }, null, 2),
    );
    if (closeError) throw closeError;
  }
  // Reopening proves the native owner released its profile and persisted work.
  const status = await run(
    binary,
    [
      ...binaryArgs,
      "--profile",
      profile,
      "--workspace",
      project,
      "--json",
      "status",
    ],
    { env, timeout: 15000 },
  );
  JSON.parse(status.stdout);
  console.log(
    `Official TypeScript ${pair} passed${model ? ` with ${model}` : ""}.`,
  );
  return {
    pair,
    passed: true,
    model: model || null,
    approvals: approvals.length,
    checkpointRestored: Boolean(model),
  };
}

try {
  const results = [];
  for (const pair of selected ? [selected] : pairs)
    results.push(await probe(pair));
  await writeFile(
    path.join(artifacts, "result.json"),
    JSON.stringify({ passed: true, results }, null, 2),
  );
} catch (error) {
  await writeFile(
    path.join(artifacts, "failure.txt"),
    String(error.stack || error),
  );
  throw error;
} finally {
  await rm(scratch, { recursive: true, force: true });
}
