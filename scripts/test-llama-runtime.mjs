// node --test scripts/test-llama-runtime.mjs
// Packaging of the managed llama.cpp runtime: pin/notice validation, relative
// symlinks in copies, the deb repack, and (when packaging/llama.cpp/bin was
// built) the real runtime loading from a scratch AppDir with a clean env.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import {
  chmod,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readlink,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import {
  addRuntimeToDeb,
  copyManagedRuntime,
  inspectRuntimeTree,
  parseFields,
  readManagedRuntime,
  verifyRuntimeDirectory,
} from "./llama-runtime.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const execFileAsync = promisify(execFile);
const run = (binary, args, options = {}) =>
  execFileAsync(binary, args, {
    timeout: 60000,
    maxBuffer: 16000000,
    ...options,
  });
const pin = parseFields(
  await readFile(path.join(root, "tools/llama.cpp.pin"), "utf8"),
);
const sources = JSON.parse(
  await readFile(path.join(root, "licenses/native/sources.json"), "utf8"),
);

// A checkout with a fake runtime shaped like the real one.
async function fakeCheckout() {
  const checkout = await mkdtemp(path.join(tmpdir(), "shadowcode-llama-"));
  const bin = path.join(checkout, "packaging/llama.cpp/bin");
  await mkdir(path.join(bin, "NOTICES"), { recursive: true });
  await mkdir(path.join(checkout, "tools"), { recursive: true });
  await cp(
    path.join(root, "licenses/native"),
    path.join(checkout, "licenses/native"),
    {
      recursive: true,
    },
  );
  await cp(
    path.join(root, "tools/llama.cpp.pin"),
    path.join(checkout, "tools/llama.cpp.pin"),
  );
  await writeFile(
    path.join(bin, "llama-server"),
    `#!/bin/sh\necho "version: 0.4.1-dev (build 1, commit ${pin.commit.slice(0, 9)})" >&2\n`,
  );
  await chmod(path.join(bin, "llama-server"), 0o775);
  await writeFile(path.join(bin, "libfake.so.0.1"), "not an ELF");
  await symlink("libfake.so.0.1", path.join(bin, "libfake.so.0"));
  await symlink("libfake.so.0", path.join(bin, "libfake.so"));
  await writeFile(
    path.join(bin, "architectures.txt"),
    `${Array.from({ length: 20 }, (_, i) => `arch${i}`).join("\n")}\n`,
  );
  await writeFile(
    path.join(bin, "COMMIT"),
    `url=${pin.url}\ncommit=${pin.commit}\nspirv_headers_commit=${pin.spirv_headers_commit}\nbackend=vulkan+cpu\nbuilt=2026-09-23T00:00:00Z\n`,
  );
  await writeFile(path.join(bin, "libggml-vulkan.so"), "not an ELF");
  for (const file of sources.files.filter((file) => file.runtime))
    await cp(
      path.join(root, "licenses/native", file.file),
      path.join(bin, file.runtime),
    );
  return { checkout, bin };
}

test("the pinned runtime and its notices are accepted", async () => {
  const { checkout } = await fakeCheckout();
  try {
    const runtime = await readManagedRuntime(checkout);
    assert.equal(runtime.commit, pin.commit);
    assert.deepEqual(
      runtime.components.map((c) => c.name),
      ["llama.cpp", "SPIRV-Headers"],
    );
    assert.ok(
      runtime.components[0].notices.some(
        (n) => n.runtime === "NOTICES/llama.cpp-LICENSE",
      ),
    );
  } finally {
    await rm(checkout, { recursive: true, force: true });
  }
});

test("a runtime from another commit, with an absolute link, or changed notices is refused", async () => {
  const cases = [
    async ({ bin }) =>
      writeFile(
        path.join(bin, "COMMIT"),
        `commit=${"0".repeat(40)}\nbackend=cpu\n`,
      ),
    async ({ bin }) => {
      await rm(path.join(bin, "libfake.so.0"));
      await symlink(
        path.join(bin, "libfake.so.0.1"),
        path.join(bin, "libfake.so.0"),
      );
    },
    async ({ bin }) =>
      writeFile(path.join(bin, "NOTICES/llama.cpp-LICENSE"), "changed"),
    async ({ bin }) => rm(path.join(bin, "NOTICES/SPIRV-Headers-LICENSE")),
    async ({ bin }) => rm(path.join(bin, "architectures.txt")),
    async ({ bin }) => rm(path.join(bin, "llama-server")),
  ];
  for (const breakIt of cases) {
    const fake = await fakeCheckout();
    try {
      await breakIt(fake);
      await assert.rejects(readManagedRuntime(fake.checkout));
    } finally {
      await rm(fake.checkout, { recursive: true, force: true });
    }
  }
});

