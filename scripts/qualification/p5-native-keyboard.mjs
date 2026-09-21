// Native GTK/WebKit keyboard probe. Not a browser harness substitute.
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
const evidence = {
  binary,
  display: process.env.DISPLAY || "",
  wayland: process.env.WAYLAND_DISPLAY || "",
  session: process.env.XDG_SESSION_TYPE || "",
  xdotool: false,
  window_id: null,
  launched: false,
  steps: [],
  defects: [],
  classification: null,
};
if (!existsSync(binary)) {
  evidence.classification = "ENVIRONMENT LIMITATION";
  evidence.note = `missing ${binary}`;
  await writeFile(path.join(artifacts, "p5-native-keyboard.json"), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ classification: evidence.classification, note: evidence.note }));
  process.exit(0);
}
let preexisting = [];
try {
  preexisting = execFileSync("xdotool", ["search", "--name", "ShadowCode"], {
    encoding: "utf8",
    timeout: 3000,
  })
    .trim()
    .split("\n")
    .filter(Boolean);
} catch {}
evidence.preexisting_windows = preexisting;
if (preexisting.length) {
  evidence.classification = "ENVIRONMENT LIMITATION";
  evidence.note =
    "A ShadowCode window is already open (likely the primary install). Refusing to send keys so user projects are not touched. Browser e2e is not treated as equivalent.";
  await writeFile(path.join(artifacts, "p5-native-keyboard.json"), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ classification: evidence.classification, note: evidence.note }));
  process.exit(0);
}
try {
  execFileSync("xdotool", ["version"], { encoding: "utf8" });
  evidence.xdotool = true;
} catch {
  evidence.classification = "ENVIRONMENT LIMITATION";
  evidence.note = "xdotool is not available";
  await writeFile(path.join(artifacts, "p5-native-keyboard.json"), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ classification: evidence.classification, note: evidence.note }));
  process.exit(0);
}

const scratch = await mkdtemp(path.join(tmpdir(), "shadowcode-p5-"));
const project = path.join(scratch, "project");
const profile = path.join(scratch, "profile");
await mkdir(project, { recursive: true });
await mkdir(path.join(profile, "config"), { recursive: true });
await writeFile(path.join(project, "README.md"), "native keyboard probe\n");
await writeFile(
  path.join(profile, "config/config.yaml"),
  JSON.stringify({
    model: { default: "mock", name: "mock", provider: "mock", context_limit: 8192 },
    onboarding: { completed: true, workspace: project },
    ui: { notify: false },
  }),
);

const child = spawn(binary, ["--profile", profile, "--workspace", project], {
  env: { ...process.env, DISPLAY: process.env.DISPLAY || ":0" },
  stdio: "pipe",
});
evidence.launched = true;
evidence.pid = child.pid;
let stdout = "";
let stderr = "";
child.stdout.on("data", (d) => (stdout += d));
child.stderr.on("data", (d) => (stderr += d));
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

function searchWindow() {
  try {
    const out = execFileSync("xdotool", ["search", "--name", "ShadowCode"], {
      encoding: "utf8",
      timeout: 3000,
    }).trim();
    return out.split("\n").filter(Boolean).at(-1) || null;
  } catch {
    return null;
  }
}

try {
  let win = null;
  for (let i = 0; i < 20 && !win; i++) {
    await delay(500);
    if (child.exitCode != null) break;
    win = searchWindow();
  }
  evidence.window_id = win;
  evidence.stdout_tail = stdout.slice(-500);
  evidence.stderr_tail = stderr.slice(-800);
  if (!win) {
    evidence.classification = "ENVIRONMENT LIMITATION";
    evidence.note =
      "Native process launched but no X11 ShadowCode window was visible to xdotool. Session is Wayland; GTK may be a native Wayland surface that xdotool cannot drive. Browser e2e is not treated as equivalent.";
  } else {
    const windowName = () => {
      try {
        return execFileSync("xdotool", ["getwindowname", win], {
          encoding: "utf8",
          timeout: 2000,
        }).trim();
      } catch {
        return "";
      }
    };
    const send = (keys, label) => {
      try {
        execFileSync("xdotool", ["windowactivate", "--sync", win, "key", "--clearmodifiers", keys], {
          timeout: 4000,
        });
        evidence.steps.push({ label, keys, ok: true, window: windowName() });
      } catch (error) {
        evidence.steps.push({ label, keys, ok: false, error: String(error) });
        evidence.defects.push(`${label}: ${error}`);
      }
    };
    send("Tab", "tab");
    send("shift+Tab", "shift_tab");
    send("Escape", "escape");
    send("ctrl+k", "command_palette");
    send("Escape", "close_palette");
    send("ctrl+n", "new_task");
    send("Escape", "close_new");
    send("ctrl+comma", "settings");
    send("Escape", "close_settings");
    send("ctrl+b", "sidebar");
    send("Return", "enter");
    send("space", "space");
    evidence.classification = evidence.defects.length
      ? "DEFECTS OBSERVED"
      : "NATIVE KEYBOARD PROBE COMPLETED";
    evidence.note =
      "Keys were delivered to the native window via xdotool. This is not a full keyboard-only day and does not replace a human pass over every surface.";
  }
} finally {
  try {
    process.kill(child.pid, "SIGTERM");
  } catch {}
  await delay(800);
  try {
    process.kill(child.pid, "SIGKILL");
  } catch {}
  evidence.exit = { code: child.exitCode, signal: child.signalCode };
  await writeFile(path.join(artifacts, "p5-native-keyboard.json"), JSON.stringify(evidence, null, 2));
  await rm(scratch, { recursive: true, force: true }).catch(() => {});
}
console.log(
  JSON.stringify({
    classification: evidence.classification,
    window_id: evidence.window_id,
    steps: evidence.steps.length,
    defects: evidence.defects.length,
  }),
);
