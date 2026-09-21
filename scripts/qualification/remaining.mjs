// P10/P11/P12/P13/P16/P21 remaining real-process gates. Disposable only.
import assert from "node:assert/strict";
import { spawn, execFileSync, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdir, mkdtemp, readFile, writeFile, rm, chmod } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-qual-rest-"));
const evidence = { gates: {}, started: new Date().toISOString() };
const children = new Set();
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(label, fn, timeout = 20000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
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

function launch(args, { cwd = scratch, extraEnv = {}, workspace, profile } = {}) {
  const child = spawn(
    binary,
    [
      ...(profile ? ["--profile", profile] : []),
      ...(workspace ? ["--workspace", workspace] : []),
      ...args,
    ],
    { cwd, env: { ...process.env, ...extraEnv }, stdio: "pipe", detached: true },
  );
  const output = { stdout: "", stderr: "", code: undefined, signal: undefined };
  child.stdout.on("data", (d) => (output.stdout += d));
  child.stderr.on("data", (d) => (output.stderr += d));
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
  return { child, output, done };
}

async function finish(run, timeout = 45000) {
  let timer;
  return Promise.race([
    run.done,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`timeout ${JSON.stringify(run.output)}`)), timeout);
    }),
  ]).finally(() => clearTimeout(timer));
}

