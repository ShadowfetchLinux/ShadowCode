import {
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  ArrowUp,
  FileCode2,
  ListPlus,
  LoaderCircle,
  Paperclip,
  Square,
  X,
} from "lucide-react";
import {
  IMAGE_ACCEPT,
  TEXT_ACCEPT,
  pastedImages,
  type Attachment,
} from "../lib/attachments";

export type SlashCommand = {
  name: string;
  description: string;
  arg_spec: string;
};

/** Message field, attachments, the model picker and Send/Stop. Everything that
 * decides whether a message may be sent is computed by the caller. */
export function Composer({
  task,
  onTask,
  promptRef,
  attachments,
  onRemoveAttachment,
  onAttach,
  canAttachImages,
  attachDisabled,
  picker,
  controls,
  commands,
  placeholder,
  hint,
  busy,
  queueing,
  submitting,
  locked,
  canSend,
  sendBlocked,
  stopDisabled,
  onSubmit,
  onStop,
}: {
  task: string;
  onTask: (value: string) => void;
  promptRef: RefObject<HTMLTextAreaElement | null>;
  attachments: Attachment[];
  onRemoveAttachment: (path: string) => void;
  onAttach: (files: File[]) => void;
  canAttachImages: boolean;
  attachDisabled: boolean;
  picker: ReactNode;
  controls: ReactNode;
  commands: SlashCommand[];
  placeholder: string;
  hint: string;
  busy: boolean;
  queueing: boolean;
  submitting: boolean;
  locked: boolean;
  canSend: boolean;
  /** Why Send is unavailable (shown under the field). */
  sendBlocked: string | null;
  stopDisabled: boolean;
  onSubmit: () => void;
  onStop: () => void;
}) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [slashIndex, setSlashIndex] = useState(0);
  const [slashOpen, setSlashOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const hits = slashOpen
    ? commands.filter((c) => c.name.startsWith(task.slice(1))).slice(0, 8)
    : [];
  useEffect(() => {
    const el = promptRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
    }
  }, [task, promptRef]);
  useEffect(() => {
    if (!task.startsWith("/")) setSlashOpen(false);
  }, [task]);
  return (
    <div
      className={`composer-shell ${dragging ? "is-dragging" : ""}`}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("Files")) {
          e.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDragging(false);
        if (e.dataTransfer.files.length)
          onAttach(Array.from(e.dataTransfer.files));
      }}
    >
      {slashOpen && (
        <div className="slash-menu" role="listbox" aria-label="Slash commands">
          {hits.length ? (
            hits.map((c, i) => (
              <div
                role="option"
                aria-selected={i === slashIndex}
                className={`slash-hit ${i === slashIndex ? "on" : ""}`}
                key={c.name}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  onTask(`/${c.name}${c.arg_spec ? " " : ""}`);
                  setSlashOpen(false);
                  promptRef.current?.focus();
                }}
              >
                <strong>/{c.name}</strong>
                <span>{c.description}</span>
              </div>
            ))
          ) : (
            <div className="slash-empty">No matching commands</div>
          )}
        </div>
      )}
      <form
        className={`composer ${locked ? "is-working" : ""}`}
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit();
        }}
      >
        {attachments.length > 0 && (
          <ul className="chips" aria-label="Attachments">
            {attachments.map((a) => (
              <li key={a.path} className="path-chip">
                {a.preview ? (
                  <img src={a.preview} alt="" className="chip-preview" />
                ) : (
                  <FileCode2 size={12} aria-hidden="true" />
                )}
                <span>{a.name}</span>
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={`Remove ${a.name}`}
                  onClick={() => onRemoveAttachment(a.path)}
                >
                  <X size={12} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <textarea
          ref={promptRef}
          aria-label="Message ShadowCode"
          aria-describedby={sendBlocked ? "composer-blocked" : undefined}
          value={task}
          rows={2}
          placeholder={placeholder}
          onChange={(e) => {
            onTask(e.target.value);
            setSlashIndex(0);
            setSlashOpen(/^\/\S*$/.test(e.target.value));
          }}
          onPaste={(e) => {
            const images = pastedImages(e.clipboardData);
            if (images.length) {
              e.preventDefault();
              onAttach(images);
            }
          }}
          onKeyDown={(e) => {
            if (slashOpen && hits.length) {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                setSlashIndex(
                  (i) =>
                    (i + (e.key === "ArrowDown" ? 1 : hits.length - 1)) %
                    hits.length,
                );
                return;
              }
              if (e.key === "Tab") {
                e.preventDefault();
                onTask(`/${hits[slashIndex].name} `);
                setSlashOpen(false);
                return;
              }
            }
            if (e.key === "Escape" && slashOpen) {
              e.stopPropagation();
              setSlashOpen(false);
              return;
            }
            if (
              e.key === "Enter" &&
              !e.shiftKey &&
              !e.nativeEvent.isComposing
            ) {
              e.preventDefault();
              onSubmit();
            }
          }}
        />
        <div className="composer-footer">
          <button
            type="button"
            className="icon-btn attach-btn"
            aria-label={
              canAttachImages ? "Attach files or images" : "Attach text files"
            }
            title={
              canAttachImages
                ? "Attach files or images (PNG, JPEG, WebP). You can also paste or drop images."
                : "Attach text files. Choose a model marked Vision to attach images."
            }
            disabled={attachDisabled}
            onClick={() => fileRef.current?.click()}
          >
            <Paperclip size={17} aria-hidden="true" />
          </button>
          <input
            ref={fileRef}
            type="file"
            multiple
            accept={
              canAttachImages ? `${IMAGE_ACCEPT},${TEXT_ACCEPT}` : TEXT_ACCEPT
            }
            hidden
            onChange={(e) => {
              if (e.target.files) onAttach(Array.from(e.target.files));
              e.target.value = "";
            }}
          />
          {picker}
          {controls}
          <span className="grow" />
          <span className="composer-hint">{hint}</span>
          {busy && (
            <button
              type="button"
              className="submit-btn stop"
              aria-label="Stop task"
              title="Stop task (Ctrl+.)"
              disabled={stopDisabled}
              onClick={onStop}
            >
              <Square size={14} fill="currentColor" aria-hidden="true" />
            </button>
          )}
          <button
            type="submit"
            className="submit-btn"
            aria-label={queueing ? "Queue follow-up" : "Send task"}
            title={sendBlocked || (queueing ? "Queue follow-up" : "Send task")}
            disabled={!canSend}
          >
            {submitting ? (
              <LoaderCircle size={17} className="spin" aria-hidden="true" />
            ) : queueing ? (
              <ListPlus size={19} aria-hidden="true" />
            ) : (
              <ArrowUp size={19} aria-hidden="true" />
            )}
          </button>
        </div>
      </form>
      {sendBlocked && (
        <p className="composer-blocked" id="composer-blocked" role="status">
          {sendBlocked}
        </p>
      )}
    </div>
  );
}
