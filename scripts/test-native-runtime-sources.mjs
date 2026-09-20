// Rebuild the distributed runtime sources with all container networking disabled.
// The matching build image supplies the already-installed, pinned build tools.
import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const root = fileURLToPath(new URL("../", import.meta.url));
const version = JSON.parse(
  await readFile(path.join(root, "src-tauri/tauri.conf.json")),
).version;
const archive = path.resolve(
  process.argv[2] ||
    path.join(
      root,
      `target/release/bundle/appimage/ShadowCode_${version}_appimage-runtime-sources.tar.gz`,
    ),
);
const artifacts = path.resolve(
  process.env.SHADOW_RUNTIME_SOURCE_ARTIFACTS ||
    path.join(root, "artifacts/native-package/source-rebuild"),
);
const engine = process.env.SHADOW_CONTAINER_ENGINE || "docker";
assert.ok(["docker", "podman"].includes(engine));
const exec = promisify(execFile);
const options = { timeout: 120000, maxBuffer: 16000000 };
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
await mkdir(artifacts, { recursive: true });
await rm(path.join(artifacts, "result.json"), { force: true });
const scratch = await mkdtemp(
  path.join(tmpdir(), "shadowcode-source-rebuild-"),
);
let container;
try {
  await exec("tar", ["-xzf", archive, "-C", scratch], options);
  const receipt = JSON.parse(
    await readFile(path.join(scratch, "receipt.json")),
  );
  assert.match(receipt.inputHash, /^[a-f0-9]{64}$/);
  for (const file of receipt.files.filter((file) =>
    file.file.startsWith("sources/"),
  )) {
    const absolute = path.resolve(scratch, file.file);
    assert.ok(absolute.startsWith(`${scratch}${path.sep}`));
    assert.equal(
      sha(await readFile(absolute)),
      file.sha256,
      `Changed source: ${file.file}`,
    );
  }
  const image = `shadowcode-runtime:${receipt.inputHash.slice(0, 24)}`;
  container = (
    await exec(
      engine,
      [
        "create",
        "--network",
        "none",
        "--entrypoint",
        "/bin/bash",
        "--mount",
        `type=bind,src=${path.join(scratch, "sources")},dst=/retained,readonly`,
        image,
        "-c",
        // These paths belong only to this disposable build container. The retained
        // sources are mounted separately and read-only; no host tree is removed.
        "set -euo pipefail; rm -rf /build /out; mkdir /build; cp -a /retained/. /build/; cd /build; bash build.sh",
      ],
      options,
    )
  ).stdout.trim();
  assert.match(container, /^[a-f0-9]{12,64}$/);
  await new Promise((resolve, reject) => {
    const child = spawn(engine, ["start", "-a", container], {
      stdio: "inherit",
    });
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error("Offline runtime rebuild exceeded five minutes"));
    }, 300000);
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      code === 0
        ? resolve()
        : reject(
            new Error(`Offline runtime rebuild failed (${signal || code})`),
          );
    });
  });
  const runtime = path.join(scratch, "runtime");
  await exec(
    engine,
    ["cp", `${container}:/out/runtime-x86_64`, runtime],
    options,
  );
  await exec(
    "objcopy",
    [
      "--dump-section",
      `.text=${path.join(scratch, "runtime.text")}`,
      runtime,
      path.join(scratch, "runtime.elf"),
    ],
    options,
  );
  const textSha256 = sha(await readFile(path.join(scratch, "runtime.text")));
  assert.equal(
    textSha256,
    receipt.textSha256,
    "The shipped sources must reproduce the runtime machine code",
  );
  const info = await exec(runtime, ["--appimage-version"], options);
  assert.ok(
    (info.stdout + info.stderr).includes(
      `ShadowCode runtime patchset: ${receipt.patchset}`,
    ),
  );
  const result = {
    checkedAt: new Date().toISOString(),
    archive: path.basename(archive),
    archiveSha256: sha(await readFile(archive)),
    inputHash: receipt.inputHash,
    network: "none",
    textSha256,
    runtimeSha256: sha(await readFile(runtime)),
    result: "passed",
  };
  await writeFile(
    path.join(artifacts, "result.json"),
    `${JSON.stringify(result, null, 2)}\n`,
  );
  console.log(JSON.stringify(result, null, 2));
} finally {
  if (container && /^[a-f0-9]{12,64}$/.test(container))
    await exec(engine, ["rm", "-f", container], options);
  await rm(scratch, { recursive: true, force: true });
}
