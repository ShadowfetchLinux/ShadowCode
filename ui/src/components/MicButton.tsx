import { useEffect, useRef, type RefObject } from "react";
import { LoaderCircle, Mic, Undo2, X } from "lucide-react";
import { HOLD_MS, useVoice, type VoiceState } from "../hooks/useVoice";
import { formatSeconds } from "../lib/voice";
import "../voice.css";

/** The dictation shortcut: Ctrl+Shift+Space (hold to talk, or tap to start
 * and tap again to stop). Pure, for tests. */
export function isVoiceShortcut(
  e: Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "shiftKey">,
): boolean {
  return (
    (e.ctrlKey || e.metaKey) &&
    e.shiftKey &&
    (e.code === "Space" || e.key === " ")
  );
}

const BARS = [0.35, 0.7, 1, 0.7, 0.35];

/** What the mic button says and shows for a state. Pure, for tests. */
export function micLabel(state: VoiceState): string {
  switch (state) {
    case "starting":
      return "Starting the microphone…";
    case "listening":
      return "Stop dictation and insert the text";
    case "transcribing":
      return "Transcribing…";
    default:
      return "Dictate (hold to talk, or click to start and stop)";
  }
}

/** Composer mic button with a level meter, the listening/transcribing state,
 * live text, Cancel and a short-lived Undo for the last insert. */
export function MicButton({
  task,
  onTask,
  promptRef,
  disabled,
  onError,
  onOpenSettings,
}: {
  task: string;
  onTask: (value: string) => void;
  promptRef: RefObject<HTMLTextAreaElement | null>;
  disabled?: boolean;
  onError: (message: string) => void;
  /** Voice input is not set up: show Settings › Voice. */
  onOpenSettings: () => void;
}) {
  const voice = useVoice({
    task,
    onTask,
    promptRef,
    onError,
    onBlocked: (message) => {
      onError(message);
      onOpenSettings();
    },
  });
  const { state, live } = voice;
  const pressedAt = useRef<number | null>(null);
  const active = state === "starting" || state === "listening";

  // Keyboard: Ctrl+Shift+Space held (push-to-talk) or tapped (toggle);
  // Escape cancels while listening.
  const voiceRef = useRef(voice);
  voiceRef.current = voice;
  const disabledRef = useRef(disabled);
  disabledRef.current = disabled;
  useEffect(() => {
    let keyDownAt: number | null = null;
    const blocked = (target: EventTarget | null) =>
      Boolean(
        (target as HTMLElement | null)?.closest?.(
          '[role="dialog"], [data-terminal]',
        ),
      );
    const onDown = (e: KeyboardEvent) => {
      const v = voiceRef.current;
      if (
        e.key === "Escape" &&
        (v.state === "listening" || v.state === "starting")
      ) {
        e.preventDefault();
        e.stopPropagation();
        void v.cancel();
        return;
      }
      if (!isVoiceShortcut(e) || blocked(e.target)) return;
      e.preventDefault();
      if (e.repeat) return;
      if (v.state === "idle" && !disabledRef.current) {
        keyDownAt = Date.now();
        void v.start();
      } else if (v.state === "listening" || v.state === "starting") {
        keyDownAt = null;
        void v.stop();
      }
    };
    const onUp = (e: KeyboardEvent) => {
      if (keyDownAt === null) return;
      if (
        e.code !== "Space" &&
        e.key !== " " &&
        e.key !== "Control" &&
        e.key !== "Meta"
      )
        return;
      const held = Date.now() - keyDownAt;
      keyDownAt = null;
      if (held >= HOLD_MS) void voiceRef.current.stop();
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, []);

  const level = live.level ?? 0;
  return (
    <span className={`voice-input is-${state}`}>
      {(active || state === "transcribing") && (
        <span className="voice-status" role="status" aria-live="polite">
          {state === "transcribing" ? (
            "Transcribing…"
          ) : state === "starting" ? (
            "Starting…"
          ) : (
            <>
              <span className="voice-dot" aria-hidden="true" />
              Listening {formatSeconds(live.seconds ?? 0)}
              {live.partial ? (
                <span className="voice-partial" title={live.partial}>
                  {live.partial.length > 60
                    ? `…${live.partial.slice(-60)}`
                    : live.partial}
                </span>
              ) : null}
            </>
          )}
        </span>
      )}
      {active && (
        <button
          type="button"
          className="icon-btn voice-cancel"
          aria-label="Cancel dictation"
          title="Cancel dictation (Esc)"
          onClick={() => void voice.cancel()}
        >
          <X size={14} aria-hidden="true" />
        </button>
      )}
      {voice.canUndo && state === "idle" && (
        <button
          type="button"
          className="ghost voice-undo"
          title="Remove the dictated text"
          onClick={voice.undo}
        >
          <Undo2 size={13} aria-hidden="true" /> Undo
        </button>
      )}
      <button
        type="button"
        className={`icon-btn mic-btn ${active ? "is-on" : ""}`}
        aria-label={micLabel(state)}
        aria-pressed={active}
        title={`${micLabel(state)} · Ctrl+Shift+Space`}
        disabled={(disabled && state === "idle") || state === "transcribing"}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          e.preventDefault(); // keep the caret in the composer
          // Release outside the button still ends push-to-talk.
          try {
            e.currentTarget.setPointerCapture?.(e.pointerId);
          } catch {
            // Synthetic events have no active pointer to capture.
          }
          if (state === "idle") {
            pressedAt.current = Date.now();
            void voice.start();
          } else if (active) {
            pressedAt.current = null;
            void voice.stop();
          }
        }}
        onPointerUp={() => {
          const at = pressedAt.current;
          pressedAt.current = null;
          if (at !== null && Date.now() - at >= HOLD_MS) void voice.stop();
        }}
        onClick={(e) => {
          // Keyboard and assistive technology clicks (pointer clicks are
          // handled above).
          if (e.detail !== 0) return;
          if (state === "idle") void voice.start();
          else if (active) void voice.stop();
        }}
      >
        {state === "transcribing" ? (
          <LoaderCircle size={16} className="spin" aria-hidden="true" />
        ) : state === "listening" ? (
          <span className="voice-meter" aria-hidden="true">
            {BARS.map((weight, i) => (
              <span
                key={i}
                style={{
                  transform: `scaleY(${Math.max(0.15, Math.min(1, level * 1.4 * weight))})`,
                }}
              />
            ))}
          </span>
        ) : (
          <Mic size={16} aria-hidden="true" />
        )}
      </button>
    </span>
  );
}
