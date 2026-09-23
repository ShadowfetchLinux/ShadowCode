import { chipLabel, chipTone, type VendorStatusMap } from "../lib/cliAgents";

const VENDORS: { id: string; label: string }[] = [
  { id: "codex", label: "Codex" },
  { id: "grok", label: "Grok" },
  { id: "claude", label: "Claude" },
];

export function VendorAgentChip({
  status,
  selected,
}: {
  status: VendorStatusMap | null;
  selected?: string;
}) {
  const selectedId = (selected || "").replace(/^cli:/, "");
  return (
    <div
      className="vendor-agent-chip"
      data-testid="vendor-agent-chip"
      title="Official CLI login stays with Claude, Codex, or Grok. ShadowCode never reads those credentials."
    >
      {VENDORS.map((vendor) => {
        const state = status?.[vendor.id];
        const tone = chipTone(state);
        const active = selectedId === vendor.id;
        return (
          <span
            key={vendor.id}
            className={`vendor-chip vendor-chip-${tone}${active ? " selected" : ""}`}
            data-vendor={vendor.id}
            data-state={state?.state || "unknown"}
            title={state?.detail || `${vendor.label} vendor agent`}
          >
            {vendor.label} · {chipLabel(state)}
          </span>
        );
      })}
    </div>
  );
}
