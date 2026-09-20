// Build-time package verification; Node is not included in either distribution.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdtemp,
  readFile,
  readdir,
  rm,
  stat,
  mkdir,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
const run = promisify(execFile);
const [appimagePath, debPath] = process.argv
  .slice(2)
  .map((p) => path.resolve(p));
assert.ok(
  appimagePath && debPath,
  "Usage: node scripts/check-native-package.mjs APPIMAGE DEB",
);
const artifacts = path.resolve(
  process.env.SHADOW_PACKAGE_ARTIFACTS || "artifacts/native-package",
);
await mkdir(artifacts, { recursive: true });
await rm(path.join(artifacts, "package.json"), { force: true });
const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-package-"));
const options = { timeout: 120000, maxBuffer: 16000000 };
const digest = async (file) =>
  createHash("sha256")
    .update(await readFile(file))
    .digest("hex");
async function inspectTree(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await inspectTree(file)));
    else if (entry.isFile() || entry.isSymbolicLink()) files.push(file);
    // Do not follow bundle symlinks outside the extracted tree.
  }
  return files;
}
async function verifyNotices(directory, includeSystem) {
  const base = path.join(directory, "usr/share/doc/shadowcode/notices");
  const application = JSON.parse(
    await readFile(path.join(base, "application.json"), "utf8"),
  );
  assert.equal(application.schema, 1);
  for (const ecosystem of ["cargo", "npm", "rust-toolchain"]) {
    assert.ok(
      application.packages.some((pkg) => pkg.ecosystem === ecosystem),
      `Missing ${ecosystem} notices`,
    );
  }
  const files = [application.projectLicense];
  for (const pkg of application.packages) {
    assert.ok(
      pkg.name && pkg.version && pkg.notices.length,
      "Incomplete application attribution",
    );
    files.push(...pkg.notices);
  }
  let systemPackages = 0;
  if (includeSystem) {
    const system = JSON.parse(
      await readFile(path.join(base, "system.json"), "utf8"),
    );
    assert.equal(system.schema, 1);
    systemPackages = system.packages.length;
    assert.ok(
      systemPackages > 0 &&
        system.helpers.length > 0 &&
        system.commonLicenses.length > 0,
    );
    for (const pkg of system.packages) {
      assert.ok(
        pkg.name &&
          pkg.version &&
          pkg.source &&
          pkg.sourceVersion &&
          pkg.files.length &&
          pkg.notices.length,
        "Incomplete system attribution",
      );
      files.push(...pkg.notices);
    }
    files.push(...system.helpers, ...system.commonLicenses);
  }
  for (const file of files) {
    const absolute = path.resolve(base, file.file);
    assert.ok(
      absolute.startsWith(`${base}${path.sep}`),
      "Notice path must stay in the package",
    );
    assert.equal(
      await digest(absolute),
      file.sha256,
      `Missing or changed notice: ${file.file}`,
    );
  }
  return {
    applicationPackages: application.packages.length,
    systemPackages,
    noticeFiles: new Set(files.map((file) => file.file)).size,
  };
}
try {
  const version = (
    await run(
      appimagePath,
      ["--appimage-extract-and-run", "--version"],
      options,
    )
  ).stdout.trim();
  assert.match(version, /^ShadowCode \d+\.\d+\.\d+$/);
  await run(appimagePath, ["--appimage-extract"], { ...options, cwd: scratch });
  const appdir = path.join(scratch, "squashfs-root");
  const appimageNotices = await verifyNotices(appdir, true);
  const files = await inspectTree(appdir);
  const python = files.filter((file) =>
    /^(?:python[\d.]*|libpython.*|.*\.py[co]?)$/i.test(path.basename(file)),
  );
  assert.deepEqual(
    python,
    [],
    "The native package must not contain Python interpreters, libraries, or sidecars",
  );
  const executable = path.join(appdir, "usr/bin/shadowcode");
  assert.equal(
    (await readFile(executable)).subarray(0, 4).toString(),
    "\x7fELF",
  );
  assert.equal(
    (await run(executable, ["ui", "--version"], options)).stdout.trim(),
    version,
  );
  const dependencies = (await run("ldd", [executable], options)).stdout;
  assert.equal(
    dependencies.includes("not found"),
    false,
    "Native dependencies must resolve on the build host",
  );
  assert.equal(/libpython/i.test(dependencies), false);
  const deb = path.join(scratch, "deb");
  await run("dpkg-deb", ["--extract", debPath, deb], options);
  const debNotices = await verifyNotices(deb, false);
  assert.deepEqual(
    (await inspectTree(deb)).filter((file) =>
      /^(?:python[\d.]*|libpython.*|.*\.py[co]?)$/i.test(path.basename(file)),
    ),
    [],
    "The Debian package must not contain Python runtimes or sidecars",
  );
  const debExecutable = path.join(deb, "usr/bin/shadowcode");
  assert.equal(
    (await readFile(debExecutable)).subarray(0, 4).toString(),
    "\x7fELF",
  );
  assert.equal(
    (await run(debExecutable, ["--version"], options)).stdout.trim(),
    version,
  );
  // Tauri patches the bundle type and linuxdeploy may change ELF rpaths, so
  // comparing entire executable hashes across formats would reject valid builds.
  await run(
    "objcopy",
    [
      "--dump-section",
      `.text=${path.join(scratch, "appimage.text")}`,
      executable,
    ],
    options,
  );
  await run(
    "objcopy",
    [
      "--dump-section",
      `.text=${path.join(scratch, "deb.text")}`,
      debExecutable,
    ],
    options,
  );
  assert.equal(
    await digest(path.join(scratch, "appimage.text")),
    await digest(path.join(scratch, "deb.text")),
    "Both packages must contain the same compiled application code",
  );
  const debVersion = (
    await run("dpkg-deb", ["--field", debPath, "Version"], options)
  ).stdout.trim();
  assert.equal(debVersion, version.split(" ")[1]);
  const report = {
    passed: true,
    version,
    checkedAt: new Date().toISOString(),
    executableBytes: (await stat(executable)).size,
    bundleFiles: files.length,
    checks: [
      "FUSE-free AppImage version",
      "ELF executable",
      "legacy ui launcher compatibility",
      "no Python runtime or sidecars",
      "host dependency resolution",
      "matching compiled code and versions in AppImage and Debian packages",
      "versioned dependency inventories and SHA-256 verification of every notice",
    ],
    packages: await Promise.all(
      [appimagePath, debPath].map(async (file) => ({
        file: path.basename(file),
        bytes: (await stat(file)).size,
        sha256: await digest(file),
      })),
    ),
    debDependencies: (
      await run("dpkg-deb", ["--field", debPath, "Depends"], options)
    ).stdout.trim(),
    maintainer: (
      await run("dpkg-deb", ["--field", debPath, "Maintainer"], options)
    ).stdout.trim(),
    notices: { appimage: appimageNotices, deb: debNotices },
  };
  await writeFile(
    path.join(artifacts, "package.json"),
    JSON.stringify(report, null, 2),
  );
  await writeFile(
    path.join(artifacts, "SHA256SUMS"),
    report.packages.map((file) => `${file.sha256}  ${file.file}\n`).join(""),
  );
  console.log(JSON.stringify(report, null, 2));
} finally {
  await rm(scratch, { recursive: true, force: true });
}
