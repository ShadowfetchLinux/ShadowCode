import { useEffect, useId, useRef, useState } from "react";
import { Globe, ShieldCheck, WifiOff } from "lucide-react";

export type PermissionMode = "ask" | "allow_edits";

const MODES: { id: PermissionMode; label: string; short: string; hint: string }[] =
  [
    {
      id: "ask",
      label: "Ask before actions",
      short: "Ask first",
      hint: "File edits and shell commands wait for your approval.",
    },
    {
      id: "allow_edits",
      label: "Allow project edits",
      short: "Edits allowed",
      hint: "File edits inside this project run without asking. Shell commands still ask.",
    },
  ];

/** Compact permission control bound to config permissions.mode. */
export function PermissionControl({
  mode,
  readOnly,
  vendorNote,
  onChange,
  onOpenSettings,
}: {
  mode: PermissionMode;
  /** Advanced read-only level set in Settings. */
  readOnly: boolean;
  /** How the selected vendor runtime maps these modes (permissions.vendor_notes). */
  vendorNote?: string;
  onChange: (mode: PermissionMode) => void;
  onOpenSettings: () => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const uid = useId();
  const current = MODES.find((m) => m.id === mode) || MODES[0];
  useEffect(() => {
    if (!open) return;
    const onDoc = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);
  const label = readOnly ? "Read only" : current.short;
  return (
    <div
      className="composer-control"
      ref={root}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          setOpen(false);
          trigger.current?.focus();
        }
      }}
    >
      <button
        ref={trigger}
        type="button"
        className="composer-chip"
        aria-expanded={open}
        aria-controls={`${uid}-menu`}
        aria-label={`Permissions: ${readOnly ? "read only" : current.label}`}
        title={vendorNote || (readOnly ? "This project is read only" : current.hint)}
        onClick={() => setOpen((v) => !v)}
      >
        <ShieldCheck size={14} aria-hidden="true" />
        <span>{label}</span>
      </button>
      {open && (
        <div className="composer-popover" id={`${uid}-menu`}>
          {readOnly ? (
            <p className="hint">
              This project is read only. Change it in Settings › Permissions &
              network.
            </p>
          ) : (
            <fieldset className="mode-options">
              <legend>Permissions</legend>
              {MODES.map((m) => (
                <label key={m.id} className="mode-option">
                  <input
                    type="radio"
                    name={`${uid}-mode`}
                    checked={mode === m.id}
                    onChange={() => {
                      onChange(m.id);
                      setOpen(false);
                      trigger.current?.focus();
                    }}
                  />
                  <span>
                    <strong>{m.label}</strong>
                    <small>{m.hint}</small>
                  </span>
                </label>
              ))}
            </fieldset>
          )}
          {vendorNote && <p className="hint">{vendorNote}</p>}
          <button
            type="button"
            className="link"
            onClick={() => {
              setOpen(false);
              onOpenSettings();
            }}
          >
            Permission settings…
          </button>
        </div>
      )}
    </div>
  );
}

/** Web lookups for the next task. Shown only for local rows; vendor CLIs bring
 * their own web tools. */
export function WebToggle({
  enabled,
  onChange,
}: {
  enabled: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      className={`composer-chip ${enabled ? "on" : ""}`}
      aria-pressed={enabled}
      title={
        enabled
          ? "The local model may fetch web pages and search the web for this task"
          : "Let the local model fetch web pages for this task"
      }
      onClick={() => onChange(!enabled)}
    >
      <Globe size={14} aria-hidden="true" />
      <span>Web</span>
    </button>
  );
}

export function NetworkPill({ mode }: { mode: "offline" | "web_off" }) {
  return (
    <span
      className="composer-pill"
      title={
        mode === "offline"
          ? "Offline mode: cloud models, web tools and usage refresh are off"
          : "Web tools are turned off in Settings"
      }
    >
      <WifiOff size={13} aria-hidden="true" />
      {mode === "offline" ? "Offline" : "Web off"}
    </span>
  );
}
