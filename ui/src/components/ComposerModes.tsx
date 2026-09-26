import { useId } from "react";
import { Brain } from "lucide-react";
import { EFFORTS, MODES, type Effort, type TaskMode } from "../lib/effort";

/** Code / Plan / Ask for the next message. Plan and Ask are read-only. */
export function ModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: TaskMode;
  onChange: (mode: TaskMode) => void;
  disabled?: boolean;
}) {
  return (
    <div
      className="seg composer-modes"
      role="radiogroup"
      aria-label="Task mode"
    >
      {MODES.map((m) => (
        <button
          key={m.id}
          type="button"
          role="radio"
          aria-checked={mode === m.id}
          className={mode === m.id ? "on" : ""}
          title={m.hint}
          disabled={disabled}
          onClick={() => onChange(m.id)}
        >
          {m.label}
        </button>
      ))}
    </div>
  );
}

/** Reasoning effort next to the model pill; shown only for rows whose
 * runtime has an effort control, remembered per model. */
export function EffortControl({
  effort,
  onChange,
  disabled,
}: {
  effort: Effort;
  onChange: (effort: Effort) => void;
  disabled?: boolean;
}) {
  const id = useId();
  const current = EFFORTS.find((e) => e.id === effort) || EFFORTS[0];
  return (
    <label
      className="composer-chip effort-control"
      htmlFor={id}
      title={`Reasoning effort: ${current.hint}`}
    >
      <Brain size={14} aria-hidden="true" />
      <span className="sr-only">Reasoning effort</span>
      <select
        id={id}
        value={effort}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as Effort)}
      >
        {EFFORTS.map((e) => (
          <option key={e.id} value={e.id}>
            {e.id === "default" ? "Effort: default" : `Effort: ${e.label}`}
          </option>
        ))}
      </select>
    </label>
  );
}
