import { useState, type ReactNode } from "react";
import { Dialog } from "./Dialog";

/** The app's confirmation dialog (instead of `window.confirm`): a title, what
 * will happen, and Cancel / a confirm button. `danger` colours the confirm
 * button for actions that discard work. The confirm button shows progress
 * while `onConfirm` runs and the dialog stays until it finishes. */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  danger = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  children?: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
}) {
  const [working, setWorking] = useState(false);
  return (
    <Dialog label={title} onClose={() => !working && onCancel()}>
      <div className="confirm-dialog">
        <h2>{title}</h2>
        {children}
        <div className="row confirm-actions">
          <button
            type="button"
            className="ghost"
            disabled={working}
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            type="button"
            className={danger ? "primary danger" : "primary"}
            disabled={working}
            onClick={async () => {
              setWorking(true);
              try {
                await onConfirm();
              } finally {
                setWorking(false);
              }
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </Dialog>
  );
}
