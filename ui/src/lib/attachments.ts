/** Composer attachment rules. Images are accepted only when the selected row
 * reports vision === true; the check runs when attaching and again at send. */

export type Attachment = {
  path: string;
  name: string;
  kind: "image" | "text";
  preview?: string;
};

export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp"];
export const IMAGE_EXT = /\.(png|jpe?g|webp)$/i;
export const MAX_IMAGE_BYTES = 4_000_000;
export const MAX_TEXT_BYTES = 1_000_000;
export const MAX_IMAGES = 6;

export const TEXT_ACCEPT =
  "text/*,.md,.json,.ts,.tsx,.js,.jsx,.py,.rs,.toml,.yaml,.yml,.css,.html,.svg,.go,.java,.c,.h,.cpp,.sh";
export const IMAGE_ACCEPT = ".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp";

export function isImageFile(file: { name: string; type: string }): boolean {
  return file.type.startsWith("image/") || IMAGE_EXT.test(file.name);
}

export type Check =
  | { ok: true; kind: "image" | "text" }
  | { ok: false; error: string };

export function checkAttachment(
  file: { name: string; type: string; size: number },
  options: { vision: boolean; images: number; modelName?: string },
): Check {
  if (isImageFile(file)) {
    if (!options.vision)
      return {
        ok: false,
        error: `${file.name}: ${options.modelName ? `${options.modelName} does not accept images` : "choose a model marked Vision to attach images"}.`,
      };
    if (
      !IMAGE_TYPES.includes(file.type) &&
      !(file.type === "" && IMAGE_EXT.test(file.name))
    )
      return { ok: false, error: `${file.name}: use PNG, JPEG, or WebP.` };
    if (file.size > MAX_IMAGE_BYTES)
      return { ok: false, error: `${file.name}: images must be smaller than 4 MB.` };
    if (options.images >= MAX_IMAGES)
      return { ok: false, error: `Attach at most ${MAX_IMAGES} images per message.` };
    return { ok: true, kind: "image" };
  }
  if (file.size > MAX_TEXT_BYTES)
    return {
      ok: false,
      error: `${file.name}: text attachments must be smaller than 1 MB.`,
    };
  if (
    file.type &&
    !file.type.startsWith("text/") &&
    !/json|javascript|typescript|xml|yaml|toml|x-sh/.test(file.type)
  )
    return { ok: false, error: `${file.name}: attach a text, source, or image file.` };
  return { ok: true, kind: "text" };
}

/** Image files from a paste event (screenshots arrive as clipboard items). */
export function pastedImages(data: DataTransfer | null): File[] {
  if (!data) return [];
  const files: File[] = [];
  for (const item of Array.from(data.items || [])) {
    if (item.kind !== "file" || !item.type.startsWith("image/")) continue;
    const file = item.getAsFile();
    if (!file) continue;
    const ext = file.type.split("/")[1]?.replace("jpeg", "jpg") || "png";
    const name =
      file.name && file.name !== "image.png"
        ? file.name
        : `pasted-${new Date().toISOString().replace(/[:.]/g, "-")}.${ext}`;
    files.push(new File([file], name, { type: file.type }));
  }
  return files;
}

/** Re-check at send time: the row may have changed since the image was added. */
export function sendBlockedByImages(
  attachments: Attachment[],
  vision: boolean,
  modelName?: string,
): string | null {
  if (vision || !attachments.some((a) => a.kind === "image")) return null;
  return `${modelName || "The selected model"} does not accept images. Remove the image${attachments.filter((a) => a.kind === "image").length > 1 ? "s" : ""} or choose a model marked Vision.`;
}

export async function toBase64(file: Blob): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk)
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(binary);
}
