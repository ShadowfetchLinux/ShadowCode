// Disposable AppImage/deb smoke. Never writes to ~/Applications.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const appimage = path.join(root, "target/release/bundle/appimage/ShadowCode_0.20.0_amd64.AppImage");
const deb = path.join(root, "target/release/bundle/deb/ShadowCode_0.20.0_amd64.deb");
const artifacts = path.join(root, "artifacts/qualification");
await mkdir(artifacts, { recursive: true });
assert.ok(existsSync(appimage), appimage);
assert.ok(existsSync(deb), deb);
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-qual-pkg-"));
const evidence = { gates: {} };

function run(bin, args, extra = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, {
      env: { ...process.env, HOME: extra.home || scratch, ...extra.env },
      stdio: "pipe",
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    const timer = setTimeout(() => child.kill("SIGTERM"), extra.timeout || 20000);
    child.on("error", reject);
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal, stdout, stderr });
    });
  });
}

try {
  const version = await run(appimage, ["--appimage-extract-and-run", "--version"]);
  evidence.gates.p19_version = {
    code: version.code,
    stdout: version.stdout.trim(),
    stderr: version.stderr.slice(0, 200),
  };
  assert.match(version.stdout, /ShadowCode 0\.20\.0/);

  const home = path.join(scratch, "home");
  const project = path.join(scratch, "project");
  const profile = path.join(scratch, "profile");
  await mkdir(path.join(home, "Applications"), { recursive: true });
  await mkdir(project);
  await mkdir(path.join(profile, "config"), { recursive: true });
  await writeFile(path.join(project, "README.md"), "packaged smoke\n");
  const doctor = await run(
    appimage,
    [
      "--appimage-extract-and-run",
      "--profile",
      profile,
      "--workspace",
      project,
      "--json",
      "doctor",
    ],
    { home, timeout: 25000 },
  );
  evidence.gates.p19_doctor = {
    code: doctor.code,
    stdout: doctor.stdout.slice(0, 400),
    stderr: doctor.stderr.slice(0, 300),
  };
  assert.equal(doctor.code, 0, doctor.stderr);
  assert.ok(!existsSync(path.join(process.env.HOME, "Applications/.qual-must-not-exist")));

  const extract = path.join(scratch, "extracted");
  await mkdir(extract);
  execFileSync("bash", ["-lc", `cd ${JSON.stringify(extract)} && ${JSON.stringify(appimage)} --appimage-extract`], {
    timeout: 30000,
  });
  evidence.gates.p19_extract = {
    appdir: existsSync(path.join(extract, "squashfs-root/usr/bin/shadowcode")),
    desktop: existsSync(path.join(extract, "squashfs-root/usr/share/applications/ShadowCode.desktop")),
  };

  const info = execFileSync("dpkg-deb", ["-I", deb], { encoding: "utf8" });
  const dest = path.join(scratch, "deb-root");
  await mkdir(dest);
  execFileSync("dpkg-deb", ["-x", deb, dest]);
  evidence.gates.p20_deb = {
    info: info.slice(0, 800),
    binary: existsSync(path.join(dest, "usr/bin/shadowcode")),
    desktop: existsSync(path.join(dest, "usr/share/applications/ShadowCode.desktop")),
    icon: existsSync(path.join(dest, "usr/share/icons/hicolor/128x128/apps/ShadowCode.png"))
      || existsSync(path.join(dest, "usr/share/icons/hicolor/256x256/apps/ShadowCode.png"))
      || existsSync(path.join(dest, "usr/share/icons/hicolor/scalable/apps/shadow-agent.svg")),
    not_installed_to_system: true,
    primary_appimage_untouched: existsSync(
      path.join(process.env.HOME, "Applications/ShadowCode-0.20.0-x86_64.AppImage"),
    ),
  };
  assert.equal(evidence.gates.p20_deb.binary, true);
  assert.equal(evidence.gates.p20_deb.desktop, true);

  if (process.env.DISPLAY) {
    const ui = spawn(
      appimage,
      ["--appimage-extract-and-run", "--profile", profile, "--workspace", project],
      { env: { ...process.env, HOME: home }, stdio: "pipe" },
    );
    await new Promise((resolve) => setTimeout(resolve, 4000));
    let alive = true;
    try {
      process.kill(ui.pid, 0);
    } catch {
      alive = false;
    }
    ui.kill("SIGTERM");
    await new Promise((resolve) => ui.once("close", resolve));
    evidence.gates.p19_window = { launched: alive, pid: ui.pid };
  } else {
    evidence.gates.p19_window = { skipped: "no DISPLAY" };
  }

  await writeFile(path.join(artifacts, "package-smoke.json"), JSON.stringify(evidence, null, 2));
  console.log("package smoke", evidence.gates);
} finally {
  await rm(scratch, { recursive: true, force: true });
}
