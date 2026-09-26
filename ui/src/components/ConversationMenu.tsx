import { useEffect, useRef, useState } from "react";
import { Download, GitFork, Pencil, Pin, Trash2 } from "lucide-react";

export type ConversationAction =
  "rename" | "pin" | "fork" | "export" | "delete";

/** The right-click menu of a sidebar conversation. Delete asks once more
 * inside the menu. Arrow keys move between items; Escape closes. */
export function ConversationMenu({
  x,
  y,
  title,
  pinned,
  onAction,
  onClose,
}: {
  x: number;
  y: number;
  title: string;
  pinned: boolean;
  onAction: (action: ConversationAction) => void;
  onClose: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  useEffect(() => {
    menu.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [confirming]);
  useEffect(() => {
    const away = (event: MouseEvent) => {
      if (!menu.current?.contains(event.target as Node)) onClose();
    };
    window.addEventListener("mousedown", away);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", away);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);
  const items: [ConversationAction, string, typeof Pin][] = [
    ["rename", "Rename", Pencil],
    ["pin", pinned ? "Unpin" : "Pin", Pin],
    ["fork", "Fork", GitFork],
    ["export", "Export…", Download],
    ["delete", "Delete…", Trash2],
  ];
  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - 220);
  return (
    <div
      ref={menu}
      className="conversation-menu"
      role="menu"
      aria-label={`Actions for ${title}`}
      style={{ left, top }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
          return;
        }
        if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
        event.preventDefault();
        const buttons = [
          ...(menu.current?.querySelectorAll<HTMLButtonElement>("button") ||
            []),
        ];
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next =
          (at + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) %
          buttons.length;
        buttons[next]?.focus();
      }}
    >
      {confirming ? (
        <div className="menu-confirm">
          <p>Delete “{title}”? Its history is removed.</p>
          <button
            type="button"
            role="menuitem"
            className="danger"
            onClick={() => onAction("delete")}
          >
            Delete
          </button>
          <button type="button" role="menuitem" onClick={onClose}>
            Cancel
          </button>
        </div>
      ) : (
        items.map(([action, label, Icon]) => (
          <button
            key={action}
            type="button"
            role="menuitem"
            className={action === "delete" ? "danger" : ""}
            onClick={() =>
              action === "delete" ? setConfirming(true) : onAction(action)
            }
          >
            <Icon size={13} aria-hidden="true" />
            {label}
          </button>
        ))
      )}
    </div>
  );
}
