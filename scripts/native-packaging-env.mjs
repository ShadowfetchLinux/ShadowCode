// Deterministic PATH for AppImage/Debian packaging.
// linuxdeploy walks every PATH directory and calls boost::filesystem::status
// on each entry. A caller PATH that includes /usr/local/bin/node →
// /root/.hermes/... dies with Permission denied. Packaging must construct
// PATH itself; it must not depend on the human sanitizing the shell.
import { lstatSync, readlinkSync, realpathSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const SYSTEM_PACKAGING_DIRS = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
const BLOCKED_DIRS = new Set(["/usr/local/bin", "/usr/local/sbin", "/snap/bin"]);

function resolveExistingDir(dir) {
  try {
    const resolved = path.resolve(dir);
    const st = lstatSync(resolved);
    if (st.isDirectory()) return resolved;
    // Debian /bin → /usr/bin: keep the PATH name linuxdeploy will scan.
    if (st.isSymbolicLink() && statSync(resolved).isDirectory()) return resolved;
    return null;
  } catch {
    return null;
  }
}

function entryResolvesToHijack(file) {
  try {
    const st = lstatSync(file);
    if (st.isSymbolicLink()) {
      const dest = readlinkSync(file);
      if (dest.includes(".hermes") || dest.startsWith("/root/")) return true;
      try {
        const real = realpathSync(file);
        return (
          real.includes(`${path.sep}.hermes${path.sep}`) || real.startsWith("/root/")
        );
      } catch {
        // Broken node/npm links are the linuxdeploy Permission-denied case.
        return true;
      }
    }
    return false;
  } catch {
    return false;
  }
}

export function isUnsafePackagingDir(dir) {
  const resolved = resolveExistingDir(dir);
  if (!resolved) return true;
  if (BLOCKED_DIRS.has(resolved)) return true;
  if (resolved.split(path.sep).includes(".hermes")) return true;
  if (resolved === "/root" || resolved.startsWith(`/root${path.sep}`)) return true;
  return ["node", "npm", "npx"].some((name) =>
    entryResolvesToHijack(path.join(resolved, name)),
  );
}

function dirContains(dir, name) {
  try {
    statSync(path.join(dir, name));
    return true;
  } catch {
    return false;
  }
}

// rustc/cargo live on the caller PATH (rustup ~/.cargo/bin, CARGO_HOME).
// linuxdeploy still must not see /usr/local/bin or Hermes; keep only the
// toolchain directories themselves when they pass the same safety checks.
function rustToolchainDirs() {
  const candidates = [];
  if (process.env.CARGO_HOME) {
    candidates.push(path.join(process.env.CARGO_HOME, "bin"));
  }
  if (process.env.HOME) {
    candidates.push(path.join(process.env.HOME, ".cargo", "bin"));
  }
  for (const dir of (process.env.PATH || "").split(path.delimiter)) {
    if (!dir) continue;
    const resolved = resolveExistingDir(dir);
    if (
      resolved &&
      (dirContains(resolved, "rustc") || dirContains(resolved, "cargo"))
    ) {
      candidates.push(resolved);
    }
  }
  return candidates;
}

export function packagingDirs(root, options = {}) {
  const execDir = options.execDir ?? path.dirname(process.execPath);
  const extras = [
    path.join(root, "tools/rust-dev/extracted/usr/bin"),
    path.join(root, "..", "tools/rust-dev/extracted/usr/bin"),
    ...rustToolchainDirs(),
    path.join(root, "target/release"),
    path.join(root, "target/debug"),
    path.join(root, "target/.tauri"),
    execDir,
  ];
  const seen = new Set();
  const dirs = [];
  for (const candidate of [...extras, ...SYSTEM_PACKAGING_DIRS]) {
    const resolved = resolveExistingDir(candidate);
    if (!resolved || seen.has(resolved) || isUnsafePackagingDir(resolved))
      continue;
    seen.add(resolved);
    dirs.push(resolved);
  }
  return dirs;
}

export function packagingPath(root, options = {}) {
  return packagingDirs(root, options).join(":");
}

export function applyPackagingPath(root, options = {}) {
  const next = packagingPath(root, options);
  process.env.PATH = next;
  return next;
}

const self = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === self) {
  const root = process.argv[3]
    ? path.resolve(process.argv[3])
    : path.resolve(path.dirname(self), "..");
  if (process.argv[2] === "--print") process.stdout.write(packagingPath(root));
  else if (process.argv[2] === "--apply")
    process.stdout.write(applyPackagingPath(root));
  else {
    console.error("Usage: node scripts/native-packaging-env.mjs --print|--apply [repo-root]");
    process.exit(2);
  }
}
