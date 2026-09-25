import { useCallback, useState, type SetStateAction } from "react";
import type { ExecResult } from "../api";

/** Drawer work that must survive switching tabs (and closing the drawer):
 * terminal output and the unsent command, the folder and open file in Files,
 * and the commit message draft. A different project starts fresh. */
export type DrawerMemory = {
  terminalCommand: string;
  terminalHistory: ExecResult[];
  filesDir: string;
  filesView: { path: string; content: string } | null;
  commitMessage: string;
};
export type DrawerMemoryUpdate = <K extends keyof DrawerMemory>(
  key: K,
  value: SetStateAction<DrawerMemory[K]>,
) => void;

export const emptyDrawerMemory = (): DrawerMemory => ({
  terminalCommand: "",
  terminalHistory: [],
  filesDir: ".",
  filesView: null,
  commitMessage: "",
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
