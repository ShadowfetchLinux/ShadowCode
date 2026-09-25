/** The in-app preview: which addresses it opens, the messages its picker
 * script sends (validated here before anything reads them), and how picked
 * elements and console errors are written into a prompt.
 *
 * The picker runs inside the previewed page (it is served by the engine's
 * loopback proxy, see native/core/src/preview.rs), so everything it sends is
 * data from that page: shapes are checked, strings are bounded, and the
 * message must come from the preview frame's own window and origin. */

/** GET /api/preview/servers */
export type PreviewServer = {
  port: number;
  /** Where to open it, e.g. http://localhost:5173/ */
  url: string;
  /** "background": printed by a background process; "process": a listening
   * port owned by a process running inside the project. */
  source: "background" | "process" | string;
  listening: boolean;
  pid: number | null;
  process: string | null;
  command: string | null;
  background_id: string | null;
  background_name: string | null;
};

/** POST /api/preview/open */
export type PreviewOpened = {
  /** http://127.0.0.1:<port>: the preview frame's origin. */
  proxy_origin: string;
  proxy_port: number;
  /** The dev server's own origin, e.g. http://localhost:5173 */
  target_origin: string;
  /** The page to load in the frame (through the proxy). */
  url: string;
  /** The same page at the dev server's own address. */
  target_url: string;
};

export type PickedElement = {
  selector: string;
  tag: string;
  role: string;
  name: string;
  text: string;
  attributes: Record<string, string>;
  outer_html: string;
  styles: Record<string, string>;
  box: { x: number; y: number; width: number; height: number };
  ancestors: string[];
  /** The page address as the frame sees it (through the proxy). */
  url: string;
  title: string;
  viewport: { width: number; height: number; dpr: number };
};

export type ConsoleEntry = {
  level: "error" | "warn";
  message: string;
  /** file:line:column, when the browser reported one. */
  source: string;
  url: string;
  time: number;
};

export type PreviewMessage =
  | { type: "ready" | "navigated"; url: string; title: string }
  | { type: "picked"; element: PickedElement }
  | { type: "console"; entries: ConsoleEntry[] }
  | { type: "pick-cancelled" };

/** Messages the app sends to the picker. */
export type PreviewCommand =
  | { type: "hello" }
  | { type: "pick"; on: boolean }
  | { type: "back" }
  | { type: "forward" }
  | { type: "reload" };

export const commandMessage = (command: PreviewCommand) => ({
  source: "shadowcode-app",
  ...command,
});

// --- addresses ---------------------------------------------------------------

const LOCALHOST_NAME = /^([a-z0-9-]{1,63}\.)+localhost$/;

/** The hosts the preview opens: this computer only. */
export function isLoopbackHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  if (host === "localhost" || LOCALHOST_NAME.test(host)) return true;
  if (host === "[::1]") return true;
  const v4 = host.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  return Boolean(v4 && v4[1] === "127" && v4.slice(2).every((n) => +n < 256));
}

/** What the user typed → a preview URL, or an error to show. A bare port
 * ("5173") or host ("localhost:3000") is read as http. 0.0.0.0 reads as
 * localhost (servers listening everywhere are reachable there). */
export function normalizePreviewUrl(
  input: string,
): { url: string } | { error: string } {
  let text = input.trim();
  if (!text) return { error: "Enter a local address such as localhost:5173." };
  if (/^\d{1,5}$/.test(text)) text = `localhost:${text}`;
  if (/^:\d{1,5}/.test(text)) text = `localhost${text}`;
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) text = `http://${text}`;
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return { error: "That is not an address. Try localhost:5173." };
  }
  if (url.protocol !== "http:")
    return {
      error: "The preview opens plain http:// addresses on this computer.",
    };
  if (url.username || url.password)
    return { error: "Remove the user name or password from the address." };
  if (url.hostname === "0.0.0.0" || url.hostname === "[::]")
    url.hostname = "localhost";
  if (!isLoopbackHost(url.hostname))
    return {
      error:
        "The preview only opens servers on this computer (localhost, *.localhost or 127.0.0.1).",
    };
  return { url: url.href };
}

