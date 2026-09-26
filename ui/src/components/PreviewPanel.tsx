import {
  ArrowLeft,
  ArrowRight,
  ExternalLink,
  Monitor,
  MousePointerClick,
  RefreshCw,
  RotateCw,
  Smartphone,
  Tablet,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { api } from "../api";
import {
  remembered,
  type DrawerMemory,
  type DrawerMemoryUpdate,
} from "../hooks/useDrawerMemory";
import { pendingAttachments } from "../lib/pendingAttachments";
import {
  commandMessage,
  consoleLabel,
  elementLabel,
  formatConsole,
  formatElement,
  normalizePreviewUrl,
  parsePreviewMessage,
  toTargetUrl,
  type ConsoleEntry,
  type PreviewCommand,
  type PreviewOpened,
  type PreviewServer,
} from "../lib/preview";
import { isRemote, openExternal } from "../lib/transport";
import "../preview.css";

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

export type PreviewDevice = "phone" | "tablet" | "desktop";
const DEVICES: {
  id: PreviewDevice;
  label: string;
  width: number;
  Icon: typeof Monitor;
}[] = [
  { id: "phone", label: "Phone", width: 390, Icon: Smartphone },
  { id: "tablet", label: "Tablet", width: 820, Icon: Tablet },
  { id: "desktop", label: "Desktop", width: 1280, Icon: Monitor },
];
const MAX_CONSOLE = 100;

/** The drawer's Preview tab: the project's dev server in a frame, loaded
 * through the engine's loopback proxy so the picker script can point at
 * elements. Picked elements and console errors become chips on the next
 * message (lib/pendingAttachments). */
type PanelProps = {
  workspace: string;
  toast: Toast;
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
};

export function PreviewPanel(props: PanelProps) {
  // The proxies listen on the engine computer's loopback, which a remote
  // device cannot reach (the engine refuses /api/preview over remote access).
  if (isRemote())
    return (
      <section className="preview-panel" aria-label="Preview">
        <p className="hint">
          The app preview works in the ShadowCode window on the computer running
          your dev server. Open it there to pick elements.
        </p>
      </section>
    );
  return <LocalPreview {...props} />;
}

function LocalPreview({ workspace, toast, memory, onMemory }: PanelProps) {
  const savedUrl = memory.previewUrl;
  const saveUrl = useCallback(
    (url: string) => onMemory("previewUrl", url),
    [onMemory],
  );
  const [device, setDevice] = remembered(memory, onMemory, "previewDevice");
  const [servers, setServers] = useState<PreviewServer[] | null>(null);
  const [address, setAddress] = useState(savedUrl);
  const [current, setCurrent] = useState<PreviewOpened | null>(null);
  const [frameKey, setFrameKey] = useState(0);
  const [pageUrl, setPageUrl] = useState("");
  const [ready, setReady] = useState(false);
  const [picking, setPicking] = useState(false);
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  const [error, setError] = useState("");
  const [opening, setOpening] = useState(false);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const pickingRef = useRef(picking);
  pickingRef.current = picking;

  const loadServers = useCallback(async () => {
    try {
      setServers((await api.previewServers()).servers);
    } catch (e) {
      setServers([]);
      toast(String(e), "err");
    }
  }, [toast]);

  const open = useCallback(
    async (input: string) => {
      const checked = normalizePreviewUrl(input);
      if ("error" in checked) {
        setError(checked.error);
        return;
      }
      setError("");
      setOpening(true);
      try {
        const opened = await api.openPreview(
          checked.url,
          window.location.origin,
        );
        setCurrent(opened);
        setFrameKey((k) => k + 1);
        setReady(false);
        setPicking(false);
        setEntries([]);
        setPageUrl(opened.target_url);
        setAddress(opened.target_url);
        saveUrl(opened.target_url);
      } catch (e) {
        setError(String(e).replace(/^Error: /, ""));
      } finally {
        setOpening(false);
      }
    },
    [saveUrl],
  );

  // Detected servers on open; the remembered page reopens by itself.
  useEffect(() => {
    void loadServers();
    if (savedUrl) void open(savedUrl);
  }, [workspace]); // eslint-disable-line react-hooks/exhaustive-deps

  const send = useCallback(
    (command: PreviewCommand) => {
      const target = frameRef.current?.contentWindow;
      if (target && current)
        target.postMessage(commandMessage(command), current.proxy_origin);
    },
    [current],
  );

  // Messages from the picker: only from this frame's window and origin.
  useEffect(() => {
    if (!current) return;
    const onMessage = (event: MessageEvent) => {
      const message = parsePreviewMessage(event, {
        origin: current.proxy_origin,
        source: frameRef.current?.contentWindow,
      });
      if (!message) return;
      const shown = (url: string) =>
        toTargetUrl(url, current.proxy_origin, current.target_origin);
      switch (message.type) {
        case "ready":
        case "navigated": {
          const url = shown(message.url);
          setPageUrl(url);
          setAddress(url);
          saveUrl(url);
          if (message.type === "ready") {
            setReady(true);
            // A reload keeps picking on.
            if (pickingRef.current) send({ type: "pick", on: true });
          }
          break;
        }
        case "picked": {
          // Only a pick the user asked for: page scripts share the picker's
          // origin and could post look-alike messages at any time.
          if (!pickingRef.current) break;
          const el = message.element;
          const label = elementLabel(el);
          pendingAttachments.add({
            kind: "element",
            label,
            detail: `${el.selector} · ${shown(el.url)}`,
            text: formatElement(el, shown(el.url)),
          });
          setPicking(false);
          toast(`Added ${label} to your next message`, "ok");
          break;
        }
        case "console":
          setEntries((prev) =>
            [...prev, ...message.entries].slice(-MAX_CONSOLE),
          );
          break;
        case "pick-cancelled":
          setPicking(false);
          break;
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [current, send, saveUrl, toast]);

  function togglePick() {
    const on = !picking;
    setPicking(on);
    send({ type: "pick", on });
    if (on) frameRef.current?.focus();
  }

  function reload() {
    setEntries([]);
    if (ready) send({ type: "reload" });
    else setFrameKey((k) => k + 1);
    setReady(false);
  }

  function attachConsole(list: ConsoleEntry[]) {
    if (!list.length) return;
    const label = consoleLabel(list);
    pendingAttachments.add({
      kind: "console",
      label,
      detail: list[0].message.split("\n")[0].slice(0, 200),
      text: formatConsole(list, pageUrl),
    });
    toast(`Added ${label.toLowerCase()} to your next message`, "ok");
  }

  const errors = entries.filter((e) => e.level === "error").length;
  return (
    <section className="preview-panel" aria-label="Preview">
      <form
        className="preview-bar"
        onSubmit={(e) => {
          e.preventDefault();
          void open(address);
        }}
      >
        <button
          type="button"
          className="icon-btn"
          aria-label="Back"
          title="Back"
          disabled={!ready}
          onClick={() => send({ type: "back" })}
        >
          <ArrowLeft size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="icon-btn"
          aria-label="Forward"
          title="Forward"
          disabled={!ready}
          onClick={() => send({ type: "forward" })}
        >
          <ArrowRight size={15} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="icon-btn"
          aria-label="Reload"
          title="Reload"
          disabled={!current}
          onClick={reload}
        >
          <RotateCw size={15} aria-hidden="true" />
        </button>
        <input
          className="search preview-address"
          aria-label="Preview address"
          placeholder="localhost:5173"
          spellCheck={false}
          value={address}
          onChange={(e) => setAddress(e.target.value)}
        />
        <button type="submit" className="mini" disabled={opening}>
          {opening ? "Opening…" : "Go"}
        </button>
        <button
          type="button"
          className="icon-btn"
          aria-label="Open in browser"
          title="Open in your browser"
          disabled={!pageUrl}
          onClick={() =>
            void openExternal(pageUrl).catch((e) => toast(String(e), "err"))
          }
        >
          <ExternalLink size={15} aria-hidden="true" />
        </button>
      </form>
      <div className="preview-tools">
        <div className="seg" role="group" aria-label="Device width">
          {DEVICES.map(({ id, label, Icon }) => (
            <button
              type="button"
              key={id}
              className={device === id ? "on" : ""}
              aria-pressed={device === id}
              title={`${label} width`}
              onClick={() => setDevice(id)}
            >
              <Icon size={13} aria-hidden="true" /> {label}
            </button>
          ))}
        </div>
        <button
          type="button"
          className={`mini preview-pick ${picking ? "on" : ""}`}
          aria-pressed={picking}
          disabled={!ready}
          title={
            ready
              ? "Click an element in the page to add it to your next message (Esc cancels)"
              : "Picking works once the page has loaded"
          }
          onClick={togglePick}
        >
          <MousePointerClick size={13} aria-hidden="true" />{" "}
          {picking ? "Picking… click an element" : "Pick element"}
        </button>
      </div>
      {error && (
        <p className="preview-error" role="alert">
          {error}
        </p>
      )}
      <ServerList
        servers={servers}
        currentPort={
          current ? Number(new URL(current.target_origin).port) || 80 : null
        }
        onOpen={(url) => void open(url)}
        onRefresh={() => void loadServers()}
      />
      {current ? (
        <DeviceFrame
          key={frameKey}
          device={device}
          src={current.url}
          title={`Preview of ${pageUrl}`}
          frameRef={frameRef}
          // Ask the picker to (re)announce itself in case its first "ready"
          // arrived before this window listened.
          onLoad={() => send({ type: "hello" })}
        />
      ) : (
        <div className="preview-empty">
          <p>
            Open your project’s dev server to see it here, then use{" "}
            <strong>Pick element</strong> to point the agent at part of the
            page.
          </p>
          <p className="muted">
            Start it from Tools › Processes or a terminal (for example{" "}
            <code>npm run dev</code>); servers started inside this project show
            up above.
          </p>
        </div>
      )}
      {current && (
        <div
          className="preview-console"
          role="region"
          aria-label="Console errors"
        >
          <div className="preview-console-head">
            <strong>Console</strong>
            <span className="muted">
              {entries.length === 0
                ? "No errors"
                : `${errors} error${errors === 1 ? "" : "s"}${entries.length > errors ? `, ${entries.length - errors} warning${entries.length - errors === 1 ? "" : "s"}` : ""}`}
            </span>
            <button
              type="button"
              className="mini"
              disabled={!entries.length}
              onClick={() => attachConsole(entries)}
            >
              Attach all
            </button>
            <button
              type="button"
              className="mini"
              disabled={!entries.length}
              onClick={() => setEntries([])}
            >
              Clear
            </button>
          </div>
          {entries.length > 0 && (
            <ul className="preview-console-list">
              {entries.map((entry, i) => (
                <li key={`${entry.time}-${i}`} className={entry.level}>
                  <span className="preview-level">{entry.level}</span>
                  <span className="preview-message" title={entry.message}>
                    {entry.message.split("\n")[0]}
                  </span>
                  <button
                    type="button"
                    className="mini"
                    aria-label={`Attach: ${entry.message.split("\n")[0].slice(0, 80)}`}
                    onClick={() => attachConsole([entry])}
                  >
                    Attach
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

function ServerList({
  servers,
  currentPort,
  onOpen,
  onRefresh,
}: {
  servers: PreviewServer[] | null;
  currentPort: number | null;
  onOpen: (url: string) => void;
  onRefresh: () => void;
}) {
  return (
    <div className="preview-servers" role="group" aria-label="Dev servers">
      <span className="muted">Servers:</span>
      {servers === null && <span className="muted">Looking…</span>}
      {servers?.length === 0 && (
        <span className="muted">None found in this project</span>
      )}
      {servers?.map((server) => {
        const who =
          server.background_name || server.process || `port ${server.port}`;
        const host = server.url.replace(/^http:\/\//, "").replace(/\/$/, "");
        return (
          <button
            type="button"
            key={server.port}
            className={`preview-server ${server.port === currentPort ? "on" : ""}`}
            title={[
              server.url,
              server.command,
              server.listening ? "" : "Not answering yet",
            ]
              .filter(Boolean)
              .join("\n")}
            onClick={() => onOpen(server.url)}
          >
            <span
              className={`dot ${server.listening ? "ok" : "idle"}`}
              aria-hidden="true"
            />
            {host}
            <span className="muted"> · {who}</span>
          </button>
        );
      })}
      <button
        type="button"
        className="icon-btn"
        aria-label="Find dev servers again"
        title="Find dev servers again"
        onClick={onRefresh}
      >
        <RefreshCw size={13} aria-hidden="true" />
      </button>
    </div>
  );
}

/** The page at a device width, scaled down to fit the drawer. */
function DeviceFrame({
  device,
  src,
  title,
  frameRef,
  onLoad,
}: {
  device: PreviewDevice;
  src: string;
  title: string;
  frameRef: RefObject<HTMLIFrameElement | null>;
  onLoad: () => void;
}) {
  const boxRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const box = boxRef.current;
    if (!box) return;
    const measure = () =>
      setSize({ width: box.clientWidth, height: box.clientHeight });
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, []);
  const width = DEVICES.find((d) => d.id === device)?.width ?? 1280;
  const scale = size.width ? Math.min(1, size.width / width) : 1;
  const height = size.height ? size.height / scale : 600;
  return (
    <div className="preview-stage" ref={boxRef}>
      <div className="preview-device" style={{ width: width * scale }}>
        <iframe
          ref={frameRef}
          className="preview-frame"
          title={title}
          src={src}
          onLoad={onLoad}
          // The page runs its own scripts at its own (proxy) origin; without
          // allow-top-navigation it cannot navigate ShadowCode's window.
          sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals"
          style={{
            width,
            height,
            transform: scale < 1 ? `scale(${scale})` : undefined,
          }}
        />
      </div>
    </div>
  );
}
