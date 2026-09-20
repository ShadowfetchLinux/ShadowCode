import type { ModelInfo } from "../api";

/** Keep model names readable while distinguishing identical names on different
 * hosts or with separately configured credentials. Never show URL credentials. */
export function modelLabel(model: ModelInfo, catalog: ModelInfo[]): string {
  const name = model.name || model.id;
  if (
    !catalog.some(
      (other) =>
        other.id !== model.id &&
        other.name === model.name &&
        other.provider === model.provider,
    )
  )
    return name;
  let endpoint = model.provider;
  try {
    const url = new URL(model.endpoint);
    endpoint = url.host + url.pathname.replace(/\/$/, "");
  } catch {
    /* Invalid legacy endpoints still have a provider label. */
  }
  const alias =
    model.id !== name && !model.id.startsWith("model:") ? ` · ${model.id}` : "";
  return `${name} · ${endpoint}${alias}`;
}
