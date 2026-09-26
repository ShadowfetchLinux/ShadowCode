import { readStore, writeStore } from "./storage";

/** Prompts sent in a project, newest last, for ↑/↓ recall in an empty
 * composer. Browser storage only: a convenience that may be absent. */
export const HISTORY_LIMIT = 100;
export const historyKey = (workspace: string) =>
  `shadow:history:${workspace || "none"}`;

export function readHistory(workspace: string): string[] {
  try {
    const value = JSON.parse(readStore(historyKey(workspace)) || "[]");
    return Array.isArray(value)
      ? value.filter((v): v is string => typeof v === "string")
      : [];
  } catch {
    return [];
  }
}

/** Remember a sent prompt; a repeat moves to the end instead of doubling. */
export function pushHistory(workspace: string, prompt: string): string[] {
  const text = prompt.trim();
  const list = readHistory(workspace);
  if (!text) return list;
  const next = [...list.filter((p) => p !== text), text].slice(-HISTORY_LIMIT);
  writeStore(historyKey(workspace), JSON.stringify(next));
  return next;
}

/** One step through the history. `index` is how far back the composer is
 * (-1: not browsing). Returns the new index and the text to show; stepping
 * past the newest entry restores the draft the user started from. */
export function stepHistory(
  list: string[],
  index: number,
  direction: "older" | "newer",
  draft: string,
): { index: number; text: string } | null {
  if (direction === "older") {
    if (index + 1 >= list.length) return null;
    const next = index + 1;
    return { index: next, text: list[list.length - 1 - next] };
  }
  if (index < 0) return null;
  const next = index - 1;
  return {
    index: next,
    text: next < 0 ? draft : list[list.length - 1 - next],
  };
}