/** A frame address (through the proxy) shown as the dev server's own. */
export function toTargetUrl(
  frameUrl: string,
  proxyOrigin: string,
  targetOrigin: string,
): string {
  if (frameUrl === proxyOrigin || frameUrl.startsWith(`${proxyOrigin}/`))
    return targetOrigin + frameUrl.slice(proxyOrigin.length);
  return frameUrl;
}

// --- picker messages ---------------------------------------------------------

const str = (value: unknown, limit: number): string | null =>
  typeof value === "string" ? value.slice(0, limit) : null;
const num = (value: unknown): number =>
  typeof value === "number" && Number.isFinite(value) ? value : 0;

function record(value: unknown, keys: number, limit: number) {
  const out: Record<string, string> = {};
  if (!value || typeof value !== "object" || Array.isArray(value)) return out;
  for (const [key, item] of Object.entries(value).slice(0, keys)) {
    const text = str(item, limit);
    if (text !== null && key.length <= 60) out[key] = text;
  }
  return out;
}

function pickedElement(value: unknown): PickedElement | null {
  if (!value || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  const selector = str(v.selector, 1000);
  const tag = str(v.tag, 60);
  const url = str(v.url, 2000);
  if (!selector || !tag || !/^[a-z][a-z0-9-]*$/i.test(tag) || url === null)
    return null;
  const box = (v.box || {}) as Record<string, unknown>;
  const viewport = (v.viewport || {}) as Record<string, unknown>;
  return {
    selector,
    tag: tag.toLowerCase(),
    role: str(v.role, 60) ?? "",
    name: str(v.name, 300) ?? "",
    text: str(v.text, 300) ?? "",
    attributes: record(v.attributes, 16, 300),
    outer_html: str(v.outer_html, 4100) ?? "",
    styles: record(v.styles, 40, 200),
    box: {
      x: num(box.x),
      y: num(box.y),
      width: num(box.width),
      height: num(box.height),
    },
    ancestors: Array.isArray(v.ancestors)
      ? v.ancestors
          .slice(0, 6)
          .map((a) => str(a, 120))
          .filter((a): a is string => Boolean(a))
      : [],
    url,
    title: str(v.title, 300) ?? "",
    viewport: {
      width: num(viewport.width),
      height: num(viewport.height),
      dpr: num(viewport.dpr) || 1,
    },
  };
}

function consoleEntries(value: unknown): ConsoleEntry[] | null {
  if (!Array.isArray(value)) return null;
  return value.slice(0, 100).flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const v = item as Record<string, unknown>;
    const message = str(v.message, 2100);
    if (message === null || (v.level !== "error" && v.level !== "warn"))
      return [];
    return [
      {
        level: v.level,
        message,
        source: str(v.source, 400) ?? "",
        url: str(v.url, 2000) ?? "",
        time: num(v.time),
      },
    ];
  });
}

/** A message from the preview frame, or null when it is not one: it must come
 * from the frame's own window (`source`) at the proxy's origin, and have the
 * picker's shape. */
export function parsePreviewMessage(
  event: { origin: string; source: unknown; data: unknown },
  expected: { origin: string; source: unknown },
): PreviewMessage | null {
  if (!expected.origin || event.origin !== expected.origin) return null;
  if (!expected.source || event.source !== expected.source) return null;
  const data = event.data as Record<string, unknown> | null;
  if (!data || typeof data !== "object") return null;
  if (data.source !== "shadowcode-preview" || data.version !== 1) return null;
  switch (data.type) {
    case "ready":
    case "navigated": {
      const url = str(data.url, 2000);
      if (url === null) return null;
      return { type: data.type, url, title: str(data.title, 300) ?? "" };
    }
    case "picked": {
      const element = pickedElement(data.element);
      return element ? { type: "picked", element } : null;
    }
    case "console": {
      const entries = consoleEntries(data.entries);
      return entries ? { type: "console", entries } : null;
    }
    case "pick-cancelled":
      return { type: "pick-cancelled" };
    default:
      return null;
  }
}

