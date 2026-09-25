import { useState } from "react";
import { api } from "../api";
import { checkAttachment, toBase64, type Attachment } from "../lib/attachments";
import type { ToastKind } from "./useToasts";

/** Files attached to the next message. Images go only to vision rows; text
 * files must be text (no NUL bytes). */
export function useAttachments({
  locked,
  vision,
  modelName,
  toast,
}: {
  locked: boolean;
  vision: boolean;
  modelName?: string;
  toast: (text: string, kind?: ToastKind) => void;
}) {
  const [attachments, setAttachments] = useState<Attachment[]>([]);

  async function attach(files: File[]) {
    if (locked) return;
    let images = attachments.filter((a) => a.kind === "image").length;
    for (const file of files) {
      const check = checkAttachment(file, { vision, images, modelName });
      if (!check.ok) {
        toast(check.error, "err");
        continue;
      }
      try {
        if (check.kind === "image") {
          images += 1;
          const saved = await api.attachImage(file.name, await toBase64(file));
          const preview = URL.createObjectURL(file);
          setAttachments((prev) =>
            prev.some((a) => a.path === saved.path)
              ? prev
              : [
                  ...prev,
                  { path: saved.path, name: file.name, kind: "image", preview },
                ],
          );
        } else {
          const text = await file.text();
          if (text.includes("\0")) {
            toast(`${file.name}: attach a text, source, or image file.`, "err");
            continue;
          }
          const saved = await api.attach(file.name, text);
          setAttachments((prev) =>
            prev.some((a) => a.path === saved.path)
              ? prev
              : [...prev, { path: saved.path, name: file.name, kind: "text" }],
          );
        }
      } catch (e) {
        toast(String(e), "err");
      }
    }
  }

  function remove(path: string) {
    setAttachments((prev) => {
      const gone = prev.find((a) => a.path === path);
      if (gone?.preview) URL.revokeObjectURL(gone.preview);
      return prev.filter((a) => a.path !== path);
    });
  }

  return { attachments, setAttachments, attach, remove };
}
