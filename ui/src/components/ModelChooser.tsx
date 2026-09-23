import type { ModelInfo } from "../api";
import { modelLabel } from "../lib/models";
import {
  groupModels,
  LOCAL_GROUP,
  VENDOR_GROUP,
} from "../lib/cliAgents";

export function ModelChooser({
  models,
  value,
  automaticLabel,
  onChange,
}: {
  models: ModelInfo[];
  value: string;
  automaticLabel: string;
  onChange: (id: string) => void;
}) {
  const groups = groupModels(models);
  return (
    <select
      className="model-select"
      aria-label="Model for this task"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      <option value="">{automaticLabel}</option>
      {groups.local.length > 0 && (
        <optgroup label={LOCAL_GROUP}>
          {groups.local.map((model) => (
            <option key={model.id} value={model.id}>
              {modelLabel(model, models)}
              {model.detected ? " · local" : ""}
            </option>
          ))}
        </optgroup>
      )}
      {groups.other.length > 0 && (
        <optgroup label="Remote API (ShadowCode agent)">
          {groups.other.map((model) => (
            <option key={model.id} value={model.id}>
              {modelLabel(model, models)}
            </option>
          ))}
        </optgroup>
      )}
      {groups.vendor.length > 0 && (
        <optgroup label={VENDOR_GROUP}>
          {groups.vendor.map((model) => (
            <option key={model.id} value={model.id}>
              {model.metadata?.label || model.name || model.id}
            </option>
          ))}
        </optgroup>
      )}
      <option value="__custom__">Custom model…</option>
    </select>
  );
}
