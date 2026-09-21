export function isProjectTrustError(error: unknown): boolean {
  return /Trust this project before starting/i.test(String(error));
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
