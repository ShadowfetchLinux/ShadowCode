import { MousePointerClick, SquareTerminal, X } from "lucide-react";
import type { ContextAttachment } from "../lib/pendingAttachments";

/** Chips for context waiting to go out with the next message (picked
 * elements, console errors); rendered inside the composer's chip list. */
export function ContextChips({
  items,
  onRemove,
}: {
  items: readonly ContextAttachment[];
  onRemove?: (id: string) => void;
}) {
  return (
    <>
      {items.map((item) => (
        <li
          key={item.id}
          className="path-chip context-chip"
          title={item.detail}
        >
          {item.kind === "element" ? (
            <MousePointerClick size={12} aria-hidden="true" />
          ) : (
            <SquareTerminal size={12} aria-hidden="true" />
          )}
          <span>{item.label}</span>
          <button
            type="button"
            className="chip-remove"
            aria-label={`Remove ${item.label}`}
            onClick={() => onRemove?.(item.id)}
          >
            <X size={12} aria-hidden="true" />
          </button>
        </li>
      ))}
    </>
  );
}
