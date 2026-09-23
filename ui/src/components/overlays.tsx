import { Dialog } from "./Dialog";
import { useEffect, useState } from "react";
import type { Project, ProviderInfo } from "../api";
import { isNative, pickDirectory } from "../lib/transport";

export const SHORTCUTS: [string, string][] = [
  ["Enter", "Send task (Shift+Enter for a new line)"],
  ["Ctrl+.", "Stop the agent"],
  ["Ctrl+K", "Command palette"],
  ["Ctrl+B", "Toggle the sidebar"],
  ["Ctrl+,", "Settings"],
  ["Ctrl+P", "Open project…"],
  ["Ctrl+N", "New session"],
  ["Ctrl+L", "Focus the composer"],
  ["Ctrl+Shift+E", "Export session as Markdown"],
  ["Enter / Space", "Activate the focused approval button"],
  ["/", "Slash commands in the composer"],
  ["?", "This cheat sheet"],
  ["Esc", "Close overlay or drawer"],
];

const SHORTCUT_GROUPS: { label: string; rows: [string, string][] }[] = [
  {
    label: "Task control",
    rows: [
      ["Enter", "Send task"],
      ["Shift+Enter", "New line in composer"],
      ["Ctrl+.", "Stop the agent"],
      ["/", "Slash commands (in composer)"],
    ],
  },
  {
    label: "Navigation",
    rows: [
      ["Ctrl+K", "Command palette"],
      ["Ctrl+B", "Toggle sidebar"],
      ["Ctrl+P", "Open project…"],
      ["Ctrl+N", "New session"],
      ["Ctrl+L", "Focus composer"],
      ["Esc", "Close overlay or drawer"],
    ],
  },
  {
    label: "Other",
    rows: [
      ["Ctrl+,", "Settings"],
      ["Ctrl+Shift+E", "Export session as Markdown"],
      ["?", "This cheat sheet"],
    ],
  },
];

const EXAMPLE_PROMPTS: string[] = [
  "Build a REST endpoint that does X",
  "Find and fix the bug where Y happens",
  "Write unit tests for the auth module",
  "Explain how the database layer works",
  "Refactor this function to be cleaner",
  "Review the git diff for issues",
];

