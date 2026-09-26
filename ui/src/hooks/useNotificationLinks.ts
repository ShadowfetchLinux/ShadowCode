import { useEffect, useRef } from "react";
import { invoke, isNative, listen } from "../lib/transport";

/** Desktop notifications: the shell is told which conversation is on screen
 * (it does not notify about that one while the window is focused), and a
 * clicked notification opens its conversation. */
export function useNotificationLinks(
  sessionId: string,
  openSession: (id: string) => void,
) {
  const open = useRef(openSession);
  open.current = openSession;
  useEffect(() => {
    if (!isNative()) return;
    void invoke("set_visible_session", { sessionId }).catch(() => undefined);
  }, [sessionId]);
  useEffect(() => {
    if (!isNative()) return;
    let stopped = false;
    let unsubscribe: (() => void) | undefined;
    void listen("shadowcode:open-session", (payload) => {
      const id = (payload as { session_id?: unknown } | null)?.session_id;
      if (typeof id === "string" && id) open.current(id);
    })
      .then((stop) => {
        if (stopped) stop();
        else unsubscribe = stop;
      })
      .catch(() => undefined);
    return () => {
      stopped = true;
      unsubscribe?.();
    };
  }, []);
}