test("copies keep relative SONAME links and normalise permissions", async () => {
  const { checkout } = await fakeCheckout();
  try {
    const runtime = await readManagedRuntime(checkout);
    const appdir = path.join(checkout, "AppDir/usr/lib/shadowcode");
    await copyManagedRuntime(runtime, appdir);
    assert.equal(
      await readlink(path.join(appdir, "libfake.so.0")),
      "libfake.so.0.1",
    );
    assert.equal(
      await readlink(path.join(appdir, "libfake.so")),
      "libfake.so.0",
    );
    const { problems, entries } = await inspectRuntimeTree(appdir);
    assert.deepEqual(problems, []);
    const modes = Object.fromEntries(
      entries.filter((e) => e.type === "file").map((e) => [e.file, e.mode]),
    );
    assert.equal(modes["llama-server"], 0o755);
    assert.equal(modes.COMMIT, 0o644);
    // Masking the source must not matter for a copy.
    await rm(runtime.directory, { recursive: true });
    assert.deepEqual((await inspectRuntimeTree(appdir)).problems, []);
  } finally {
    await rm(checkout, { recursive: true, force: true });
  }
});

test("the fake runtime passes directory verification with a clean environment", async () => {
  const { checkout, bin } = await fakeCheckout();
  try {
    const report = await verifyRuntimeDirectory(bin, pin, { run });
    assert.equal(report.commit, pin.commit);
    assert.equal(report.symlinks, 2);
  } finally {
    await rm(checkout, { recursive: true, force: true });
  }
});

test("the deb repack adds the runtime with relative links, md5sums and size", async (t) => {
  if (!existsSync("/usr/bin/dpkg-deb"))
    return t.skip("dpkg-deb is not installed");
  const { checkout } = await fakeCheckout();
  try {
    const runtime = await readManagedRuntime(checkout);
    const tree = path.join(checkout, "deb-tree");
    await mkdir(path.join(tree, "DEBIAN"), { recursive: true });
    await mkdir(path.join(tree, "usr/bin"), { recursive: true });
    await writeFile(path.join(tree, "usr/bin/shadowcode"), "#!/bin/sh\n");
    await chmod(path.join(tree, "usr/bin/shadowcode"), 0o755);
    await writeFile(
      path.join(tree, "DEBIAN/control"),
      "Package: shadow-code\nVersion: 0.28.0\nArchitecture: amd64\nInstalled-Size: 1\nMaintainer: Test <test@example.invalid>\nDescription: test\n",
    );
    const deb = path.join(checkout, "ShadowCode_0.28.0_amd64.deb");
    await run("dpkg-deb", ["--root-owner-group", "--build", tree, deb]);
    const scratch = path.join(checkout, "scratch");
    await mkdir(scratch);
    await addRuntimeToDeb(deb, runtime, scratch, { run });
    const listing = (await run("dpkg-deb", ["--contents", deb])).stdout;
    assert.match(listing, /\.\/usr\/lib\/shadowcode\/llama-server\n/);
    assert.match(
      listing,
      /\.\/usr\/lib\/shadowcode\/libfake\.so\.0 -> libfake\.so\.0\.1\n/,
    );
    assert.match(
      listing,
      /\.\/usr\/lib\/shadowcode\/NOTICES\/llama\.cpp-LICENSE\n/,
    );
    assert.doesNotMatch(listing, /-> \//, "no absolute symlinks");
    assert.match(listing, /root\/root/);
    const size = Number(
      (await run("dpkg-deb", ["--field", deb, "Installed-Size"])).stdout.trim(),
    );
    assert.ok(size > 10, `Installed-Size ${size}`);
    const extracted = path.join(checkout, "extracted");
    await run("dpkg-deb", ["--raw-extract", deb, extracted]);
    const check = await run(
      "md5sum",
      ["--check", "--strict", "DEBIAN/md5sums"],
      {
        cwd: extracted,
      },
    );
    assert.match(check.stdout, /usr\/lib\/shadowcode\/llama-server: OK/);
    assert.match(check.stdout, /usr\/bin\/shadowcode: OK/);
  } finally {
    await rm(checkout, { recursive: true, force: true });
  }
});

test(
  "the built runtime loads from a scratch AppDir with LD_LIBRARY_PATH unset",
  {
    skip:
      !existsSync(path.join(root, "packaging/llama.cpp/bin/llama-server")) &&
      "packaging/llama.cpp/bin is not built",
  },
  async () => {
    const runtime = await readManagedRuntime(root);
    const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-appdir-"));
    try {
      const target = path.join(scratch, "squashfs-root/usr/lib/shadowcode");
      await copyManagedRuntime(runtime, target);
      const report = await verifyRuntimeDirectory(target, pin, { run });
      assert.equal(report.commit, pin.commit);
      assert.ok(report.symlinks >= 5, "SONAME links were kept");
      assert.ok(
        report.needed["llama-server"].includes("libllama-server-impl.so"),
      );
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  },
);
