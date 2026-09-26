import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { insertAtCursor, voiceApi, type VoiceRecording } from "../lib/voice";

export type VoiceState = "idle" | "starting" | "listening" | "transcribing";

/** Held at least this long, the press was push-to-talk: releasing stops. A
 * shorter tap starts listening until the next tap. */
export const HOLD_MS = 350;
const POLL_MS = 120;

type Insert = { before: string; after: string };

/** Dictation into the composer: start/stop the engine's recording, poll its
 * level and live text while listening, and insert the transcript at the
 * cursor (never sends it). The last insert can be undone while the text is
 * unchanged since. */
export function useVoice({
  task,
  onTask,
  promptRef,
  onError,
  onBlocked,
}: {
  task: string;
  onTask: (value: string) => void;
  promptRef: RefObject<HTMLTextAreaElement | null>;
  onError: (message: string) => void;
  /** Starting failed because voice input is not set up. */
  onBlocked?: (message: string) => void;
}) {
  const [state, setState] = useState<VoiceState>("idle");
  const [live, setLive] = useState<VoiceRecording>({ active: false });
  const [inserted, setInserted] = useState<Insert | null>(null);
  const stateRef = useRef(state);
  stateRef.current = state;
  const taskRef = useRef(task);
  taskRef.current = task;
  /** Stop as soon as the recording has started (released while starting). */
  const stopWhenStarted = useRef(false);

  const insert = useCallback(
    (text: string) => {
      const before = taskRef.current;
      const el = promptRef.current;
      const start = el ? el.selectionStart : before.length;
      const end = el ? el.selectionEnd : before.length;
      const next = insertAtCursor(before, start, end, text);
      onTask(next.value);
      setInserted({ before, after: next.value });
      requestAnimationFrame(() => {
        const field = promptRef.current;
        if (!field) return;
        field.focus();
        field.setSelectionRange(next.caret, next.caret);
      });
    },
    [onTask, promptRef],
  );

  const stop = useCallback(async () => {
    if (stateRef.current === "starting") {
      stopWhenStarted.current = true;
      return;
    }
    if (stateRef.current !== "listening") return;
    setState("transcribing");
    try {
      const result = await voiceApi.stop();
      if (result.text) insert(result.text);
      else if (result.message) onError(result.message);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setLive({ active: false });
      setState("idle");
    }
  }, [insert, onError]);

  const cancel = useCallback(async () => {
    stopWhenStarted.current = false;
    if (stateRef.current !== "listening" && stateRef.current !== "starting")
      return;
    setState("idle");
    setLive({ active: false });
    try {
      await voiceApi.cancel();
    } catch {
      // Nothing to cancel.
    }
  }, []);

  const start = useCallback(async () => {
    if (stateRef.current !== "idle") return;
    stopWhenStarted.current = false;
    setState("starting");
    try {
      await voiceApi.start();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setState("idle");
      if (onBlocked && /Settings › Voice|API key/.test(message))
        onBlocked(message);
      else onError(message);
      return;
    }
    setLive({ active: true, level: 0, seconds: 0 });
    stateRef.current = "listening";
    setState("listening");
    if (stopWhenStarted.current) {
      stopWhenStarted.current = false;
      await stop();
    }
  }, [onBlocked, onError, stop]);

  // Level, elapsed time and live text while listening; stop at the limit.
  useEffect(() => {
    if (state !== "listening") return;
    let alive = true;
    const timer = window.setInterval(async () => {
      try {
        const now = await voiceApi.recording();
        if (!alive) return;
        if (!now.active) {
          setLive({ active: false });
          setState("idle");
          return;
        }
        setLive(now);
        if (now.error) {
          onError(now.error);
          void cancel();
        } else if (now.full) void stop();
      } catch {
        // The next poll tries again.
      }
    }, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [state, stop, cancel, onError]);

  // Leaving the view never leaves the microphone open.
  useEffect(
    () => () => {
      if (stateRef.current === "listening" || stateRef.current === "starting")
        void voiceApi.cancel().catch(() => undefined);
    },
    [],
  );

  // Undo is offered only while the composer still holds what was inserted.
  const canUndo = Boolean(inserted && inserted.after === task);
  useEffect(() => {
    if (inserted && inserted.after !== task) setInserted(null);
  }, [inserted, task]);
  const undo = useCallback(() => {
    if (!inserted || inserted.after !== taskRef.current) return;
    onTask(inserted.before);
    setInserted(null);
    promptRef.current?.focus();
  }, [inserted, onTask, promptRef]);

  return { state, live, start, stop, cancel, canUndo, undo };
}
