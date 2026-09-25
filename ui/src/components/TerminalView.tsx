/** One terminal's screen: an xterm.js view of a shell the engine keeps.
 * Mounting replays the engine's scrollback, so switching tabs or closing
 * the drawer loses nothing; the shell itself keeps running. */
import { useEffect, useRef } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  inputQueue,
  outputReader,
  subscribeTerminals,
  terminalApi,
  type TerminalChunk,
} from "../lib/terminal";

function theme(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) =>
    css.getPropertyValue(name).trim() || fallback;
  return {
    background: token("--surface-2", "#f5f5f2"),
    foreground: token("--text", "#242824"),
    cursor: token("--accent", "#386c51"),
    cursorAccent: token("--surface-2", "#f5f5f2"),
    selectionBackground: token("--accent-soft", "#e6eee7"),
    selectionForeground: token("--text", "#242824"),
  };
}

export function TerminalView({
  id,
  title,
  onState,
  onError,
}: {
  id: string;
  title: string;
  onState: (chunk: TerminalChunk) => void;
  onError: (message: string) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const state = useRef(onState);
  state.current = onState;
  const failed = useRef(onError);
  failed.current = onError;

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const css = getComputedStyle(document.documentElement);
    const term = new Terminal({
      cursorBlink: true,
      fontFamily:
        css.getPropertyValue("--mono").trim() || "ui-monospace, monospace",
      fontSize: 12.5,
      scrollback: 5000,
      allowProposedApi: false,
      theme: theme(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(element);
    const input = inputQueue(
      (data) => terminalApi.input(id, data),
      (error) => failed.current(String(error)),
    );
    const typing = term.onData((data) => input.push(data));
    let lastSize = "";
    const sizing = term.onResize(({ cols, rows }) => {
      const key = `${cols}x${rows}`;
      if (key === lastSize) return;
      lastSize = key;
      void terminalApi.resize(id, cols, rows).catch(() => undefined);
    });
    const reader = outputReader({
      id,
      read: terminalApi.output,
      write: (bytes) => term.write(bytes),
      onState: (chunk) => state.current(chunk),
    });
    const refit = () => {
      if (element.clientWidth > 0 && element.clientHeight > 0) {
        try {
          fit.fit();
        } catch {
          // Not laid out yet; the next resize fits it.
        }
      }
    };
    refit();
    const { cols, rows } = term;
    lastSize = `${cols}x${rows}`;
    void terminalApi.resize(id, cols, rows).catch(() => undefined);
    void reader.wake().catch((error) => failed.current(String(error)));
    const stopWakeups = subscribeTerminals((terminal) => {
      if (terminal === null || terminal === id)
        void reader.wake().catch(() => undefined);
    });
    const observer = new ResizeObserver(() => refit());
    observer.observe(element);
    const recolour = new MutationObserver(() => {
      term.options.theme = theme();
    });
    recolour.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme", "class"],
    });
    term.focus();
    return () => {
      stopWakeups();
      observer.disconnect();
      recolour.disconnect();
      reader.stop();
      input.close();
      typing.dispose();
      sizing.dispose();
      term.dispose();
    };
  }, [id]);

  return (
    <div
      className="terminal-screen"
      ref={host}
      data-terminal={id}
      role="group"
      aria-label={title}
      onClick={(e) =>
        (e.currentTarget.querySelector("textarea") as HTMLElement | null)?.focus()
      }
    />
  );
}
