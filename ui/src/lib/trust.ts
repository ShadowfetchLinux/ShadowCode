export function isProjectTrustError(error: unknown): boolean {
  return /Trust this project before starting/i.test(String(error));
}

export function trustErrorHint(error: unknown): string | undefined {
  if (!isProjectTrustError(error)) return undefined;
  return "This folder is not trusted. Click Trust this folder, then Trust and open, and send the task again.";
}

export function sameWorkspacePath(
  left?: string | null,
  right?: string | null,
): boolean {
  if (!left || !right) return false;
  const slim = (path: string) =>
    path.trim().replace(/\\/g, "/").replace(/\/+$/, "");
  return slim(left) === slim(right) && slim(left).length > 0;
}

export function trustRequestFor(
  path: string,
  permissions?: Record<string, unknown>,
): { path: string; name?: string; permissions?: Record<string, unknown> } {
  const workspace = path.trim();
  return {
    path: workspace,
    name: workspace.split(/[\\/]/).filter(Boolean).at(-1),
    permissions,
  };
}

export function trustPromptFor(
  workspace: string | undefined,
  trusted: boolean | undefined,
  permissions?: Record<string, unknown>,
): ReturnType<typeof trustRequestFor> | null {
  const folder = workspace?.trim();
  if (!folder || trusted !== false) return null;
  return trustRequestFor(folder, permissions);
}
