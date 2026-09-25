import { expect, it } from "vitest";
import { shortcutFor, type ShortcutContext } from "./useShortcuts";

const closed: ShortcutContext = {
  consent: false,
  overlay: false,
  trust: false,
  picker: false,
  panel: true,
  onboarding: false,
};

function inTerminal() {
  const screen = document.createElement("div");
  screen.dataset.terminal = "t";
  const input = document.createElement("textarea");
  screen.appendChild(input);
  return input;
}

it("lets a terminal keep Escape and editing shortcuts", () => {
  const target = inTerminal();
  const key = (key: string, ctrlKey = false) =>
    shortcutFor(
      { key, ctrlKey, metaKey: false, shiftKey: false, target },
      closed,
    );
  expect(key("Escape")).toBeNull();
  for (const k of ["l", "p", "n", "b", "k"]) expect(key(k, true)).toBeNull();
  expect(key("`", true)).toBe("terminal");
});

it("toggles the terminal with Ctrl+` elsewhere", () => {
  const target = document.createElement("div");
  expect(
    shortcutFor(
      { key: "`", ctrlKey: true, metaKey: false, shiftKey: false, target },
      closed,
    ),
  ).toBe("terminal");
  expect(
    shortcutFor(
      { key: "Escape", ctrlKey: false, metaKey: false, shiftKey: false, target },
      closed,
    ),
  ).toBe("close-panel");
});
