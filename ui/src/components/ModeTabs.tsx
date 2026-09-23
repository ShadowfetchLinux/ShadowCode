import { Code2, Map, Search, TestTube2 } from "lucide-react";

export const MODES = [
  {
    id: "coder",
    label: "Build",
    icon: Code2,
    description: "Write and edit code, run commands, apply fixes",
  },
  {
    id: "planner",
    label: "Plan",
    icon: Map,
    description: "Design architecture and milestones without changing files",
  },
  {
    id: "reviewer",
    label: "Review",
    icon: Search,
    description: "Read code, explain behaviour, and find issues",
  },
  {
    id: "tester",
    label: "Test",
    icon: TestTube2,
    description: "Write tests, run them, and verify coverage",
  },
] as const;

export function ModeTabs({
  value,
  onChange,
  disabled,
}: {
  value: string;
  onChange: (mode: string) => void;
  disabled?: boolean;
}) {
  return (
    <div className="mode-tabs" role="tablist" aria-label="Agent mode">
      {MODES.map((m) => (
        <button
          key={m.id}
          type="button"
          role="tab"
          aria-selected={value === m.id}
          aria-label={`${m.label}: ${m.description}`}
          title={m.description}
          className={`mode-tab${value === m.id ? " active" : ""}`}
          disabled={disabled}
          onClick={() => onChange(m.id)}
        >
          <m.icon size={14} />
          <span>{m.label}</span>
        </button>
      ))}
    </div>
  );
}