// --- prompt text -------------------------------------------------------------

const clip = (text: string, limit: number) =>
  text.length > limit ? `${text.slice(0, limit)}…` : text;
const oneLine = (text: string) => text.replace(/\s+/g, " ").trim();

/** A code fence longer than any backtick run inside `text`. */
function fenced(text: string, lang = "") {
  const longest = Math.max(
    2,
    ...(text.match(/`+/g) || []).map((run) => run.length),
  );
  const fence = "`".repeat(longest + 1);
  return `${fence}${lang}\n${text}\n${fence}`;
}

const KEY_ATTRIBUTES = [
  "id",
  "class",
  "type",
  "name",
  "role",
  "href",
  "src",
  "aria-label",
  "data-testid",
  "placeholder",
  "disabled",
  "aria-expanded",
  "aria-disabled",
];

/** `<button class="btn primary" type="submit">`, bounded. */
export function startTag(el: PickedElement): string {
  const attrs = KEY_ATTRIBUTES.filter((k) => el.attributes[k] !== undefined)
    .slice(0, 8)
    .map((k) => {
      const value = el.attributes[k];
      return value === ""
        ? k
        : `${k}="${clip(oneLine(value), 80).replace(/"/g, "&quot;")}"`;
    });
  return `<${el.tag}${attrs.length ? ` ${attrs.join(" ")}` : ""}>`;
}

/** The chip label: `button "Save"`. */
export function elementLabel(el: PickedElement): string {
  const words = oneLine(el.name || el.text);
  return words ? `${el.tag} "${clip(words, 28)}"` : `<${el.tag}>`;
}

/** A picked element as prompt context. */
export function formatElement(el: PickedElement, pageUrl = el.url): string {
  const lines: string[] = [];
  const title = el.title ? ` (“${clip(oneLine(el.title), 80)}”)` : "";
  lines.push(`Element on ${pageUrl}${title}:`);
  const words = oneLine(el.text);
  lines.push(`${startTag(el)}${words ? ` "${clip(words, 160)}"` : ""}`);
  lines.push(`- Selector: \`${el.selector.replace(/`/g, "\\`")}\``);
  const role = [
    el.role && `role ${el.role}`,
    el.name && `name "${clip(oneLine(el.name), 120)}"`,
  ].filter(Boolean);
  if (role.length) lines.push(`- Accessibility: ${role.join(", ")}`);
  const { box, viewport } = el;
  lines.push(
    `- Box: ${Math.round(box.width)}×${Math.round(box.height)} at (${Math.round(box.x)}, ${Math.round(box.y)})` +
      (viewport.width
        ? ` in a ${viewport.width}×${viewport.height} viewport`
        : ""),
  );
  if (el.ancestors.length)
    lines.push(`- Inside: ${el.ancestors.map((a) => clip(a, 60)).join(" < ")}`);
  const styles = Object.entries(el.styles)
    .slice(0, 30)
    .map(([k, v]) => `${k}: ${clip(oneLine(v), 80)}`);
  if (styles.length) lines.push(`- Computed styles: ${styles.join("; ")}`);
  if (el.outer_html)
    lines.push(`- HTML:\n${fenced(clip(el.outer_html, 4000), "html")}`);
  return lines.join("\n");
}

export const consoleLabel = (entries: ConsoleEntry[]) =>
  entries.length === 1
    ? `Console ${entries[0].level === "warn" ? "warning" : "error"}`
    : `${entries.length} console messages`;

/** Console errors as prompt context. */
export function formatConsole(entries: ConsoleEntry[], pageUrl: string) {
  const body = entries
    .slice(0, 30)
    .map(
      (e) =>
        `[${e.level}] ${clip(e.message.trim(), 1500)}${e.source ? `\n    at ${e.source}` : ""}`,
    )
    .join("\n");
  return `Console messages from ${pageUrl}:\n${fenced(body, "text")}`;
}
