import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";
import {
  THEME_KEY,
  applyInitialTheme,
  resolveTheme,
  useTheme,
} from "./useTheme";

let dark = false;
let listeners: (() => void)[] = [];
beforeEach(() => {
  dark = false;
  listeners = [];
  localStorage.clear();
  delete document.documentElement.dataset.theme;
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        media: query,
        get matches() {
          return dark;
        },
        addEventListener: (_: string, fn: () => void) => listeners.push(fn),
        removeEventListener: (_: string, fn: () => void) => {
          listeners = listeners.filter((l) => l !== fn);
        },
      }) as unknown as MediaQueryList,
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
const theme = () => document.documentElement.dataset.theme;

it("resolves explicit choices and follows the system otherwise", () => {
  expect(resolveTheme("light", true)).toBe("light");
  expect(resolveTheme("dark", false)).toBe("dark");
  expect(resolveTheme("system", true)).toBe("dark");
  expect(resolveTheme(undefined, false)).toBe("light");
  expect(resolveTheme(null, true)).toBe("dark");
});

it("an explicit light theme sticks on a dark system and across config reloads", () => {
  dark = true;
  const { rerender } = renderHook(({ pref }) => useTheme(pref), {
    initialProps: { pref: "light" as string | undefined },
  });
  expect(theme()).toBe("light");
  expect(localStorage.getItem(THEME_KEY)).toBe("light");
  // The system changing does not matter, nor re-reading the same config.
  listeners.forEach((fn) => fn());
  rerender({ pref: "light" });
  expect(theme()).toBe("light");
  expect(listeners).toHaveLength(0);
  // An unchanged preference is not applied again over a manual choice.
  document.documentElement.dataset.theme = "dark";
  rerender({ pref: "light" });
  expect(theme()).toBe("dark");
  document.documentElement.dataset.theme = "light";
  rerender({ pref: "dark" });
  expect(theme()).toBe("dark");
});

it("waits for the config before replacing the startup theme", () => {
  localStorage.setItem(THEME_KEY, "light");
  dark = true;
  applyInitialTheme();
  expect(theme()).toBe("light");
  renderHook(() => useTheme(undefined));
  expect(theme()).toBe("light");
});

it("'system' follows system changes until the preference changes", () => {
  const { rerender, unmount } = renderHook(({ pref }) => useTheme(pref), {
    initialProps: { pref: "system" },
  });
  expect(theme()).toBe("light");
  dark = true;
  listeners.forEach((fn) => fn());
  expect(theme()).toBe("dark");
  rerender({ pref: "light" });
  expect(theme()).toBe("light");
  expect(listeners).toHaveLength(0);
  unmount();
});
