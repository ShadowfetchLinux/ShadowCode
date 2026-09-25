import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { CHECKS_REFRESH_MS, usePrChecks } from "./usePrChecks";
import type { PrChecks } from "../lib/forge";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const answer: PrChecks = {
  supported: true,
  checks: [],
  summary: {},
  overall: "none",
  url: "https://github.com/o/r/pull/1/checks",
};

it("refreshes every minute while visible and on demand", async () => {
  vi.useFakeTimers();
  const read = vi.fn(async () => answer);
  const { result, unmount } = renderHook(() => usePrChecks(1, "origin", read));
  await act(async () => undefined);
  expect(read).toHaveBeenCalledTimes(1);
  expect(read).toHaveBeenCalledWith(1, "origin");
  await act(async () => {
    vi.advanceTimersByTime(CHECKS_REFRESH_MS);
  });
  expect(read).toHaveBeenCalledTimes(2);
  // Hidden windows skip the timer…
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => "hidden",
  });
  await act(async () => {
    vi.advanceTimersByTime(CHECKS_REFRESH_MS);
  });
  expect(read).toHaveBeenCalledTimes(2);
  // …and catch up as soon as they show again.
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => "visible",
  });
  await act(async () => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(read).toHaveBeenCalledTimes(3);
  await act(async () => {
    await result.current.refresh();
  });
  expect(read).toHaveBeenCalledTimes(4);
  unmount();
  await act(async () => {
    vi.advanceTimersByTime(CHECKS_REFRESH_MS * 3);
  });
  expect(read).toHaveBeenCalledTimes(4);
});

it("does nothing without a pull request", async () => {
  const read = vi.fn(async () => answer);
  renderHook(() => usePrChecks(null, "", read));
  await act(async () => undefined);
  expect(read).not.toHaveBeenCalled();
});
