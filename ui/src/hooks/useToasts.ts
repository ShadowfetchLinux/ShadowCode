import { useCallback, useRef, useState } from "react";

export type ToastKind = "ok" | "err" | "info";
/** A button on the notification (Undo after a rewind). */
export type ToastAction = { label: string; run: () => void };
export type Toast = {
  id: number;
  text: string;
  kind: ToastKind;
  action?: ToastAction;
};

/** Up to four notifications; errors stay 8 s, ones with an action 12 s, the
 * rest 5 s. */
export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const seq = useRef(0);
  const toast = useCallback(
    (text: string, kind: ToastKind = "info", action?: ToastAction) => {
      const id = ++seq.current;
      setToasts((prev) => [...prev.slice(-3), { id, text, kind, action }]);
      const duration = action ? 12000 : kind === "err" ? 8000 : 5000;
      setTimeout(
        () => setToasts((prev) => prev.filter((t) => t.id !== id)),
        duration,
      );
    },
    [],
  );
  const dismiss = useCallback(
    (id: number) => setToasts((prev) => prev.filter((t) => t.id !== id)),
    [],
  );
  return { toasts, toast, dismiss };
}
