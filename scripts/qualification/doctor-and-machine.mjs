// P22/P25 local doctor variants and clean-machine inventory. Disposable profile.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-qual-doc-"));
const evidence = { gates: {} };

function run(args, extra = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      env: { ...process.env, ...extra.env },
      stdio: "pipe",
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

try {
  const project = path.join(scratch, "project");
  const profile = path.join(scratch, "profile");
  await mkdir(project);
  await mkdir(path.join(profile, "config"), { recursive: true });
  await writeFile(
    path.join(profile, "config/config.yaml"),
    JSON.stringify({
      model: {
        default: "qual",
        name: "qual",
        provider: "local",
        endpoint: "http://127.0.0.1:1/v1",
        context_limit: 8192,
      },
      onboarding: { completed: true, workspace: project },
    }),
  );
  await run(["--profile", profile, "--workspace", project, "--json", "trust"]);
  const healthyish = await run(["--profile", profile, "--workspace", project, "--json", "doctor"]);
  const providerDown = JSON.parse(healthyish.stdout);
  evidence.gates.p25_provider_down = {
    code: healthyish.code,
    checks: (providerDown.checks || []).map((c) => ({ id: c.id, status: c.status, ok: c.ok })),
    telemetry: JSON.stringify(providerDown).includes("telemetry"),
  };
  const noGit = await run(["--profile", profile, "--workspace", project, "--json", "doctor"], {
    env: { PATH: "/usr/bin" },
  });
  // /usr/bin still has git on this host; hide it with a fake PATH of only /bin.
  const hidden = await run(["--profile", profile, "--workspace", project, "--json", "doctor"], {
    env: { PATH: path.join(scratch, "empty-bin") },
  });
  await mkdir(path.join(scratch, "empty-bin"));
  const hidden2 = await run(["--profile", profile, "--workspace", project, "--json", "doctor"], {
    env: { PATH: path.join(scratch, "empty-bin") },
  });
  evidence.gates.p25_git_missing = {
    code: hidden2.code,
    stdout: hidden2.stdout.slice(0, 800),
  };
  const missingWs = await run([
    "--profile",
    profile,
    "--workspace",
    path.join(scratch, "does-not-exist"),
    "--json",
    "doctor",
  ]);
  evidence.gates.p25_invalid_workspace = {
    code: missingWs.code,
    stdout: missingWs.stdout.slice(0, 400),
    stderr: missingWs.stderr.slice(0, 400),
  };
  evidence.gates.p25_no_git_path_first = { code: hidden.code, stdout: hidden.stdout.slice(0, 200) };
  evidence.gates.p25_path_with_git = { code: noGit.code };

  const ldd = execFileSync("ldd", [binary], { encoding: "utf8" });
  const glibc = execFileSync("ldd", ["--version"], { encoding: "utf8" }).split("\n")[0];
  const git = execFileSync("git", ["--version"], { encoding: "utf8" }).trim();
  const fuse = existsSync("/usr/bin/fusermount") || existsSync("/usr/bin/fusermount3");
  let devPkgs = "";
  try {
    devPkgs = execFileSync(
      "bash",
      [
        "-lc",
        "dpkg-query -W -f='${Package}\\n' 'libwebkit2gtk-4.1-dev' 'libgtk-3-dev' 'librsvg2-dev' 2>/dev/null || true",
      ],
      { encoding: "utf8" },
    );
  } catch {
    devPkgs = "";
  }
  evidence.gates.p22_clean_machine = {
    glibc,
    git,
    fuse_helper: fuse,
    debug_binary_needs_webkit: /libwebkit2gtk/.test(ldd),
    debug_binary_needs_gtk: /libgtk-3/.test(ldd),
    host_has_webkit_dev: /libwebkit2gtk-4.1-dev/.test(devPkgs),
    note: "Debug/dev builds link the host WebKit/GTK. Packaged AppImage/deb must be judged from those artifacts, not this debug binary.",
    state_paths: {
      profile_default: "~/.local/share/shadow-agent or --profile",
      config: "<profile>/config/config.yaml",
      db: "<profile>/state/shadow-agent.db",
    },
    ldd_head: ldd.split("\n").slice(0, 16),
  };
  assert.equal(JSON.stringify(providerDown).includes("telemetry"), true);
  await writeFile(path.join(artifacts, "doctor-machine.json"), JSON.stringify(evidence, null, 2));
  console.log("doctor/machine", Object.keys(evidence.gates));
} finally {
  await rm(scratch, { recursive: true, force: true });
}
