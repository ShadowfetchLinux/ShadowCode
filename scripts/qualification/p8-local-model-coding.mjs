// Disposable-repo local-model coding task. Judge the harness, not model IQ.
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, writeFile, rm, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
if (!existsSync(binary)) throw new Error(`missing ${binary}`);

const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-p8-"));
const project = path.join(scratch, "repo");
const profile = path.join(scratch, "profile");
await mkdir(project, { recursive: true });
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(
  path.join(project, "add.mjs"),
  "export function add(a, b) {\n  return a - b;\n}\n",
);
await writeFile(
  path.join(project, "add.test.mjs"),
  `import assert from "node:assert/strict";
import { add } from "./add.mjs";
assert.equal(add(2, 3), 5);
console.log("ok");
`,
);
await writeFile(path.join(project, "README.md"), "Broken add helper. Tests must pass.\n");
const git = (args) =>
  new Promise((resolve, reject) => {
    const child = spawn("git", args, { cwd: project, stdio: "pipe" });
    child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`git ${args}`))));
  });
await git(["init"]);
await git(["add", "."]);
await git(["-c", "user.email=soak@example.invalid", "-c", "user.name=Soak", "commit", "-m", "broken add"]);

await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: {
      default: "gpt-oss:20b",
      name: "gpt-oss:20b",
      provider: "local",
      endpoint: "http://127.0.0.1:11434/v1",
      context_limit: 8192,
    },
    agent: { max_steps: 12, retry_attempts: 0 },
    onboarding: { completed: true, workspace: project },
    ui: { notify: false },
    permissions: { approve_shell: false },
    trusted_workspaces: [project],
  }),
);

function run(args, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, ["--profile", profile, "--workspace", project, "--json", ...args], {
      stdio: "pipe",
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error(`timeout ${args.join(" ")}`));
    }, timeoutMs);
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, stdout, stderr });
    });
  });
}

const evidence = { scratch, binary, started: new Date().toISOString() };
try {
  const trust = await run(["trust"], 15000);
  evidence.trust = { code: trust.code };
  const task =
    "Inspect add.mjs and add.test.mjs. The add function is wrong. Edit add.mjs so add(2, 3) is 5, then run node add.test.mjs. If the test fails, repair and rerun. Stop when the test prints ok. Do not invent test output.";
  const result = await run(["run", task], 420000);
  evidence.run = {
    code: result.code,
    signal: result.signal,
    stdout: result.stdout.slice(0, 8000),
    stderr: result.stderr.slice(0, 2000),
  };
  let parsed = {};
  try {
    parsed = JSON.parse(result.stdout);
  } catch {
    parsed = { parse_error: true };
  }
  evidence.job_status = parsed.status;
  evidence.summary = parsed.summary;
  evidence.verification = parsed.result?.verification || parsed.verification;
  evidence.usage_is_estimated = parsed.usage_is_estimated ?? parsed.result?.usage_is_estimated;
  evidence.tool_names = [];
  const jobs = await run(["jobs"], 15000);
  evidence.jobs_stdout = jobs.stdout.slice(0, 4000);
  const source = await readFile(path.join(project, "add.mjs"), "utf8");
  evidence.add_mjs = source;
  evidence.file_looks_fixed = source.includes("a + b") || source.includes("a+b");
  const test = spawn("node", [path.join(project, "add.test.mjs")], { stdio: "pipe" });
  let testOut = "";
  test.stdout.on("data", (d) => (testOut += d));
  test.stderr.on("data", (d) => (testOut += d));
  evidence.test_exit = await new Promise((resolve) => test.on("close", resolve));
  evidence.test_out = testOut.trim();
  evidence.harness_survived = result.code === 0 || result.code === 1;
  evidence.false_verified =
    evidence.verification?.verified === true &&
    evidence.verification?.claim === "model_claim" &&
    evidence.test_exit !== 0;
  evidence.note =
    "A poor model answer is not a harness failure. Judge transport, tools, verification honesty, and final status.";
} finally {
  await writeFile(path.join(artifacts, "p8-local-model-coding.json"), JSON.stringify(evidence, null, 2));
  await rm(scratch, { recursive: true, force: true }).catch(() => {});
}
console.log(
  JSON.stringify({
    artifact: path.join(artifacts, "p8-local-model-coding.json"),
    status: evidence.job_status,
    test_exit: evidence.test_exit,
    false_verified: evidence.false_verified,
  }),
);
