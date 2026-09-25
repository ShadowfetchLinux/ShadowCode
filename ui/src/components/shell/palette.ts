import type { PaletteItem } from "../overlays";
import type { AdvancedTab, SettingsSection } from "../Settings";
import type { DrawerTab } from "../Drawer";

export type PaletteActions = {
  newTask: () => void;
  chooseModel: () => void;
  openProject: () => void;
  panel: (tab: DrawerTab) => void;
  compare: () => void;
  comparisons: () => void;
  settings: (section?: SettingsSection, advanced?: AdvancedTab) => void;
  exportTask: (format: "md" | "json") => void;
  stop: () => void;
  toggleTheme: () => void;
  help: () => void;
};

/** The command palette (Ctrl+K) entries, in display order. */
export function paletteItems(a: PaletteActions): PaletteItem[] {
  return [
    { id: "new", label: "New task", hint: "Ctrl+N", run: a.newTask },
    {
      id: "model",
      label: "Choose a model",
      hint: "Ctrl+M",
      run: a.chooseModel,
    },
    {
      id: "project",
      label: "Open project",
      hint: "Ctrl+P",
      run: a.openProject,
    },
    { id: "changes", label: "Review changes", run: () => a.panel("changes") },
    { id: "compare", label: "Compare models on this task", run: a.compare },
    {
      id: "comparisons",
      label: "Comparisons in this project",
      run: a.comparisons,
    },
    { id: "files", label: "Browse files", run: () => a.panel("files") },
    {
      id: "terminal",
      label: "Run a terminal command",
      run: () => a.panel("terminal"),
    },
    {
      id: "sessions",
      label: "Manage tasks · rename, branch, export, delete",
      run: () => a.panel("sessions"),
    },
    { id: "accounts", label: "Accounts", run: () => a.settings("accounts") },
    { id: "local", label: "Local models", run: () => a.settings("local") },
    {
      id: "goals",
      label: "Goals and milestones",
      run: () => a.settings("advanced", "goals"),
    },
    {
      id: "skills",
      label: "Skills and instructions",
      run: () => a.settings("advanced", "skills"),
    },
    {
      id: "health",
      label: "Workspace health",
      run: () => a.settings("advanced", "health"),
    },
    {
      id: "export",
      label: "Export this task as Markdown",
      hint: "Ctrl+Shift+E",
      run: () => a.exportTask("md"),
    },
    {
      id: "export-json",
      label: "Export this task as JSON",
      hint: "Complete event records",
      run: () => a.exportTask("json"),
    },
    { id: "stop", label: "Stop the agent", hint: "Ctrl+.", run: a.stop },
    {
      id: "settings",
      label: "Settings",
      hint: "Ctrl+,",
      run: () => a.settings(),
    },
    {
      id: "theme",
      label: "Toggle light / dark appearance",
      run: a.toggleTheme,
    },
    { id: "help", label: "Keyboard shortcuts", hint: "?", run: a.help },
  ];
}
