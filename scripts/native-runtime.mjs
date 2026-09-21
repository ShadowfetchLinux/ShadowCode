// Build the reviewed AppImage runtime patch from pinned source in a container.
// Only runtime/output documentation is exported; build tools stay in the image.
import { createHash } from "node:crypto";
import { execFile, spawn } from "node:child_process";
import {
  chmod,
  copyFile,
  mkdir,
  readFile,
  readdir,
  rename,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { applyPackagingPath } from "./native-packaging-env.mjs";
const root = fileURLToPath(new URL("../", import.meta.url));
const exec = promisify(execFile);
const sha = (value) => createHash("sha256").update(value).digest("hex");
async function inventory(directory, relative = "") {
  const files = [];
  for (const item of await readdir(path.join(directory, relative), {
    withFileTypes: true,
  })) {
    const file = path.join(relative, item.name);
    if (item.isDirectory()) files.push(...(await inventory(directory, file)));
    else if (item.isFile())
      files.push({
        file,
        sha256: sha(await readFile(path.join(directory, file))),
      });
    else throw new Error(`Unexpected runtime artifact: ${file}`);
  }
  return files.sort((a, b) => a.file.localeCompare(b.file));
}
async function command(binary, args) {
  await new Promise((resolve, reject) => {
    const child = spawn(binary, args, { cwd: root, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) =>
      code === 0
        ? resolve()
        : reject(new Error(`${binary} failed (${signal || code})`)),
    );
  });
}
export async function buildRuntime() {
  applyPackagingPath(root);
  const recipe = path.join(root, "packaging/native-runtime");
  const files = [
    "manifest.json",
    "Dockerfile",
    "build.sh",
    "fetch-source.sh",
    "apk-packages.txt",
    "alpine-sources.json",
    "isolated-extraction.patch",
  ];
  const input = createHash("sha256");
  for (const name of files)
    input
      .update(name)
      .update("\0")
      .update(await readFile(path.join(recipe, name)));
  const inputHash = input.digest("hex");
  const manifest = JSON.parse(
    await readFile(path.join(recipe, "manifest.json"), "utf8"),
  );
  if (
    manifest.schema !== 1 ||
    !/^alpine@sha256:[a-f0-9]{64}$/.test(manifest.container) ||
    !/^[a-f0-9]{64}$/.test(manifest.upstream.sha256)
  )
    throw new Error("Invalid native runtime source lock");
  const directory = path.join(
    root,
    "target/native-runtime",
    inputHash.slice(0, 24),
  );
  const runtime = path.join(directory, "runtime-x86_64");
  const receiptPath = path.join(directory, "receipt.json");
  try {
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    if (
      receipt.inputHash === inputHash &&
      receipt.sha256 === sha(await readFile(runtime)) &&
      receipt.files?.length
    ) {
      for (const file of receipt.files) {
        if (
          path.isAbsolute(file.file) ||
          file.file.split(/[\\/]/).includes("..") ||
          sha(await readFile(path.join(directory, file.file))) !== file.sha256
        )
          throw new Error(`Runtime artifact changed: ${file.file}`);
      }
      console.log(
        `Using verified native runtime ${manifest.patchset} (${receipt.sha256.slice(0, 12)})`,
      );
      return { directory, runtime, receipt };
    }
  } catch (error) {
    if (error.code !== "ENOENT")
      console.warn(`Rebuilding native runtime: ${error.message}`);
  }
  const engine = process.env.SHADOW_CONTAINER_ENGINE || "docker";
  if (!["docker", "podman"].includes(engine))
    throw new Error("SHADOW_CONTAINER_ENGINE must be docker or podman");
  await mkdir(directory, { recursive: true });
  const context = path.join(directory, "context");
  await mkdir(context, { recursive: true });
  for (const name of files)
    await copyFile(path.join(recipe, name), path.join(context, name));
  const archive = path.join(
    root,
    "target/native-runtime",
    `type2-runtime-${manifest.upstream.commit}.tar.gz`,
  );
  let archiveBytes = await readFile(archive).catch((error) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (!archiveBytes || sha(archiveBytes) !== manifest.upstream.sha256) {
    const response = await fetch(manifest.upstream.url, {
      signal: AbortSignal.timeout(120000),
    });
    if (!response.ok)
      throw new Error(`Runtime source download failed (${response.status})`);
    archiveBytes = Buffer.from(await response.arrayBuffer());
    if (sha(archiveBytes) !== manifest.upstream.sha256)
      throw new Error("Runtime source checksum changed");
    await writeFile(`${archive}.pending`, archiveBytes);
    await rename(`${archive}.pending`, archive);
  }
  await copyFile(archive, path.join(context, "upstream.tar.gz"));
  const tag = `shadowcode-runtime:${inputHash.slice(0, 24)}`;
  await command(engine, [
    "build",
    "--platform",
    "linux/amd64",
    "--build-arg",
    `BASE_IMAGE=${manifest.container}`,
    "--tag",
    tag,
    context,
  ]);
  const container = (
    await exec(engine, ["create", "--network", "none", tag])
  ).stdout.trim();
  if (!/^[a-f0-9]{12,64}$/.test(container))
    throw new Error("Container engine returned an invalid build container ID");
  try {
    await command(engine, ["cp", `${container}:/out/.`, directory]);
  } finally {
    await exec(engine, ["rm", container]);
  }
  await chmod(runtime, 0o755);
  const version = await exec(runtime, ["--appimage-version"]);
  if (
    !(version.stdout + version.stderr).includes(
      `ShadowCode runtime patchset: ${manifest.patchset}`,
    )
  )
    throw new Error("Built runtime is missing its patch marker");
  await exec("objcopy", [
    "--dump-section",
    `.text=${path.join(directory, "runtime.text")}`,
    runtime,
    path.join(directory, "runtime-code.elf"),
  ]);
  const artifacts = [
    ...(await inventory(directory, "notices")),
    ...(await inventory(directory, "sources")),
  ];
  for (const file of [
    "runtime.map",
    "build-packages.db",
    "build-packages.txt",
    "compiler.txt",
    "runtime-dynamic.txt",
    "runtime-x86_64.debug",
  ])
    artifacts.push({
      file,
      sha256: sha(await readFile(path.join(directory, file))),
    });
  const receipt = {
    schema: 1,
    inputHash,
    patchset: manifest.patchset,
    upstream: manifest.upstream,
    container: manifest.container,
    sha256: sha(await readFile(runtime)),
    textSha256: sha(await readFile(path.join(directory, "runtime.text"))),
    files: artifacts,
  };
  await writeFile(
    `${receiptPath}.pending`,
    `${JSON.stringify(receipt, null, 2)}\n`,
  );
  await rename(`${receiptPath}.pending`, receiptPath);
  console.log(`Built native runtime ${manifest.patchset} (${receipt.sha256})`);
  return { directory, runtime, receipt };
}
export async function runtimeNotices(appdir, built) {
  const destination = path.join(
    appdir,
    "usr/share/doc/shadowcode/notices/runtime",
  );
  await mkdir(destination, { recursive: true });
  const files = [];
  for (const file of built.receipt.files.filter(
    (file) =>
      file.file.startsWith("notices/") ||
      [
        "runtime.map",
        "build-packages.db",
        "build-packages.txt",
        "compiler.txt",
        "runtime-dynamic.txt",
      ].includes(file.file),
  )) {
    await mkdir(path.dirname(path.join(destination, file.file)), {
      recursive: true,
    });
    await copyFile(
      path.join(built.directory, file.file),
      path.join(destination, file.file),
    );
    files.push(file);
  }
  for (const file of [
    "manifest.json",
    "alpine-sources.json",
    "isolated-extraction.patch",
    "build.sh",
    "fetch-source.sh",
    "Dockerfile",
    "apk-packages.txt",
  ]) {
    const bytes = await readFile(path.join(built.directory, "sources", file));
    await writeFile(path.join(destination, file), bytes);
    files.push({ file, sha256: sha(bytes) });
  }
  await writeFile(
    path.join(destination, "runtime.json"),
    `${JSON.stringify({ ...built.receipt, files }, null, 2)}\n`,
  );
  return destination;
}
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
)
  await buildRuntime();
