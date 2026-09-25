import { useCallback, useEffect, useRef, useState } from "react";
import { forgeApi, type PrChecks } from "../lib/forge";

/** How often CI checks refresh while the panel is visible. */
export const CHECKS_REFRESH_MS = 60_000;

/** A pull request's CI checks: read on demand, and every minute while the
 * panel is mounted and the window visible (again as soon as it shows). */
export function usePrChecks(
  number: number | null | undefined,
  remote = "",
  read: typeof forgeApi.checks = forgeApi.checks,
) {
  const [checks, setChecks] = useState<PrChecks | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const reader = useRef(read);
  reader.current = read;
  const refresh = useCallback(async () => {
    if (!number) return;
    setLoading(true);
    try {
      setChecks(await reader.current(number, remote));
      setError("");
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [number, remote]);
  useEffect(() => {
    setChecks(null);
    if (!number) return;
    void refresh();
    const tick = () => {
      if (document.visibilityState !== "hidden") void refresh();
    };
    const timer = setInterval(tick, CHECKS_REFRESH_MS);
    const shown = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", shown);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", shown);
    };
  }, [number, refresh]);
  return { checks, error, loading, refresh };
}