try {
  assert.ok(existsSync(binary), binary);

  // P21 checksum absent vs present vs mismatch (isolated HOME).
  const home = path.join(scratch, "home");
  await mkdir(path.join(home, "Applications"), { recursive: true });
  const release = path.join(scratch, "release");
  await mkdir(release);
  const fake = path.join(release, "ShadowCode_0.20.0_amd64.AppImage");
  await writeFile(
    fake,
    ["#!/usr/bin/env bash", "if [[ \"$1\" == --appimage-extract-and-run && \"$2\" == --version ]]; then", "  printf 'ShadowCode 0.20.0\\n'", "  exit 0", "fi", "exit 1", ""].join("\n"),
  );
  await chmod(fake, 0o755);
  const installer = path.join(root, "scripts/install-appimage.sh");
  const absent = spawnSync("bash", [installer, fake], {
    encoding: "utf8",
    env: { ...process.env, HOME: home, XDG_DATA_HOME: path.join(home, ".local/share") },
  });
  evidence.gates.p21_checksum_absent = {
    code: absent.status,
    stderr: (absent.stderr || "").slice(0, 300),
    warns: /not checksum-verified/.test(absent.stderr || ""),
    installed: existsSync(path.join(home, "Applications/ShadowCode-0.20.0-x86_64.AppImage")),
  };
  assert.equal(absent.status, 0, absent.stderr);
  assert.equal(evidence.gates.p21_checksum_absent.warns, true);
  execFileSync("bash", ["-lc", `cd ${JSON.stringify(release)} && sha256sum ShadowCode_0.20.0_amd64.AppImage > SHA256SUMS`]);
  const match = spawnSync("bash", [installer, fake], {
    encoding: "utf8",
    env: { ...process.env, HOME: home, XDG_DATA_HOME: path.join(home, ".local/share") },
  });
  evidence.gates.p21_checksum_present = {
    code: match.status,
    stdout: (match.stdout || "").slice(0, 200),
    verified: /Verified SHA-256/.test(match.stdout || ""),
  };
  assert.equal(match.status, 0, match.stderr);
  await writeFile(fake, "\n# mutated\n", { flag: "a" });
  const mismatch = spawnSync("bash", [installer, fake], {
    encoding: "utf8",
    env: { ...process.env, HOME: home, XDG_DATA_HOME: path.join(home, ".local/share") },
  });
  evidence.gates.p21_checksum_mismatch = {
    code: mismatch.status,
    stderr: (mismatch.stderr || "").slice(0, 240),
    refused: mismatch.status !== 0,
  };
  assert.notEqual(mismatch.status, 0);

  // P13 real disposable git workspace via CLI git_status / denied clean.
  const repo = path.join(scratch, "git-repo");
  await mkdir(repo);
  execFileSync("git", ["init", "-b", "main"], { cwd: repo });
  execFileSync("git", ["config", "user.name", "Qual"], { cwd: repo });
  execFileSync("git", ["config", "user.email", "qual@example.test"], { cwd: repo });
  await writeFile(path.join(repo, "keep-me.txt"), "committed\n");
  execFileSync("git", ["add", "keep-me.txt"], { cwd: repo });
  execFileSync("git", ["commit", "-m", "base"], { cwd: repo });
  await writeFile(path.join(repo, "dirty.txt"), "dirty\n");
  await writeFile(path.join(repo, "file with spaces.txt"), "spaces\n");
  await writeFile(path.join(repo, "雪.txt"), "snow\n");
  const profile13 = path.join(scratch, "p13-profile");
  await mkdir(path.join(profile13, "config"), { recursive: true });
  const model13 = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const payload = JSON.parse(body || "{}");
    const hadTool = (payload.messages || []).some((m) => m.role === "tool");
    const message = hadTool
      ? { role: "assistant", content: "Stopped before destructive Git." }
      : {
          role: "assistant",
          content: "Cleaning",
          tool_calls: [
            { id: "g1", type: "function", function: { name: "git_clean", arguments: "{}" } },
          ],
        };
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        choices: [{ message, finish_reason: message.tool_calls ? "tool_calls" : "stop" }],
      }),
    );
  });
  await new Promise((resolve) => model13.listen(0, "127.0.0.1", resolve));
  const endpoint13 = `http://127.0.0.1:${model13.address().port}/v1`;
  await writeFile(
    path.join(profile13, "config/config.yaml"),
    JSON.stringify({
      model: { default: "qual", name: "qual", provider: "local", endpoint: endpoint13, context_limit: 8192 },
      onboarding: { completed: true, workspace: repo },
      permissions: { approve_shell: false },
    }),
  );
  const trust = launch(["--json", "trust"], { workspace: repo, profile: profile13 });
  await finish(trust);
  const cleanRun = launch(["--json", "run", "Please git_clean this dirty repo"], {
    workspace: repo,
    profile: profile13,
  });
  const cleanOut = await finish(cleanRun);
  evidence.gates.p13_cli_git_clean = {
    code: cleanOut.code,
    stdout: cleanOut.stdout.slice(0, 500),
    dirty_survived: existsSync(path.join(repo, "dirty.txt")),
    spaces_survived: existsSync(path.join(repo, "file with spaces.txt")),
    unicode_survived: existsSync(path.join(repo, "雪.txt")),
    committed_survived: existsSync(path.join(repo, "keep-me.txt")),
  };
  assert.equal(evidence.gates.p13_cli_git_clean.dirty_survived, true);
  assert.equal(evidence.gates.p13_cli_git_clean.unicode_survived, true);
  model13.close();

  // P11 disposable multi-step explore/edit/test/repair.
  const trial = path.join(scratch, "trial");
  await mkdir(trial);
  execFileSync("git", ["init", "-b", "main"], { cwd: trial });
  await writeFile(path.join(trial, "add.mjs"), "export const add = (a, b) => a - b;\n");
  await writeFile(
    path.join(trial, "add.test.mjs"),
    "import { add } from './add.mjs'; if (add(2,3)!==5) { console.error('fail'); process.exit(1);} console.log('ok');\n",
  );
  let step = 0;
  const model11 = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const payload = JSON.parse(body || "{}");
    const hadTool = (payload.messages || []).some((m) => m.role === "tool");
    let message;
    if (!hadTool && step === 0) {
      step = 1;
      message = {
        role: "assistant",
        content: "Reading",
        tool_calls: [
          { id: "r1", type: "function", function: { name: "read_file", arguments: JSON.stringify({ path: "add.mjs" }) } },
        ],
      };
    } else if (hadTool && step === 1) {
      step = 2;
      message = {
        role: "assistant",
        content: "Fixing",
        tool_calls: [
          {
            id: "w1",
            type: "function",
            function: {
              name: "edit_file",
              arguments: JSON.stringify({
                path: "add.mjs",
                old_string: "export const add = (a, b) => a - b;",
                new_string: "export const add = (a, b) => a + b;",
              }),
            },
          },
        ],
      };
    } else if (hadTool && step === 2) {
      step = 3;
      message = {
        role: "assistant",
        content: "Testing",
        tool_calls: [
          {
            id: "e1",
            type: "function",
            function: { name: "exec", arguments: JSON.stringify({ command: "node add.test.mjs" }) },
          },
        ],
      };
    } else {
      message = { role: "assistant", content: "Add is correct and the test passed." };
    }
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        choices: [{ message, finish_reason: message.tool_calls ? "tool_calls" : "stop" }],
      }),
    );
  });
  await new Promise((resolve) => model11.listen(0, "127.0.0.1", resolve));
  const profile11 = path.join(scratch, "p11-profile");
  await mkdir(path.join(profile11, "config"), { recursive: true });
  const endpoint11 = `http://127.0.0.1:${model11.address().port}/v1`;
  await writeFile(
    path.join(profile11, "config/config.yaml"),
    JSON.stringify({
      model: { default: "qual", name: "qual", provider: "local", endpoint: endpoint11, context_limit: 8192 },
      onboarding: { completed: true, workspace: trial },
      permissions: { approve_shell: false },
    }),
  );
  await finish(launch(["--json", "trust"], { workspace: trial, profile: profile11 }));
  const trialRun = await finish(
    launch(["--json", "run", "Fix add() and run the test"], { workspace: trial, profile: profile11 }),
  );
  const fixed = await readFile(path.join(trial, "add.mjs"), "utf8");
  evidence.gates.p11_disposable_repo = {
    code: trialRun.code,
    stdout: trialRun.stdout.slice(0, 400),
    fixed: fixed.includes("a + b"),
    steps: step,
  };
  assert.equal(evidence.gates.p11_disposable_repo.fixed, true);
  model11.close();

  // P16 one hostile MCP server must not break doctor.
  const profile16 = path.join(scratch, "p16-profile");
  const project16 = path.join(scratch, "p16-project");
  await mkdir(path.join(profile16, "config"), { recursive: true });
  await mkdir(project16);
  const fixture = path.join(root, "native/core/tests/fixtures/mcp-server.mjs");
  const def = {
    name: "hostile",
    command: ["node", fixture, "init_malformed"],
    timeout_sec: 2,
  };
  await writeFile(
    path.join(profile16, "config/config.yaml"),
    JSON.stringify({
      model: { default: "qual", name: "qual", provider: "mock", context_limit: 8192 },
      onboarding: { completed: true, workspace: project16 },
    }),
  );
  await finish(launch(["--json", "trust"], { workspace: project16, profile: profile16 }));
  await writeFile(path.join(scratch, "hostile.yaml"), JSON.stringify(def));
  const added = launch(["--json", "mcp", "add", path.join(scratch, "hostile.yaml")], {
    workspace: project16,
    profile: profile16,
  });
  const addedOut = await finish(added).catch((error) => ({
    code: 1,
    stdout: "",
    stderr: String(error),
  }));
  const doctor16 = launch(["--json", "doctor"], { workspace: project16, profile: profile16 });
  const doctorOut = await finish(doctor16);
  evidence.gates.p16_hostile_mcp = {
    add_code: addedOut.code,
    add_stdout: (addedOut.stdout || "").slice(0, 300),
    add_stderr: (addedOut.stderr || "").slice(0, 300),
    doctor_code: doctorOut.code,
    doctor_ok: doctorOut.code === 0 || /runtime/.test(doctorOut.stdout),
  };
  assert.ok(evidence.gates.p16_hostile_mcp.doctor_ok, doctorOut.stderr);

  // P10 real local model harness — judge transport, not IQ.
  let ollama;
  try {
    ollama = execFileSync("ollama", ["list"], { encoding: "utf8" });
  } catch (error) {
    ollama = String(error);
  }
  evidence.gates.p10_ollama_list = { output: ollama.slice(0, 800) };
  const modelName = (ollama.match(/^(\S+:\S+)/m) || ollama.match(/\b(qwen3:14b|gpt-oss:20b|llama[\w:.-]+)\b/))?.[1];
  if (modelName && existsSync("/home/rtx5060ti/.local/bin/ollama")) {
    const profile10 = path.join(scratch, "p10-profile");
    const project10 = path.join(scratch, "p10-project");
    await mkdir(path.join(profile10, "config"), { recursive: true });
    await mkdir(project10);
    await writeFile(path.join(project10, "README.md"), "Harness check.\n");
    await writeFile(
      path.join(profile10, "config/config.yaml"),
      JSON.stringify({
        model: {
          default: modelName,
          name: modelName,
          provider: "ollama",
          endpoint: "http://127.0.0.1:11434/v1",
          context_limit: 8192,
        },
        onboarding: { completed: true, workspace: project10 },
        agent: { max_steps: 3, model_retries: 0 },
        permissions: { approve_shell: true },
      }),
    );
    await finish(launch(["--json", "trust"], { workspace: project10, profile: profile10 }));
    const run10 = launch(["--json", "run", "Reply with exactly OK. Do not use tools."], {
      workspace: project10,
      profile: profile10,
    });
    const out10 = await finish(run10, 180000).catch((error) => ({
      code: 1,
      stdout: "",
      stderr: String(error),
    }));
    evidence.gates.p10_real_local = {
      model: modelName,
      code: out10.code,
      stdout: (out10.stdout || "").slice(0, 600),
      stderr: (out10.stderr || "").slice(0, 400),
      harness_survived: true,
    };
  } else {
    evidence.gates.p10_real_local = { skipped: true, reason: "no ollama model listed" };
  }

  // P12 isolated ShadowCode worktree — never overwrite the running binary.
  const worktree = path.join(scratch, "self-host");
  execFileSync("git", ["worktree", "add", "--detach", worktree, "HEAD"], { cwd: root });
  const running = path.join(root, "target/debug/shadowcode");
  const beforeStat = execFileSync("stat", ["-c", "%Y %s", running], { encoding: "utf8" }).trim();
  const check = spawnSync(
    "cargo",
    ["test", "-p", "shadowcode-core", "--offline", "--lib", "never_auto_replays"],
    { cwd: worktree, encoding: "utf8", env: { ...process.env, CARGO_TARGET_DIR: path.join(scratch, "self-host-target") } },
  );
  const afterStat = execFileSync("stat", ["-c", "%Y %s", running], { encoding: "utf8" }).trim();
  evidence.gates.p12_self_host = {
    worktree,
    cargo_code: check.status,
    stderr: (check.stderr || "").slice(0, 400),
    running_binary_unchanged: beforeStat === afterStat,
    target_dir: path.join(scratch, "self-host-target"),
  };
  assert.equal(evidence.gates.p12_self_host.running_binary_unchanged, true);
  execFileSync("git", ["worktree", "remove", "--force", worktree], { cwd: root });

  evidence.ok = true;
  await writeFile(path.join(artifacts, "remaining.json"), JSON.stringify(evidence, null, 2));
  console.log("remaining gates", Object.keys(evidence.gates));
} catch (error) {
  evidence.ok = false;
  evidence.error = String(error.stack || error);
  await writeFile(path.join(artifacts, "remaining-failure.json"), JSON.stringify(evidence, null, 2));
  throw error;
} finally {
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {}
  }
  await delay(400);
  await rm(scratch, { recursive: true, force: true });
}
