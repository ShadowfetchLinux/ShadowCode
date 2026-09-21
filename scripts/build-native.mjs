// Package each format from the original executable. The bundler patches its
// bundle type into the binary; reusing an already patched binary loses that tag.
import { execFile, spawn } from "node:child_process";
import { copyFile, mkdtemp, readFile, rename, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { applicationNotices, appdirNotices } from "./native-notices.mjs";
import { buildRuntime, runtimeNotices } from "./native-runtime.mjs";
const root = fileURLToPath(new URL("../", import.meta.url));
const cli = path.join(root, "ui/node_modules/@tauri-apps/cli/tauri.js");
const exec = promisify(execFile);
async function command(binary, args, env = {}) {
  await new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      cwd: root,
      stdio: "inherit",
      env: { ...process.env, ...env },
    });
    child.once("error", reject);
    child.once("exit", (code, signal) =>
      code === 0
        ? resolve()
        : reject(new Error(`${binary} failed (${signal || code})`)),
    );
  });
}
const run = (args) => command(process.execPath, [cli, ...args]);
if (process.platform !== "linux" || process.arch !== "x64")
  throw new Error("This packaging workflow currently supports Linux x86_64");
const notices = path.join(root, "target/native-notices");
const nativeRuntime = await buildRuntime();
await applicationNotices(notices);
const files = { "/usr/share/doc/shadowcode/notices": notices };
const config = JSON.stringify({
  bundle: {
    useLocalToolsDir: true,
    linux: { deb: { files }, appimage: { files } },
  },
});
const executable = path.join(root, "target/release/shadowcode");
const unbundledMarker = Buffer.from("__TAURI_BUNDLE_TYPE_VAR_UNK");
try {
  if (!(await readFile(executable)).includes(unbundledMarker)) {
    // Recover after an interrupted bundle or a direct `tauri build`. Cargo
    // cannot detect the bundler's in-place patch in its cached executable.
    await command("cargo", ["clean", "--release", "-p", "shadowcode-desktop"]);
  }
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
await run(["build", "--no-bundle", "--ci", "--", "--locked"]);
if (!(await readFile(executable)).includes(unbundledMarker)) {
  throw new Error("Expected an unbundled Tauri executable before packaging");
}
// Keep the final atomic rename on the same filesystem as the package output.
const scratch = await mkdtemp(path.join(root, "target/.shadowcode-bundle-"));
const original = path.join(scratch, "shadowcode");
await copyFile(executable, original);
try {
  for (const format of ["appimage", "deb"]) {
    await copyFile(original, executable);
    await run(["bundle", "--bundles", format, "--ci", "--config", config]);
    if (format === "appimage") {
      const version = JSON.parse(
        await readFile(path.join(root, "src-tauri/tauri.conf.json"), "utf8"),
      ).version;
      const appimage = path.join(
        root,
        `target/release/bundle/appimage/ShadowCode_${version}_amd64.AppImage`,
      );
      const appdir = path.join(
        root,
        "target/release/bundle/appimage/ShadowCode.AppDir",
      );
      const runtimeInfo = await exec(appimage, ["--appimage-version"]);
      const runtimeVersion = runtimeInfo.stderr + runtimeInfo.stdout;
      if (!runtimeVersion.includes("/commit/75849dc"))
        throw new Error(
          "AppImage runtime changed; update and verify its dependency notices before packaging",
        );
      await appdirNotices(appdir);
      await runtimeNotices(appdir, nativeRuntime);
      const repacked = path.join(scratch, path.basename(appimage));
      await command(
        path.join(root, "target/.tauri/linuxdeploy-plugin-appimage.AppImage"),
        ["--appimage-extract-and-run", "--appdir", appdir],
        {
          APPIMAGE_EXTRACT_AND_RUN: "1",
          OUTPUT: repacked,
          ARCH: "x86_64",
          LDAI_RUNTIME_FILE: nativeRuntime.runtime,
        },
      );
      await rename(repacked, appimage);
      const sources = path.join(
        root,
        `target/release/bundle/appimage/ShadowCode_${version}_appimage-runtime-sources.tar.gz`,
      );
      const pendingSources = path.join(scratch, path.basename(sources));
      await command("tar", [
        "-czf",
        pendingSources,
        "-C",
        nativeRuntime.directory,
        "sources",
        "receipt.json",
        "runtime.map",
        "build-packages.db",
        "build-packages.txt",
        "compiler.txt",
        "runtime-dynamic.txt",
        "runtime-x86_64.debug",
      ]);
      await rename(pendingSources, sources);
    }
  }
} finally {
  await copyFile(original, executable);
  await rm(scratch, { recursive: true, force: true });
}
