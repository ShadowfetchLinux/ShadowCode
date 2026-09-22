// Real AppImage extraction lifetimes, using one shared TMPDIR deliberately.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  readlink,
  stat,
  writeFile,
  rm,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
const expectedVersion = JSON.parse(
  await readFile(path.join(root, "src-tauri/tauri.conf.json"), "utf8"),
).version;
const binary = path.resolve(
  process.argv[2] ||
    path.join(
      root,
      `target/release/bundle/appimage/ShadowCode_${expectedVersion}_amd64.AppImage`,
    ),
);
const artifacts = path.resolve(
  process.env.SHADOW_RUNTIME_ARTIFACTS ||
    path.join(root, "artifacts/native-package/runtime"),
);
await mkdir(artifacts, { recursive: true });
await rm(path.join(artifacts, "result.json"), { force: true });
await rm(path.join(artifacts, "failure.txt"), { force: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-runtime-"));
const temp = path.join(scratch, "images");
await mkdir(temp);
const env = { ...process.env, TMPDIR: temp };
delete env.NO_CLEANUP;
delete env.APPIMAGE_EXTRACT_AND_RUN;
delete env.DISPLAY;
delete env.WAYLAND_DISPLAY;
const children = new Set();
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const hash = (data) => createHash("sha256").update(data).digest("hex");
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
    await delay(40);
  }
  throw new Error(`${label} timed out${last ? `: ${last}` : ""}`);
}
function launch(
  args,
  extraEnv = {},
  environmentMode = false,
  ignoreHangup = false,
  cwd = root,
) {
  const options = environmentMode ? { APPIMAGE_EXTRACT_AND_RUN: "1" } : {};
  const child = spawn(
    ignoreHangup ? "nohup" : binary,
    [
      ...(ignoreHangup ? [binary] : []),
      ...(environmentMode ? [] : ["--appimage-extract-and-run"]),
      ...args,
    ],
    {
      cwd,
      env: { ...env, ...extraEnv, ...options },
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  children.add(child);
  const result = { stdout: "", stderr: "", code: undefined, signal: undefined };
  child.stdout.on("data", (chunk) => {
    result.stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    result.stderr += chunk;
  });
  const done = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => {
      children.delete(child);
      result.code = code;
      result.signal = signal;
      resolve(result);
    });
  });
  return { child, result, done, args };
}
async function finish(run, code = 0) {
  let timer;
  const result = await Promise.race([
    run.done,
    new Promise((_, reject) => {
      timer = setTimeout(
        () =>
          reject(
            new Error(
              `Timeout: ${JSON.stringify(run.args)} ${JSON.stringify(run.result)}`,
            ),
          ),
        20000,
      );
    }),
  ]).finally(() => clearTimeout(timer));
  assert.equal(result.code, code, JSON.stringify(result));
  assert.equal(result.signal, null, JSON.stringify(result));
  return result;
}
async function cli(profile, workspace, args) {
  return JSON.parse(
    (
      await finish(
        launch([
          "--profile",
          profile,
          "--workspace",
          workspace,
          "--json",
          ...args,
        ]),
      )
    ).stdout,
  );
}
async function owner(name, ignoreHangup = false) {
  const profile = path.join(scratch, name, "profile"),
    workspace = path.join(scratch, name, "project");
  await mkdir(workspace, { recursive: true });
  const run = launch(
    ["--profile", profile, "--workspace", workspace, "--json", "serve"],
    {},
    false,
    ignoreHangup,
  );
  await until(`${name} owner`, () => run.result.stderr.includes("serving"));
  const pids = (
    await readFile(
      `/proc/${run.child.pid}/task/${run.child.pid}/children`,
      "utf8",
    )
  )
    .trim()
    .split(/\s+/)
    .filter((pid) => /^\d+$/.test(pid));
  assert.equal(pids.length, 1, "Runtime owns exactly one native child");
  const pid = pids[0];
  const appdir = (await readFile(`/proc/${pid}/environ`, "utf8"))
    .split("\0")
    .find((value) => value.startsWith("APPDIR="))
    ?.slice(7);
  assert.ok(appdir?.startsWith(`${temp}/appimage_extracted_`));
  assert.equal(
    (await stat(appdir)).mode & 0o777,
    0o700,
    "Extraction directory must be private",
  );
  assert.equal((await stat(appdir)).uid, process.getuid());
  const executable = await readlink(`/proc/${pid}/exe`);
  assert.equal(executable, path.join(appdir, "usr/bin/shadowcode"));
  const files = await readdir(appdir, { recursive: true });
  const resources = [
    "usr/bin/shadowcode",
    ...files.filter((file) => /\/WebKit(?:Web|Network)Process$/.test(file)),
  ];
  assert.equal(
    resources.length,
    3,
    "Native window subprocess resources must be bundled",
  );
  const hashes = new Map(
    await Promise.all(
      resources.map(async (file) => [
        file,
        hash(await readFile(path.join(appdir, file))),
      ]),
    ),
  );
  return { run, profile, workspace, pid, appdir, hashes };
}
async function intact(owner) {
  assert.equal(
    await readlink(`/proc/${owner.pid}/exe`),
    path.join(owner.appdir, "usr/bin/shadowcode"),
    "Running executable must not have been deleted",
  );
  for (const [file, digest] of owner.hashes)
    assert.equal(
      hash(await readFile(path.join(owner.appdir, file))),
      digest,
      `Deferred resource retained: ${file}`,
    );
}
async function dead(pid) {
  assert.ok(
    Number.isSafeInteger(Number(pid)) && Number(pid) > 1,
    "Expected an actual owned process PID",
  );
  try {
    return /\) [ZX] /.test(await readFile(`/proc/${pid}/stat`, "utf8"));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    return true;
  }
}
try {
  const launchDirectory = path.join(scratch, "launch directory with spaces");
  await mkdir(path.join(launchDirectory, "nested project"), {
    recursive: true,
  });
  for (const environmentMode of [false, true]) {
    const args = ["--profile", "relative profile", "--json", "health"];
    const health = JSON.parse(
      (
        await finish(
          launch(
            args,
            { PWD: "/deliberately-stale" },
            environmentMode,
            false,
            launchDirectory,
          ),
        )
      ).stdout,
    );
    assert.equal(
      health.workspace,
      launchDirectory,
      "Default project must be the actual caller directory",
    );
    assert.ok(
      (
        await stat(path.join(launchDirectory, "relative profile"))
      ).isDirectory(),
      "Relative profiles resolve from the caller directory",
    );
    const nested = JSON.parse(
      (
        await finish(
          launch(
            [...args, "--workspace", "nested project"],
            {},
            environmentMode,
            false,
            launchDirectory,
          ),
        )
      ).stdout,
    );
    assert.equal(
      nested.workspace,
      path.join(launchDirectory, "nested project"),
      "Explicit relative projects remain caller-relative",
    );
  }
  const first = await owner("first"),
    second = await owner("second");
  assert.notEqual(
    first.appdir,
    second.appdir,
    "Each launch must own a separate extraction directory",
  );
  const sentinel = path.join(temp, "unrelated.txt");
  await writeFile(sentinel, "preserve peer files");
  const replies = await Promise.all(
    Array.from({ length: 8 }, () => finish(launch(["--version"]))),
  );
  for (const reply of replies)
    assert.equal(reply.stdout.trim(), `ShadowCode ${expectedVersion}`);
  assert.equal(
    (await finish(launch(["ui", "--version"], {}, true))).stdout.trim(),
    `ShadowCode ${expectedVersion}`,
    "Environment-based extraction preserves arguments",
  );
  await intact(first);
  await intact(second);
  await cli(first.profile, first.workspace, ["trust"]);
  // Python is supplied by the test host, never bundled by ShadowCode. The
  // launcher must not poison language tools run by the coding agent.
  await cli(first.profile, first.workspace, [
    "background",
    "start",
    "--command",
    'python3 -c \'from pathlib import Path; Path("python-ok.txt").write_text("external-python-ok")\'',
  ]);
  await until(
    "External Python tool",
    async () =>
      (await readFile(path.join(first.workspace, "python-ok.txt"), "utf8")) ===
      "external-python-ok",
  );
  const background = await cli(first.profile, first.workspace, [
    "background",
    "start",
    "--command",
    "trap '' TERM; sleep 60 & echo $! > child.pid; wait",
  ]);
  const child = await until("Managed child", async () =>
    (await readFile(path.join(first.workspace, "child.pid"), "utf8")).trim(),
  );
  const running = await cli(first.profile, first.workspace, [
    "background",
    "logs",
    background.id,
  ]);
  assert.equal(await dead(running.pid), false);
  assert.equal(await dead(child), false);
  await intact(first);
  await intact(second);
  first.run.child.kill("SIGTERM");
  assert.equal(JSON.parse((await finish(first.run)).stdout).status, "stopped");
  await until("Managed parent shutdown", () => dead(running.pid));
  await until("Managed child shutdown", () => dead(child));
  assert.equal(
    await stat(first.appdir).then(
      () => true,
      () => false,
    ),
    false,
    "Exited owner removes its own extraction directory",
  );
  await intact(second);
  second.run.child.kill("SIGINT");
  assert.equal(JSON.parse((await finish(second.run)).stdout).status, "stopped");
  assert.equal(
    await stat(second.appdir).then(
      () => true,
      () => false,
    ),
    false,
  );
  const hungup = await owner("hangup");
  await cli(hungup.profile, hungup.workspace, ["trust"]);
  const hangupBackground = await cli(hungup.profile, hungup.workspace, [
    "background",
    "start",
    "--command",
    "trap '' TERM; sleep 60 & echo $! > child.pid; wait",
  ]);
  const hangupChild = await until("Hangup managed child", async () =>
    (await readFile(path.join(hungup.workspace, "child.pid"), "utf8")).trim(),
  );
  const hangupRunning = await cli(hungup.profile, hungup.workspace, [
    "background",
    "logs",
    hangupBackground.id,
  ]);
  assert.equal(await dead(hangupRunning.pid), false);
  assert.equal(await dead(hangupChild), false);
  hungup.run.child.kill("SIGHUP");
  assert.equal(JSON.parse((await finish(hungup.run)).stdout).status, "stopped");
  await until("Hangup managed parent shutdown", () => dead(hangupRunning.pid));
  await until("Hangup managed child shutdown", () => dead(hangupChild));
  assert.equal(
    await stat(hungup.appdir).then(
      () => true,
      () => false,
    ),
    false,
  );
  const immune = await owner("nohup", true);
  immune.run.child.kill("SIGHUP");
  // Also exercise the native handler directly: neither layer may undo nohup.
  process.kill(Number(immune.pid), "SIGHUP");
  await delay(250);
  assert.equal(
    (await cli(immune.profile, immune.workspace, ["health"])).workspace,
    immune.workspace,
  );
  await intact(immune);
  immune.run.child.kill("SIGTERM");
  assert.equal(JSON.parse((await finish(immune.run)).stdout).status, "stopped");
  assert.equal(
    await stat(immune.appdir).then(
      () => true,
      () => false,
    ),
    false,
  );
  assert.equal(await readFile(sentinel, "utf8"), "preserve peer files");
  assert.deepEqual(
    (await readdir(temp)).filter((name) =>
      name.startsWith("appimage_extracted_"),
    ),
    [],
    "Ordinary exits and forwarded signals clean each private directory",
  );
  const long = path.join(
    temp,
    ...Array.from({ length: 5 }, (_, i) => String(i).repeat(205)),
  );
  await mkdir(long, { recursive: true });
  const overlong = launch(["--version"], { TMPDIR: long });
  const failure = await finish(overlong, 127);
  assert.match(failure.stderr, /Extraction path is too long/);
  assert.deepEqual(
    await readdir(long),
    [],
    "Failed extraction cleans only its newly created directory",
  );
  await writeFile(
    path.join(artifacts, "result.json"),
    JSON.stringify(
      {
        passed: true,
        appimage: binary,
        sha256: hash(await readFile(binary)),
        checks: [
          "caller directory and relative profiles/projects survive both extraction launch modes",
          "external Python tools run without injected runtime paths",
          "simultaneous owners use private unique directories in a shared TMPDIR",
          "eight concurrent short clients preserve both owners' executable and WebKit subprocess resources",
          "environment extraction mode preserves argv",
          "SIGTERM, SIGINT and SIGHUP are forwarded with native cleanup and preserved exit status",
          "background process groups stop before runtime cleanup",
          "nohup remains effective in both wrapper and payload",
          "one owner cannot remove another's files or unrelated peer files",
          "overlong extraction paths fail without overflow or leaked directories",
        ],
      },
      null,
      2,
    ),
  );
  console.log(
    "Native AppImage runtime passed: concurrent extraction, deferred resources, signals, cleanup, and path bounds.",
  );
} catch (error) {
  await writeFile(path.join(artifacts, "failure.txt"), `${error.stack}\n`);
  throw error;
} finally {
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {}
  }
  await delay(3000);
  for (const child of children) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {}
  }
  await rm(scratch, { recursive: true, force: true });
}
