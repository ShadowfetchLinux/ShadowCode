/** The drawer's Terminal tab: the user's own shells in this project, one
 * per tab. They keep running when the drawer closes or another tab opens,
 * work while the agent runs, and are never shown to the model. */
import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { terminalApi, type TerminalInfo } from "../lib/terminal";
import { TerminalView } from "./TerminalView";
import {
  remembered,
  type DrawerMemory,
  type DrawerMemoryUpdate,
} from "../hooks/useDrawerMemory";
import "../tools.css";

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

/** Rough initial size; the view fits itself once laid out. */
const START = { cols: 80, rows: 24 };

export function TerminalPanel({
  workspace,
  toast,
  memory,
  onMemory,
}: {
  workspace: string;
  toast: Toast;
  memory: DrawerMemory;
  onMemory: DrawerMemoryUpdate;
}) {
  const [active, setActive] = remembered(memory, onMemory, "terminalActive");
  const [terminals, setTerminals] = useState<TerminalInfo[] | null>(null);
  const [error, setError] = useState("");
  const [opening, setOpening] = useState(false);
  const started = useRef(false);

  const open = useCallback(async () => {
    setOpening(true);
    setError("");
    try {
      const made = await terminalApi.open(START.cols, START.rows);
      setTerminals((list) => [...(list || []), made]);
      onMemory("terminalActive", made.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setOpening(false);
    }
  }, [onMemory]);

  useEffect(() => {
    let current = true;
    void terminalApi
      .list()
      .then(async ({ terminals: list }) => {
        if (!current) return;
        setTerminals(list);
        // The tab opens on a shell: start one when none is running here.
        if (!list.length && !started.current) {
          started.current = true;
          await open();
        }
      })
      .catch((e) => {
        if (current) {
          setTerminals([]);
          setError(String(e));
        }
      });
    return () => {
      current = false;
    };
  }, [workspace, open]);

  const list = terminals || [];
  const shown = list.find((t) => t.id === active) || list[list.length - 1];

  async function close(id: string) {
    try {
      await terminalApi.close(id);
    } catch (e) {
      // Already gone in the engine: drop the tab anyway.
      if (!String(e).includes("closed")) toast(String(e), "err");
    }
    setTerminals((rows) => (rows || []).filter((t) => t.id !== id));
    if (active === id) setActive("");
  }

  return (
    <section className="terminal-panel" aria-label="Terminals">
      <div className="terminal-tabs">
        {list.map((t) => (
          <div
            key={t.id}
            className={`terminal-tab ${shown?.id === t.id ? "on" : ""}`}
          >
            <button
              type="button"
              aria-pressed={shown?.id === t.id}
              onClick={() => setActive(t.id)}
              title={t.shell}
            >
              {t.title}
              {t.exited && <span className="dim"> · exited</span>}
            </button>
            <button
              type="button"
              className="terminal-tab-close"
              aria-label={`Close ${t.title}`}
              onClick={() => void close(t.id)}
            >
              <X size={12} aria-hidden="true" />
            </button>
          </div>
        ))}
        <button
          type="button"
          className="icon-btn terminal-new"
          aria-label="New terminal"
          title="New terminal"
          disabled={opening}
          onClick={() => void open()}
        >
          <Plus size={14} aria-hidden="true" />
        </button>
      </div>
      <p className="hint terminal-hint">
        Your own shell in {workspace.split("/").pop() || workspace}. It runs
        outside the agent's sandbox, keeps running while the agent works, and
        the agent never sees it.
      </p>
      {error && (
        <p className="hint error" role="alert">
          {error}
        </p>
      )}
      {terminals === null && <p role="status">Starting a terminal…</p>}
      {terminals !== null && !list.length && !opening && (
        <div className="terminal-empty">
          <p className="hint">No terminal is open in this project.</p>
          <button type="button" className="mini" onClick={() => void open()}>
            Open a terminal
          </button>
        </div>
      )}
      {shown && (
        <>
          <TerminalView
            key={shown.id}
            id={shown.id}
            title={shown.title}
            onError={(message) => setError(message)}
            onState={(chunk) => {
              if (chunk.exited !== shown.exited)
                setTerminals((rows) =>
                  (rows || []).map((t) =>
                    t.id === chunk.id
                      ? { ...t, exited: chunk.exited, exit_code: chunk.exit_code }
                      : t,
                  ),
                );
            }}
          />
          {shown.exited && (
            <div className="terminal-exited" role="status">
              <span>
                The shell exited
                {shown.exit_code !== null ? ` with code ${shown.exit_code}` : ""}
                .
              </span>
              <button
                type="button"
                className="mini"
                onClick={() => {
                  void close(shown.id).then(open);
                }}
              >
                Restart
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
