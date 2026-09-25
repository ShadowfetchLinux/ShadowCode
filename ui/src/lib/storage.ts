/** Browser storage holds conveniences only (drafts, the last model per
 * project, panel state); every read and write tolerates a blocked store. */
export const readStore = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};

export const writeStore = (key: string, value: string | null) => {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* Browser storage only holds conveniences. */
  }
};

export const draftKey = (id: string, workspace: string) =>
  `shadow:draft:${id || workspace}`;
export const targetKey = (workspace: string) => `shadow:model:${workspace}`;
export const isSessionCommand = (text: string) =>
  ["/new", "/clear"].includes(text.trim());
