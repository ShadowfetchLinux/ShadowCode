import { useState } from "react";
import { SlidersHorizontal } from "lucide-react";

export function ToolsControl({
  webEnabled,
  onWebEnabled,
  permissionLabel,
  onOpenSettings,
}: {
  webEnabled: boolean;
  onWebEnabled: (next: boolean) => void;
  permissionLabel: string;
  onOpenSettings: () => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="tools-control">
      <button
        type="button"
        className="tools-control-trigger"
        aria-expanded={open}
        aria-haspopup="true"
        aria-label="Tools and permissions"
        title="Tools and permissions"
        onClick={() => setOpen((v) => !v)}
      >
        <SlidersHorizontal size={15} />
        Tools
      </button>
      {open && (
        <div className="tools-control-menu" role="dialog" aria-label="Tools and permissions">
          <p className="tools-control-note">
            {permissionLabel}. These settings stay honest: shell can still do
            what the selected runtime allows.
          </p>
          <label className="tools-control-row">
            <input
              type="checkbox"
              checked={webEnabled}
              onChange={(e) => onWebEnabled(e.target.checked)}
            />
            Web lookup
          </label>
          <button
            type="button"
            className="ghost"
            onClick={() => {
              setOpen(false);
              onOpenSettings();
            }}
          >
            Advanced settings
          </button>
        </div>
      )}
    </div>
  );
}
