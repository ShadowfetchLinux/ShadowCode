import { useId, useState } from "react";
import { Copy, Pencil, RotateCcw } from "lucide-react";
import type { ChatItem } from "./cards";

type UserItem = Extract<ChatItem, { kind: "user" }>;

/** A user's message with Edit & resend, Retry and Copy. Editing opens the
 * text in place; sending starts a new conversation from just before this
 * message (the original stays), optionally undoing the file changes made
 * from here on. */
export function UserMessage({
  item,
  disabled,
  onEditResend,
  onRetry,
  onCopy,
}: {
  item: UserItem;
  /** A task is running or being sent. */
  disabled: boolean;
  onEditResend: (item: UserItem, text: string, undoFiles: boolean) => void;
  onRetry: (text: string) => void;
  onCopy: (text: string) => void;
}) {
  const id = useId();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(item.text);
  const [undoFiles, setUndoFiles] = useState(false);
  return (
    <div className={`msg-user${item.continued ? " is-continuation" : ""}`}>
      <div className="user-pill">
        {item.continued && (
          <div className="continued-label">
            {item.continued === "auto"
              ? "Continued automatically"
              : "Continued after the plan limit"}
          </div>
        )}
        {editing ? (
          <form
            className="bubble edit-message"
            aria-label="Edit message"
            onSubmit={(e) => {
              e.preventDefault();
              if (!text.trim() || disabled) return;
              setEditing(false);
              onEditResend(item, text.trim(), undoFiles);
            }}
          >
            <label htmlFor={`${id}-text`} className="sr-only">
              Edited message
            </label>
            <textarea
              id={`${id}-text`}
              value={text}
              rows={Math.min(8, Math.max(2, text.split("\n").length))}
              autoFocus
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setEditing(false);
                }
              }}
            />
            <label className="check">
              <input
                type="checkbox"
                checked={undoFiles}
                onChange={(e) => setUndoFiles(e.target.checked)}
              />
              Also undo the file changes made from this message on
            </label>
            <p className="hint">
              Sends the edited message in a new conversation that starts just
              before this one. This conversation is kept as it is.
            </p>
            <div className="row">
              <button
                type="button"
                className="ghost"
                onClick={() => {
                  setEditing(false);
                  setText(item.text);
                }}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="primary"
                disabled={disabled || !text.trim()}
              >
                Send edited message
              </button>
            </div>
          </form>
        ) : (
          <div className="bubble">{item.text}</div>
        )}
        {!editing && (
          <div
            className="message-actions"
            role="group"
            aria-label="Message actions"
          >
            <button
              type="button"
              className="icon-btn"
              aria-label="Edit and resend"
              title={
                disabled
                  ? "Available when the task finishes"
                  : "Edit and resend in a new conversation"
              }
              disabled={disabled || !item.eventId}
              onClick={() => {
                setText(item.text);
                setEditing(true);
              }}
            >
              <Pencil size={13} aria-hidden="true" />
            </button>
            <button
              type="button"
              className="icon-btn"
              aria-label="Retry"
              title="Send this message again"
              disabled={disabled}
              onClick={() => onRetry(item.text)}
            >
              <RotateCcw size={13} aria-hidden="true" />
            </button>
            <CopyButton text={item.text} onCopy={onCopy} />
          </div>
        )}
      </div>
    </div>
  );
}

export function CopyButton({
  text,
  onCopy,
  label = "Copy message",
}: {
  text: string;
  onCopy: (text: string) => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      className="icon-btn"
      aria-label={label}
      title={label}
      onClick={() => onCopy(text)}
    >
      <Copy size={13} aria-hidden="true" />
    </button>
  );
}
