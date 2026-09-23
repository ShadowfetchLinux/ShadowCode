// Builds the production bundle and fails if any test-transport code or the
// fake engine made it in. Run by `npm run test:e2e` before Playwright.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = mkdtempSync(join(tmpdir(), "shadowcode-prod-"));
const env = { ...process.env };
delete env.VITE_SHADOW_TEST_TRANSPORT;
try {
  execFileSync(
    process.execPath,
    [join(ui, "node_modules/vite/bin/vite.js"), "build", "--outDir", out, "--emptyOutDir"],
    { cwd: ui, env, stdio: ["ignore", "ignore", "inherit"] },
  );
  const forbidden = [
    "__SHADOW_TEST_TRANSPORT__",
    "__SHADOW_FAKE__",
    "installFakeBackend",
    "Fake backend has no route",
  ];
  const assets = join(out, "assets");
  const hits = [];
  for (const name of readdirSync(assets).filter((n) => n.endsWith(".js"))) {
    const text = readFileSync(join(assets, name), "utf8");
    for (const word of forbidden) if (text.includes(word)) hits.push(`${name}: ${word}`);
  }
  if (hits.length) {
    console.error(`Production bundle contains test transport code:\n${hits.join("\n")}`);
    process.exit(1);
  }
  console.log("Production bundle check: no test transport or fake engine code.");
} finally {
  rmSync(out, { recursive: true, force: true });
}