export function Help({
  onClose,
  version,
}: {
  onClose: () => void;
  version: string;
}) {
  return (
    <Dialog
      label="Help & Shortcuts"
      className="modal modal-sm help-modal"
      onClose={onClose}
    >
      <h2>Keyboard shortcuts</h2>
      <div className="help-groups">
        {SHORTCUT_GROUPS.map((group) => (
          <div className="help-group" key={group.label}>
            <div className="help-group-label">{group.label}</div>
            <div className="help-grid">
              {group.rows.map(([k, label]) => (
                <div className="help-row" key={k}>
                  <span>{label}</span>
                  <span className="kbd">{k}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
      <h2 className="help-section-title">What can I ask?</h2>
      <div className="help-examples">
        {EXAMPLE_PROMPTS.map((p) => (
          <div className="help-example" key={p}>
            "{p}"
          </div>
        ))}
      </div>
      <p className="hint">
        ShadowCode {version} · the model is replaceable; the harness owns tools,
        permissions, and verification.
      </p>
      <div className="row end">
        <button type="button" className="ghost" onClick={onClose}>
          Close
        </button>
      </div>
    </Dialog>
  );
}

export type PaletteItem = {
  id: string;
  label: string;
  hint?: string;
  run: () => void;
};

export function Palette({
  items,
  onClose,
}: {
  items: PaletteItem[];
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [index, setIndex] = useState(0);
  const hits = items.filter((c) =>
    c.label.toLowerCase().includes(q.toLowerCase()),
  );
  useEffect(() => setIndex(0), [q]);
  return (
    <Dialog label="Command palette" className="palette" onClose={onClose}>
      <input
        autoFocus
        value={q}
        onChange={(e) => setQ(e.target.value)}
        aria-label="Search commands"
        placeholder="Type a command…"
        onKeyDown={(ev) => {
          if (ev.key === "ArrowDown") {
            ev.preventDefault();
            setIndex((i) => Math.min(i + 1, hits.length - 1));
          }
          if (ev.key === "ArrowUp") {
            ev.preventDefault();
            setIndex((i) => Math.max(i - 1, 0));
          }
          if (ev.key === "Enter" && hits[index]) {
            ev.preventDefault();
            onClose();
            hits[index].run();
          }
        }}
      />
      <div className="palette-list">
        {hits.map((item, i) => (
          <button
            type="button"
            key={item.id}
            className={`hit ${i === index ? "on" : ""}`}
            onMouseEnter={() => setIndex(i)}
            onClick={() => {
              onClose();
              item.run();
            }}
          >
            <span>{item.label}</span>
            {item.hint && <span className="kbd">{item.hint}</span>}
          </button>
        ))}
        {hits.length === 0 && (
          <div className="palette-empty">No matching commands.</div>
        )}
      </div>
    </Dialog>
  );
}

export function ProjectPicker({
  projects,
  current,
  onClose,
  onPick,
}: {
  projects: Project[];
  current: string;
  onClose: () => void;
  onPick: (path: string) => void;
}) {
  const [path, setPath] = useState(current);
  const [error, setError] = useState("");
  return (
    <Dialog label="Open project" className="modal modal-sm" onClose={onClose}>
      <h2>Open a project</h2>
      <div className="field">
        <label htmlFor="overlays-field-1">Folder path</label>
        <div className="input-action">
          <input
            id="overlays-field-1"
            autoFocus
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="/path/to/project"
            onKeyDown={(ev) => {
              if (ev.key === "Enter") onPick(path);
            }}
          />
          {isNative() && (
            <button
              type="button"
              className="ghost"
              onClick={() =>
                void pickDirectory()
                  .then((path) => {
                    if (path) setPath(path);
                  })
                  .catch((error) => setError(String(error)))
              }
            >
              Browse…
            </button>
          )}
        </div>
        {error && (
          <p role="alert" className="hint danger-text">
            {error}
          </p>
        )}
      </div>
      {projects.length > 0 && (
        <div className="list">
          <div className="list-h">Recent</div>
          {projects.map((p) => (
            <button
              type="button"
              key={p.id}
              className={`item ${p.path === current ? "active" : ""}`}
              onClick={() => onPick(p.path)}
            >
              <strong>{p.name}</strong>
              <span>{p.path}</span>
            </button>
          ))}
        </div>
      )}
      <div className="row end">
        <button type="button" className="ghost" onClick={onClose}>
          Cancel
        </button>
        <button type="button" className="primary" onClick={() => onPick(path)}>
          Open
        </button>
      </div>
    </Dialog>
  );
}

export function TrustDialog({
  req,
  onCancel,
  onConfirm,
}: {
  req: { path: string; name?: string; permissions?: Record<string, unknown> };
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <Dialog label="Trust folder" className="modal modal-sm" onClose={onCancel}>
      <h2>Trust this folder?</h2>
      <p className="hint">
        <code>{req.path}</code> becomes the active workspace at permission level{" "}
        <code>{String(req.permissions?.level || "workspace")}</code>
        {req.permissions?.network ? " with network access" : ", network off"}.
        Dangerous commands still ask first.
      </p>
      <div className="row end">
        <button type="button" className="ghost" onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="primary" onClick={onConfirm}>
          Trust and open
        </button>
      </div>
    </Dialog>
  );
}

/** Free-text model for any provider — used by the composer's "Custom model…" entry. */
export function CustomModelDialog({
  providers,
  onClose,
  onSubmit,
}: {
  providers: ProviderInfo[];
  onClose: () => void;
  onSubmit: (id: string, provider: string, endpoint: string) => Promise<void>;
}) {
  const [id, setId] = useState("");
  const [provider, setProvider] = useState(
    providers.find((p) => p.running)?.id || "openai_compatible",
  );
  const [endpoint, setEndpoint] = useState("");
  const [busy, setBusy] = useState(false);
  const preset = providers.find((p) => p.id === provider);
  return (
    <Dialog label="Custom model" className="modal modal-sm" onClose={onClose}>
      <h2>Use a custom model</h2>
      <p className="hint">
        Any model id your provider accepts. It becomes the default and shows up
        in the picker.
      </p>
      <div className="field">
        <label htmlFor="overlays-field-2">Model id</label>
        <input
          id="overlays-field-2"
          autoFocus
          value={id}
          onChange={(e) => setId(e.target.value)}
          placeholder="gpt-4.1 · grok-4 · llama3.2 · my-finetune"
        />
      </div>
      <div className="field">
        <label htmlFor="overlays-field-3">Provider</label>
        <select
          id="overlays-field-3"
          value={provider}
          onChange={(e) => setProvider(e.target.value)}
        >
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
              {p.running ? " — running" : ""}
            </option>
          ))}
        </select>
      </div>
      <div className="field">
        <label htmlFor="overlays-field-4">
          Endpoint{" "}
          <span className="dim">
            (blank = {preset?.endpoint || "provider default"})
          </span>
        </label>
        <input
          id="overlays-field-4"
          value={endpoint}
          onChange={(e) => setEndpoint(e.target.value)}
          placeholder={preset?.endpoint || "https://host/v1"}
        />
      </div>
      {preset?.needs_key && (
        <p className="hint">
          Needs <code>{preset.api_key_env}</code> — paste the key in Settings →
          Model.
        </p>
      )}
      <div className="row end">
        <button type="button" className="ghost" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="primary"
          disabled={!id.trim() || busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onSubmit(id.trim(), provider, endpoint.trim());
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "Saving…" : "Use model"}
        </button>
      </div>
    </Dialog>
  );
}
