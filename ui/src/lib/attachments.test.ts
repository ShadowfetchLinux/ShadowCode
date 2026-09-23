import { afterEach, expect, it, vi } from "vitest";
import {
  checkAttachment,
  pastedImages,
  sendBlockedByImages,
  type Attachment,
} from "./attachments";
import { api } from "../api";
import { installFakeBackend } from "../../e2e/fakeBackend";

afterEach(() => {
  vi.unstubAllEnvs();
  delete window.__SHADOW_TEST_TRANSPORT__;
});

const png = { name: "shot.png", type: "image/png", size: 1000 };

it("accepts images only for vision rows, with type, size and count limits", () => {
  expect(
    checkAttachment(png, { vision: false, images: 0, modelName: "qwen3:14b" }),
  ).toEqual({
    ok: false,
    error: "shot.png: qwen3:14b does not accept images.",
  });
  expect(checkAttachment(png, { vision: true, images: 0 })).toEqual({
    ok: true,
    kind: "image",
  });
  expect(
    checkAttachment(
      { ...png, name: "a.gif", type: "image/gif" },
      { vision: true, images: 0 },
    ),
  ).toMatchObject({
    ok: false,
    error: expect.stringContaining("PNG, JPEG, or WebP"),
  });
  expect(
    checkAttachment({ ...png, size: 5_000_000 }, { vision: true, images: 0 }),
  ).toMatchObject({ ok: false, error: expect.stringContaining("4 MB") });
  expect(checkAttachment(png, { vision: true, images: 6 })).toMatchObject({
    ok: false,
  });
  expect(
    checkAttachment(
      { name: "notes.md", type: "text/markdown", size: 10 },
      { vision: false, images: 0 },
    ),
  ).toEqual({ ok: true, kind: "text" });
  expect(
    checkAttachment(
      { name: "a.bin", type: "application/octet-stream", size: 10 },
      { vision: false, images: 0 },
    ),
  ).toMatchObject({ ok: false });
});

it("re-checks images at send time after the row changed", () => {
  const items: Attachment[] = [
    { path: ".shadow/a.png", name: "a.png", kind: "image" },
    { path: ".shadow/n.md", name: "n.md", kind: "text" },
  ];
  expect(sendBlockedByImages(items, true)).toBeNull();
  expect(sendBlockedByImages(items, false, "Grok · Default")).toBe(
    "Grok · Default does not accept images. Remove the image or choose a model marked Vision.",
  );
  expect(sendBlockedByImages([items[1]], false)).toBeNull();
});

it("reads pasted image data from clipboard items", () => {
  const file = new File([new Uint8Array([1, 2, 3])], "image.png", {
    type: "image/png",
  });
  const data = {
    items: [
      { kind: "string", type: "text/plain", getAsFile: () => null },
      { kind: "file", type: "image/png", getAsFile: () => file },
    ],
  } as unknown as DataTransfer;
  const files = pastedImages(data);
  expect(files).toHaveLength(1);
  expect(files[0].name).toMatch(/^pasted-.*\.png$/);
  expect(pastedImages(null)).toEqual([]);
});

it("turns a needs_consent answer into a consent request, from either transport form", async () => {
  vi.stubEnv("VITE_SHADOW_TEST_TRANSPORT", "1");
  const fake = installFakeBackend();
  // IPC error string carrying the JSON body (the fake's default).
  fake.state.lastRoute.s1 = "local";
  const first = await api.startJob({
    task: "x",
    session_id: "s1",
    model: "cli:codex:gpt-6-astra",
  });
  expect("consent" in first && first.consent.handoff.excerpt_chars).toBe(2400);
  // A successful IPC value with needs_consent.
  window.__SHADOW_TEST_TRANSPORT__ = {
    ...fake.bridge,
    request: async () => ({
      needs_consent: true,
      handoff: { to: "cli:cursor", images: 1 },
    }),
  };
  const second = await api.startJob({ task: "x", model: "cli:cursor:auto" });
  expect("consent" in second && second.consent.handoff.images).toBe(1);
  // Nothing was created by the refused request.
  expect(fake.state.jobs).toHaveLength(0);
});
