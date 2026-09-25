import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, renderHook } from "@testing-library/react";
import {
  shortcutFor,
  useShortcuts,
  type ShortcutContext,
} from "./useShortcuts";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const closed: ShortcutContext = {
  consent: false,
  overlay: false,
  trust: false,
  picker: false,
  panel: false,
  onboarding: false,
};
const key = (
  k: string,
  extra: Partial<KeyboardEvent> = {},
): Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "target"> =>
  ({
    key: k,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    target: document.body,
    ...extra,
  }) as KeyboardEvent;

it("maps the documented shortcuts", () => {
  const ctrl = { ctrlKey: true };
  expect(shortcutFor(key("k", ctrl), closed)).toBe("palette");
  expect(shortcutFor(key("K", { metaKey: true }), closed)).toBe("palette");
  expect(shortcutFor(key("b", ctrl), closed)).toBe("sidebar");
  expect(shortcutFor(key("B", { ...ctrl, shiftKey: true }), closed)).toBe(
    "changes",
  );
  expect(shortcutFor(key(",", ctrl), closed)).toBe("settings");
  expect(shortcutFor(key("p", ctrl), closed)).toBe("project");
  expect(shortcutFor(key("n", ctrl), closed)).toBe("new");
  expect(shortcutFor(key("m", ctrl), closed)).toBe("model");
  expect(shortcutFor(key("l", ctrl), closed)).toBe("focus");
  expect(shortcutFor(key(".", ctrl), closed)).toBe("stop");
  expect(shortcutFor(key("E", { ...ctrl, shiftKey: true }), closed)).toBe(
    "export",
  );
  expect(shortcutFor(key("?"), closed)).toBe("help");
  expect(shortcutFor(key("k"), closed)).toBeNull();
});

it("does not open help while typing", () => {
  const input = document.createElement("textarea");
  expect(shortcutFor(key("?", { target: input }), closed)).toBeNull();
});

it("Escape closes the topmost layer; dialogs own the keyboard", () => {
  const esc = key("Escape");
  expect(shortcutFor(esc, { ...closed, overlay: true, panel: true })).toBe(
    "close-overlay",
  );
  expect(shortcutFor(esc, { ...closed, trust: true, picker: true })).toBe(
    "close-trust",
  );
  expect(shortcutFor(esc, { ...closed, picker: true, panel: true })).toBe(
    "close-picker",
  );
  expect(shortcutFor(esc, { ...closed, panel: true })).toBe("close-panel");
  expect(shortcutFor(esc, { ...closed, consent: true, overlay: true })).toBe(
    null,
  );
  expect(shortcutFor(esc, closed)).toBeNull();
  for (const open of ["overlay", "trust", "consent", "onboarding"] as const)
    expect(
      shortcutFor(key("k", { ctrlKey: true }), { ...closed, [open]: true }),
    ).toBeNull();
});

it("registers one listener for the app's lifetime and reads the latest state", () => {
  const add = vi.spyOn(window, "addEventListener");
  const run = vi.fn();
  const { rerender, unmount } = renderHook(
    ({ open, handler }) => useShortcuts(open, handler),
    { initialProps: { open: closed, handler: run } },
  );
  const next = vi.fn();
  for (let i = 0; i < 5; i++)
    rerender({ open: { ...closed, panel: i % 2 === 0 }, handler: next });
  rerender({ open: { ...closed, overlay: true }, handler: next });
  expect(add.mock.calls.filter(([type]) => type === "keydown")).toHaveLength(1);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(run).not.toHaveBeenCalled();
  expect(next).toHaveBeenCalledWith("close-overlay");
  rerender({ open: closed, handler: next });
  const event = new KeyboardEvent("keydown", {
    key: "k",
    ctrlKey: true,
    cancelable: true,
  });
  window.dispatchEvent(event);
  expect(next).toHaveBeenLastCalledWith("palette");
  expect(event.defaultPrevented).toBe(true);
  const remove = vi.spyOn(window, "removeEventListener");
  unmount();
  expect(remove.mock.calls.filter(([type]) => type === "keydown")).toHaveLength(
    1,
  );
});
