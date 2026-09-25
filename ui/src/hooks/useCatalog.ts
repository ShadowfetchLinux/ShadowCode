import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AllowanceResponse } from "../api";
import type { PickerTarget } from "../lib/picker";
import type { ToastKind } from "./useToasts";

/** The model picker's rows. Requests can overlap (vendor checks are slow);
 * only the newest one may replace the rows, or an older answer would hide a
 * model that was just added. */
export function usePickerTargets(
  toast: (text: string, kind?: ToastKind) => void,
) {
  const [targets, setTargets] = useState<PickerTarget[]>([]);
  const [loaded, setLoaded] = useState(false);
  const fetched = useRef(0);
  const seq = useRef(0);
  const reload = useCallback(
    async (refresh = false) => {
      fetched.current = Date.now();
      const ticket = ++seq.current;
      try {
        const result = await api.picker(refresh);
        if (ticket !== seq.current) return;
        setTargets(Array.isArray(result.targets) ? result.targets : []);
      } catch (e) {
        if (ticket === seq.current)
          toast(`Could not load models: ${String(e)}`, "err");
      } finally {
        if (ticket === seq.current) setLoaded(true);
      }
    },
    [toast],
  );
  return { targets, loaded, reload, fetched };
}

/** Allowance rows for the status bar and its panel. `refresh` re-checks
 * vendor accounts (slow); otherwise the engine answers from its cache. */
export function useAllowance() {
  const [data, setData] = useState<AllowanceResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const seq = useRef(0);
  const reload = useCallback(async (refresh = false) => {
    const ticket = ++seq.current;
    setLoading(true);
    try {
      const result = await api.allowance(refresh);
      if (ticket !== seq.current) return;
      setData(result);
      setError("");
    } catch (e) {
      if (ticket === seq.current) setError(String(e));
    } finally {
      if (ticket === seq.current) setLoading(false);
    }
  }, []);
  return { data, loading, error, reload };
}

/** Readiness and usage change outside the app (sign-in in a browser, plan
 * resets): refresh rows when the window regains focus, at most every 15 s. */
export function useRefreshOnFocus(
  fetched: { current: number },
  reload: () => void,
) {
  const latest = useRef(reload);
  latest.current = reload;
  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - fetched.current > 15000) latest.current();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [fetched]);
}
