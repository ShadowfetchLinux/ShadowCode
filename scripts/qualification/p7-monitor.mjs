// Longest-practical leak hunt. Samples RSS/FDs/threads/sqlite of a live serve.
import { spawn, execFileSync } from "node:child_process";
import { mkdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const binary = process.env.SHADOW_DESKTOP_BINARY || path.join(root, "target/debug/shadowcode");
const seconds = Number(process.env.QUAL_P7_SECONDS || 1200);
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-p7-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
await mkdir(project, { recursive: true });
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: { default: "mock", name: "mock", provider: "mock", context_limit: 8192 },
    onboarding: { completed: true, workspace: project },
  }),
);
const child = spawn(binary, ["--profile", profile, "--workspace", project, "--json", "serve"], {
  stdio: "pipe",
  detached: true,
});
const samples = [];
const started = Date.now();
const sample = () => {
  try {
    const status = execFileSync("cat", [`/proc/${child.pid}/status`], { encoding: "utf8" });
    const rss = Number((status.match(/VmRSS:\s+(\d+)/) || [])[1] || 0);
    const threads = Number((status.match(/Threads:\s+(\d+)/) || [])[1] || 0);
    const fds = execFileSync("bash", ["-lc", `ls /proc/${child.pid}/fd | wc -l`], {
      encoding: "utf8",
    }).trim();
    const children = execFileSync(
      "bash",
      ["-lc", `ps --ppid ${child.pid} -o pid= | wc -l`],
      { encoding: "utf8" },
    ).trim();
    samples.push({
      elapsed_s: Math.round((Date.now() - started) / 1000),
      rss_kb: rss,
      threads,
      fds: Number(fds),
      children: Number(children),
    });
  } catch (error) {
    samples.push({ elapsed_s: Math.round((Date.now() - started) / 1000), error: String(error) });
  }
};
await new Promise((resolve) => setTimeout(resolve, 1500));
sample();
const interval = setInterval(sample, 15000);
const stop = async () => {
  clearInterval(interval);
  sample();
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch {}
  await new Promise((resolve) => setTimeout(resolve, 800));
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {}
  const first = samples[0] || {};
  const last = samples.at(-1) || {};
  const report = {
    duration_s: Math.round((Date.now() - started) / 1000),
    requested_s: seconds,
    six_hour: false,
    samples,
    rss_delta_kb: (last.rss_kb || 0) - (first.rss_kb || 0),
    fd_delta: (last.fds || 0) - (first.fds || 0),
    leak_claimed: false,
    note: "Cache growth is not a leak without an unbounded monotonic climb after warmup.",
  };
  await writeFile(path.join(artifacts, "p7-monitor.json"), JSON.stringify(report, null, 2));
  await rm(scratch, { recursive: true, force: true });
  console.log(JSON.stringify({ duration_s: report.duration_s, rss_delta_kb: report.rss_delta_kb, samples: samples.length }));
};
setTimeout(stop, seconds * 1000);
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
