import { useCallback, useState, type SetStateAction } from "react";
import type { PreviewDevice } from "../components/PreviewPanel";
import type { ToolsView } from "../components/ToolsTab";
import type { IssueLink } from "../lib/issues";

/** A pull request being written in the Git tab. */
export type PrDraft = {
  title: string;
  body: string;
  /** Empty: the repository's default branch. */
  base: string;
  draft: boolean;
};

/** Drawer work that must survive switching tabs (and closing the drawer):
 * the terminal tab in front (the shells themselves live in the engine), the
 * folder and open file in Files, the commit message and pull request drafts,
 * the last Tools view, and the page and device width in Preview. A different
 * project starts fresh. */
export type DrawerMemory = {
  terminalActive: string;
  filesDir: string;
  filesView: { path: string; content: string } | null;
  commitMessage: string;
  prDraft: PrDraft;
  toolsView: ToolsView;
  previewUrl: string;
  previewDevice: PreviewDevice;
  /** The issue the latest task was started from (Tools › Issues). */
  issueTask: IssueLink | null;
};
export type DrawerMemoryUpdate = <K extends keyof DrawerMemory>(
  key: K,
  value: SetStateAction<DrawerMemory[K]>,
) => void;

export const emptyDrawerMemory = (): DrawerMemory => ({
  terminalActive: "",
  filesDir: ".",
  filesView: null,
  commitMessage: "",
  prDraft: { title: "", body: "", base: "", draft: false },
  toolsView: "goals",
  previewUrl: "",
  previewDevice: "desktop",
  issueTask: null,
});

export function useDrawerMemory(workspace: string) {
  const [state, setState] = useState(() => ({
    workspace,
    memory: emptyDrawerMemory(),
  }));
  const memory =
    state.workspace === workspace ? state.memory : emptyDrawerMemory();
  if (state.workspace !== workspace) setState({ workspace, memory });
  const update: DrawerMemoryUpdate = useCallback((key, value) => {
    setState((current) => {
      const previous = current.memory[key];
      const next =
        typeof value === "function"
          ? (value as (prev: typeof previous) => typeof previous)(previous)
          : value;
      return next === previous
        ? current
        : { ...current, memory: { ...current.memory, [key]: next } };
    });
  }, []);
  return { memory, update };
}

/** `useState`-shaped access to one remembered drawer value. */
export function remembered<K extends keyof DrawerMemory>(
  memory: DrawerMemory,
  update: DrawerMemoryUpdate,
  key: K,
): [DrawerMemory[K], (value: SetStateAction<DrawerMemory[K]>) => void] {
  return [memory[key], (value) => update(key, value)];
}
