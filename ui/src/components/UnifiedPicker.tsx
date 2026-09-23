import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown, Plus, Search } from "lucide-react";
import {
  availabilityLabel,
  groupTargets,
  isSelectable,
  usageLabel,
  type PickerTarget,
} from "../lib/picker";

export function UnifiedPicker({
  targets,
  value,
  automaticLabel,
  onChange,
  onConnect,
  onAddLocal,
  disabled,
}: {
  targets: PickerTarget[];
  value: string;
  automaticLabel: string;
  onChange: (id: string) => void;
  onConnect?: () => void;
  onAddLocal?: () => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const selected = targets.find((t) => t.id === value);
  const groups = useMemo(() => {
    const all = groupTargets(targets);
    const match = (t: PickerTarget) =>
      `${t.name} ${t.subtitle || ""} ${t.provider}`.toLowerCase().includes(query.toLowerCase());
    return {
      subscriptions: all.subscriptions.filter(match),
      local: all.local.filter(match),
    };
  }, [targets, query]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  function pick(id: string) {
    onChange(id);
    setOpen(false);
    setQuery("");
  }

  return (
    <div className="unified-picker" ref={root}>
      <button
        type="button"
        className="unified-picker-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label="Model for this task"
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="unified-picker-current">
          {selected?.name || automaticLabel}
        </span>
        <ChevronDown size={14} />
      </button>
      {open && (
        <div className="unified-picker-menu" role="listbox" aria-label="Select an agent or model">
          <label className="unified-picker-search">
            <Search size={14} />
            <input
              autoFocus
              value={query}
              placeholder="Search models…"
              aria-label="Search models"
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          <Section title="Subscriptions (use your account)">
            {groups.subscriptions.length ? (
              groups.subscriptions.map((t) => (
                <Row
                  key={t.id}
                  target={t}
                  selected={t.id === value}
                  onPick={() => pick(t.id)}
                />
              ))
            ) : (
              <div className="unified-picker-empty">No subscription matches</div>
            )}
          </Section>
          <Section title="On this computer">
            {groups.local.length ? (
              groups.local.map((t) => (
                <Row
                  key={t.id}
                  target={t}
                  selected={t.id === value}
                  onPick={() => pick(t.id)}
                />
              ))
            ) : (
              <div className="unified-picker-empty">No local models yet</div>
            )}
          </Section>
          <div className="unified-picker-actions">
            <button type="button" onClick={() => { setOpen(false); onConnect?.(); }}>
              <Plus size={14} /> Connect account…
            </button>
            <button type="button" onClick={() => { setOpen(false); onAddLocal?.(); }}>
              <Plus size={14} /> Add local model…
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="unified-picker-group" role="group" aria-label={title}>
      <div className="unified-picker-heading">{title}</div>
      {children}
    </div>
  );
}

function Row({
  target,
  selected,
  onPick,
}: {
  target: PickerTarget;
  selected: boolean;
  onPick: () => void;
}) {
  const ready = isSelectable(target);
  const usage = usageLabel(target.usage, target.inference);
  const title = [
    target.reason,
    target.usage?.window ? `Window: ${target.usage.window}` : "",
    target.usage?.reset ? `Reset: ${target.usage.reset}` : "",
    target.usage?.shared_pool ? `Shared pool: ${target.usage.shared_pool}` : "",
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      className={`unified-picker-row ${ready ? "" : "is-blocked"} ${selected ? "is-selected" : ""}`}
      disabled={!ready}
      title={title}
      onClick={onPick}
    >
      <span className="unified-picker-copy">
        <strong>{target.name}</strong>
        <small>
          {target.inference === "local" ? "Local" : "Cloud"} · {availabilityLabel(target)}
          {target.vision ? " · Vision" : ""}
          {target.tools === false ? " · Chat only" : ""}
        </small>
      </span>
      <span className="unified-picker-meta">
        {ready ? usage : availabilityLabel(target)}
        {selected ? <Check size={14} /> : null}
      </span>
    </button>
  );
}
