import { useEffect } from "react";
import { readStore, writeStore } from "../lib/storage";

/** The saved appearance (`ui.theme`), kept in browser storage too so a
 * reload opens in the chosen theme before the config has loaded. */
export const THEME_KEY = "shadow:theme";

export type Theme = "light" | "dark";

const systemDark = () =>
  Boolean(window.matchMedia?.("(prefers-color-scheme: dark)")?.matches);

/** Explicit light/dark, or the system's choice for "system" and unset. */
export function resolveTheme(
  preference: string | null | undefined,
  dark = systemDark(),
): Theme {
  if (preference === "light" || preference === "dark") return preference;
  return dark ? "dark" : "light";
}

export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

/** Before the first render: the last saved preference, else the system. */
export function applyInitialTheme() {
  applyTheme(resolveTheme(readStore(THEME_KEY)));
}

/** Applies the saved preference whenever it changes. `undefined` means the
 * config has not loaded yet, so the theme chosen at startup stays. The
 * document follows system changes only while the preference is "system".
 * Re-reading an unchanged config does not re-apply it. */
export function useTheme(preference: string | undefined) {
  useEffect(() => {
    if (preference === undefined) return;
    writeStore(THEME_KEY, preference);
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    const apply = () => applyTheme(resolveTheme(preference, media?.matches));
    apply();
    if (preference === "light" || preference === "dark" || !media) return;
    media.addEventListener?.("change", apply);
    return () => media.removeEventListener?.("change", apply);
  }, [preference]);
}
