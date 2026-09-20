// Manual probe against an installed Ollama model; not an offline CI fixture.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const exec = promisify(execFile),
  root = fileURLToPath(new URL("../", import.meta.url));
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-hook-local-"));
const project = path.join(scratch, "project"),
  profile = path.join(scratch, "profile");
const artifact = path.join(root, "artifacts/native-hooks-local");
await mkdir(artifact, { recursive: true });
await mkdir(path.join(project, ".shadowcode/hooks"), { recursive: true });
await mkdir(path.join(profile, "config"), { recursive: true });
const binary = path.join(root, "target/debug/shadowcode");
const cli = async (args) =>
  JSON.parse(
    (
      await exec(
        binary,
        ["--profile", profile, "--workspace", project, "--json", ...args],
        { timeout: 240000, maxBuffer: 2000000 },
      )
    ).stdout,
  );
const model = process.env.SHADOW_HOOK_PROBE_MODEL || "gpt-oss:20b";
const name = model.replace(/[^a-zA-Z0-9_-]/g, "-");
try {
  await rm(path.join(artifact, `${name}-result.json`), { force: true });
  await rm(path.join(artifact, `${name}-failure.txt`), { force: true });
  await writeFile(
    path.join(profile, "config/config.yaml"),
    JSON.stringify({
      model: {
        provider: "ollama",
        endpoint: "http://127.0.0.1:11434/v1",
        name: model,
        default: model,
        context_limit: 16384,
      },
      trusted_workspaces: [project],
      permissions: { approve_shell: true },
      agent: { max_steps: 20, max_task_tokens: 100000, max_fix_retries: 2 },
    }),
  );
  const definition = JSON.stringify({
    name: "verify-files",
    events: ["on_complete"],
    timeout_sec: 5,
    command:
      'test "$(cat output.txt 2>/dev/null)" = native-hook-ready && test "$(cat verified.txt 2>/dev/null)" = native-hook-ready || { printf "Completion check: output.txt and verified.txt must each contain native-hook-ready. Inspect and fix the missing or incorrect file.\n" >&2; exit 2; }',
  });
  await writeFile(
    path.join(project, ".shadowcode/hooks/verify.json"),
    definition,
  );
  const hook = (await cli(["hooks"])).hooks[0];
  await cli(["hooks", "--enable", hook.path, "--hash", hook.hash]);
  const run = await exec(
    binary,
    [
      "--profile",
      profile,
      "--workspace",
      project,
      "run",
      "Create output.txt containing the line native-hook-ready. Use file tools. If a configured completion check reports a problem, inspect the files and repair it without changing hook definitions.",
      "--events",
    ],
    { timeout: 240000, maxBuffer: 2000000 },
  );
  const rows = run.stdout
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
  await writeFile(
    path.join(artifact, `${name}-events.json`),
    JSON.stringify(rows, null, 2),
  );
  const result = rows.at(-1).result;
  assert.equal(result.status, "completed");
  for (const file of ["output.txt", "verified.txt"])
    assert.equal(
      (await readFile(path.join(project, file), "utf8")).trim(),
      "native-hook-ready",
    );
  assert.equal(
    await readFile(path.join(project, ".shadowcode/hooks/verify.json"), "utf8"),
    definition,
  );
  const checks = rows
    .filter(
      (row) => row.type === "event" && row.event.type === "hook.completed",
    )
    .map((row) => row.event.payload);
  assert.ok(checks.some((check) => check.success));
  const report = {
    passed: true,
    model,
    status: result.status,
    steps: result.steps,
    usage: result.usage,
    checks: checks.map((check) => ({
      status: check.status,
      exit: check.process?.exit_code,
    })),
    repairObserved: checks.some((check) => !check.success),
    summary: result.summary,
  };
  await writeFile(
    path.join(artifact, `${name}-result.json`),
    JSON.stringify(report, null, 2),
  );
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  await writeFile(
    path.join(artifact, `${name}-failure.txt`),
    `${error.stack}\n${error.stdout || ""}\n${error.stderr || ""}`,
  );
  throw error;
} finally {
  await rm(scratch, { recursive: true, force: true });
}
