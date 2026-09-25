import { useCallback, useRef, useState } from "react";

export type ToastKind = "ok" | "err" | "info";
export type Toast = { id: number; text: string; kind: ToastKind };

/** Up to four notifications; errors stay 8 s, the rest 5 s. */
export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const seq = useRef(0);
  const toast = useCallback((text: string, kind: ToastKind = "info") => {
    const id = ++seq.current;
    setToasts((prev) => [...prev.slice(-3), { id, text, kind }]);
    const duration = kind === "err" ? 8000 : 5000;
    setTimeout(
      () => setToasts((prev) => prev.filter((t) => t.id !== id)),
      duration,
    );
  }, []);
  const dismiss = useCallback(
    (id: number) => setToasts((prev) => prev.filter((t) => t.id !== id)),
    [],
  );
  return { toasts, toast, dismiss };
}
