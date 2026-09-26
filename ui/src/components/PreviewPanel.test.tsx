import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { api } from "../api";
import { emptyDrawerMemory } from "../hooks/useDrawerMemory";
import { pendingAttachments } from "../lib/pendingAttachments";
import * as transport from "../lib/transport";
import { PreviewPanel } from "./PreviewPanel";

const PROXY = "http://127.0.0.1:41234";

vi.mock("../lib/transport", async (original) => ({
  ...(await original<typeof import("../lib/transport")>()),
  isRemote: vi.fn(() => false),
}));

beforeEach(() => {
  vi.spyOn(api, "previewServers").mockResolvedValue({
    workspace: "/work/demo",
    servers: [
      {
        port: 5173,
        url: "http://localhost:5173/",
        source: "process",
        listening: true,
        pid: 1,
        process: "node",
        command: "node vite",
        background_id: null,
        background_name: null,
      },
    ],
  });
  vi.spyOn(api, "openPreview").mockImplementation(async (url) => ({
    proxy_origin: PROXY,
    proxy_port: 41234,
    target_origin: "http://localhost:5173",
    // The frame's page is not loaded here (happy-dom would fetch it).
    url: "about:blank",
    target_url: url,
  }));
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.mocked(transport.isRemote).mockReturnValue(false);
  pendingAttachments.clear();
});

function renderPanel() {
  const toast = vi.fn();
  render(
    <PreviewPanel
      workspace="/work/demo"
      toast={toast}
      memory={emptyDrawerMemory()}
      onMemory={() => {}}
    />,
  );
  return toast;
}

/** A message as the picker in the frame would post it. */
function fromFrame(data: Record<string, unknown>, origin = PROXY) {
  const frame = document.querySelector("iframe") as HTMLIFrameElement;
  act(() => {
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { source: "shadowcode-preview", version: 1, ...data },
        origin,
        source: frame.contentWindow,
      }),
    );
  });
}

const element = {
  selector: "button.primary",
  tag: "button",
  role: "button",
  name: "",
  text: "Save",
  attributes: { class: "primary" },
  outer_html: '<button class="primary">Save</button>',
  styles: {},
  box: { x: 1, y: 2, width: 60, height: 30 },
  ancestors: [],
  url: `${PROXY}/`,
  title: "Demo",
  viewport: { width: 1280, height: 800, dpr: 1 },
};

it("over remote access explains that the preview is local", () => {
  vi.mocked(transport.isRemote).mockReturnValue(true);
  renderPanel();
  expect(screen.getByText(/works in the ShadowCode window/)).toBeTruthy();
  expect(api.previewServers).not.toHaveBeenCalled();
});

it("opens a detected server and only trusts its own frame", async () => {
  const toast = renderPanel();
  fireEvent.click(
    await screen.findByRole("button", { name: /localhost:5173/ }),
  );
  await waitFor(() =>
    expect(api.openPreview).toHaveBeenCalledWith(
      "http://localhost:5173/",
      window.location.origin,
    ),
  );
  const frame = await waitFor(() => {
    const found = document.querySelector("iframe");
    if (!found) throw new Error("no frame yet");
    return found;
  });
  expect(frame.getAttribute("src")).toBe("about:blank");
  expect(frame.getAttribute("sandbox")).not.toContain("allow-top-navigation");
  const pick = () =>
    screen.getByRole("button", { name: /Pick element|Picking/ });
  expect((pick() as HTMLButtonElement).disabled).toBe(true);

  // Another origin (or the dev server itself) is not the picker.
  fromFrame(
    { type: "ready", url: `${PROXY}/`, title: "x" },
    "http://localhost:5173",
  );
  expect((pick() as HTMLButtonElement).disabled).toBe(true);
  fromFrame({ type: "ready", url: `${PROXY}/next`, title: "Demo" });
  expect((pick() as HTMLButtonElement).disabled).toBe(false);
  expect(
    (
      screen.getByRole("textbox", {
        name: "Preview address",
      }) as HTMLInputElement
    ).value,
  ).toBe("http://localhost:5173/next");

  // A pick nobody asked for (a page script can post one) is ignored.
  fromFrame({ type: "picked", element });
  expect(pendingAttachments.list()).toEqual([]);
  fireEvent.click(pick());
  expect(pick().getAttribute("aria-pressed")).toBe("true");
  fromFrame({ type: "picked", element });
  expect(pendingAttachments.list().map((i) => i.label)).toEqual([
    'button "Save"',
  ]);
  expect(pendingAttachments.list()[0].text).toContain(
    "Element on http://localhost:5173/ (“Demo”):",
  );
  expect(toast).toHaveBeenCalledWith(
    'Added button "Save" to your next message',
    "ok",
  );

  // Console messages show in the strip; attaching adds a chip.
  fromFrame({
    type: "console",
    entries: [
      { level: "error", message: "boom", source: "", url: "", time: 1 },
    ],
  });
  fireEvent.click(screen.getByRole("button", { name: "Attach: boom" }));
  expect(pendingAttachments.list().map((i) => i.kind)).toEqual([
    "element",
    "console",
  ]);
});

it("refuses addresses on other computers before asking the engine", async () => {
  renderPanel();
  const address = screen.getByRole("textbox", { name: "Preview address" });
  fireEvent.change(address, { target: { value: "http://example.com:8080" } });
  fireEvent.submit(address.closest("form")!);
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(api.openPreview).not.toHaveBeenCalled();
});
