import { ConfirmDialog } from "./ConfirmDialog";

/** Confirms a rewind and names every file it changes. */
export function RewindDialog({
  paths,
  onConfirm,
  onCancel,
}: {
  paths: string[];
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
}) {
  const shown = paths.slice(0, 12);
  return (
    <ConfirmDialog
      title="Rewind this task's changes?"
      confirmLabel={`Rewind ${paths.length} file${paths.length === 1 ? "" : "s"}`}
      danger
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <p>
        These files go back to how they were before the task. Files it created
        are removed. You can undo the rewind right after.
      </p>
      <ul className="rewind-files" aria-label="Files that will change">
        {shown.map((path) => (
          <li key={path}>
            <code>{path}</code>
          </li>
        ))}
        {paths.length > shown.length && (
          <li className="dim">and {paths.length - shown.length} more</li>
        )}
      </ul>
      <p className="hint">
        The conversation stays as it is; a divider marks the rewind.
      </p>
    </ConfirmDialog>
  );
}
