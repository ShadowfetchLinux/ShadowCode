import { useCallback, useRef } from "react";

/** A function whose identity never changes but always calls the latest
 * `fn`, for handlers passed to memoized rows and long-lived listeners. */
export function useStableCallback<A extends unknown[], R>(
  fn: (...args: A) => R,
): (...args: A) => R {
  const latest = useRef(fn);
  latest.current = fn;
  return useCallback((...args: A) => latest.current(...args), []);
}
