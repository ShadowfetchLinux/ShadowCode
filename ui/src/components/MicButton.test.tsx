import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { voiceApi, type VoiceRecording } from "../lib/voice";
import { MicButton, isVoiceShortcut, micLabel } from "./MicButton";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function Harness({
  initial = "",
  onError = () => undefined,
  onOpenSettings = () => undefined,
}: {
  initial?: string;
  onError?: (message: string) => void;
  onOpenSettings?: () => void;
}) {
  const [task, setTask] = useState(initial);
  const ref = useRef<HTMLTextAreaElement>(null);
  return (
    <>
      <textarea
        aria-label="Message"
        ref={ref}
        value={task}
        onChange={(e) => setTask(e.target.value)}
      />
      <MicButton
        task={task}
        onTask={setTask}
        promptRef={ref}
        onError={onError}
        onOpenSettings={onOpenSettings}
      />
    </>
  );
}

function mockEngine(recording: Partial<VoiceRecording> = {}, text = "") {
  return {
    start: vi
      .spyOn(voiceApi, "start")
      .mockResolvedValue({ ok: true, engine: "local", device: "Mic" }),
    recording: vi.spyOn(voiceApi, "recording").mockResolvedValue({
      active: true,
      level: 0.6,
      seconds: 2,
      ...recording,
    }),
    stop: vi.spyOn(voiceApi, "stop").mockResolvedValue({
      text,
      engine: "local",
      seconds: 2,
      ms: 400,
      message: text ? null : "No speech was recognised.",
    }),
    cancel: vi
      .spyOn(voiceApi, "cancel")
      .mockResolvedValue({ ok: true, cancelled: true }),
  };
}

const mic = () =>
  screen.getAllByRole("button").find((b) => b.classList.contains("mic-btn"))!;
const field = () => screen.getByLabelText("Message") as HTMLTextAreaElement;

describe("MicButton", () => {
  it("listens, shows level and live text, inserts at the cursor and can undo", async () => {
    const engine = mockEngine({ partial: "add a" }, "add a test");
    render(<Harness initial="Please now" />);
    field().setSelectionRange(6, 6);
    expect(mic().getAttribute("aria-label")).toBe(micLabel("idle"));
    expect(mic().getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(mic());
    await screen.findByText(/Listening 0:02/);
    expect(engine.start).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("add a")).toBeTruthy();
    expect(mic().getAttribute("aria-pressed")).toBe("true");
    expect(mic().getAttribute("aria-label")).toBe(micLabel("listening"));
    expect(mic().querySelectorAll(".voice-meter span")).toHaveLength(5);

    fireEvent.click(mic());
    await waitFor(() => expect(field().value).toBe("Please add a test now"));
    expect(engine.stop).toHaveBeenCalledTimes(1);
    expect(mic().getAttribute("aria-pressed")).toBe("false");
    // Not sent, not moved elsewhere: only the composer changed. Undo puts
    // back what was there.
    fireEvent.click(screen.getByRole("button", { name: /Undo/ }));
    expect(field().value).toBe("Please now");
    expect(screen.queryByRole("button", { name: /Undo/ })).toBeNull();
  });

  it("offers undo only until the text is edited", async () => {
    mockEngine({}, "hello");
    render(<Harness />);
    fireEvent.click(mic());
    await screen.findByText(/Listening/);
    fireEvent.click(mic());
    await waitFor(() => expect(field().value).toBe("hello"));
    expect(screen.getByRole("button", { name: /Undo/ })).toBeTruthy();
    fireEvent.change(field(), { target: { value: "hello there" } });
    expect(screen.queryByRole("button", { name: /Undo/ })).toBeNull();
  });

  it("push-to-talk: a long press stops on release, a tap keeps listening", async () => {
    const engine = mockEngine({}, "held");
    const now = vi.spyOn(Date, "now").mockReturnValue(1_000);
    render(<Harness />);
    fireEvent.pointerDown(mic(), { button: 0 });
    await screen.findByText(/Listening/);
    now.mockReturnValue(1_100);
    fireEvent.pointerUp(mic());
    // A 100 ms tap: still listening.
    expect(engine.stop).not.toHaveBeenCalled();
    expect(screen.getByText(/Listening/)).toBeTruthy();
    // Tap again to stop.
    fireEvent.pointerDown(mic(), { button: 0 });
    await waitFor(() => expect(field().value).toBe("held"));

    // Now hold for 600 ms.
    now.mockReturnValue(5_000);
    fireEvent.pointerDown(mic(), { button: 0 });
    await screen.findByText(/Listening/);
    now.mockReturnValue(5_600);
    fireEvent.pointerUp(mic());
    await waitFor(() => expect(engine.stop).toHaveBeenCalledTimes(2));
  });

  it("sends a missing model to Settings › Voice", async () => {
    vi.spyOn(voiceApi, "start").mockRejectedValue(
      new Error(
        "No voice model is installed. Open Settings › Voice and install Whisper base (English) (147 MB).",
      ),
    );
    const onError = vi.fn();
    const onOpenSettings = vi.fn();
    render(<Harness onError={onError} onOpenSettings={onOpenSettings} />);
    fireEvent.click(mic());
    await waitFor(() => expect(onOpenSettings).toHaveBeenCalledTimes(1));
    expect(onError.mock.calls[0][0]).toContain("No voice model is installed");
    expect(mic().getAttribute("aria-pressed")).toBe("false");
  });

  it("other start errors only show a message", async () => {
    vi.spyOn(voiceApi, "start").mockRejectedValue(
      new Error("No microphone found."),
    );
    const onError = vi.fn();
    const onOpenSettings = vi.fn();
    render(<Harness onError={onError} onOpenSettings={onOpenSettings} />);
    fireEvent.click(mic());
    await waitFor(() =>
      expect(onError).toHaveBeenCalledWith("No microphone found."),
    );
    expect(onOpenSettings).not.toHaveBeenCalled();
  });

  it("Escape and the Cancel button throw the recording away", async () => {
    const engine = mockEngine();
    render(<Harness initial="keep" />);
    fireEvent.click(mic());
    await screen.findByText(/Listening/);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(engine.cancel).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/Listening/)).toBeNull();
    fireEvent.click(mic());
    await screen.findByText(/Listening/);
    fireEvent.click(screen.getByRole("button", { name: "Cancel dictation" }));
    await waitFor(() => expect(engine.cancel).toHaveBeenCalledTimes(2));
    expect(engine.stop).not.toHaveBeenCalled();
    expect(field().value).toBe("keep");
  });

  it("stops by itself at the length limit and reports empty results", async () => {
    const engine = mockEngine({ full: true });
    const onError = vi.fn();
    render(<Harness onError={onError} />);
    fireEvent.click(mic());
    await waitFor(() => expect(engine.stop).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(onError).toHaveBeenCalledWith("No speech was recognised."),
    );
    expect(field().value).toBe("");
  });

  it("Ctrl+Shift+Space held is push-to-talk", async () => {
    const engine = mockEngine({}, "from the keyboard");
    const now = vi.spyOn(Date, "now").mockReturnValue(10_000);
    render(<Harness />);
    act(() => {
      fireEvent.keyDown(field(), {
        key: " ",
        code: "Space",
        ctrlKey: true,
        shiftKey: true,
      });
    });
    await screen.findByText(/Listening/);
    expect(engine.start).toHaveBeenCalledTimes(1);
    now.mockReturnValue(10_800);
    fireEvent.keyUp(field(), { key: " ", code: "Space" });
    await waitFor(() => expect(field().value).toBe("from the keyboard"));
  });
});

describe("pure helpers", () => {
  it("recognises the dictation shortcut only with Ctrl and Shift", () => {
    const key = { key: " ", code: "Space", metaKey: false };
    expect(isVoiceShortcut({ ...key, ctrlKey: true, shiftKey: true })).toBe(
      true,
    );
    expect(isVoiceShortcut({ ...key, ctrlKey: true, shiftKey: false })).toBe(
      false,
    );
    expect(isVoiceShortcut({ ...key, ctrlKey: false, shiftKey: true })).toBe(
      false,
    );
    expect(
      isVoiceShortcut({
        key: "v",
        code: "KeyV",
        ctrlKey: true,
        shiftKey: true,
        metaKey: false,
      }),
    ).toBe(false);
  });

  it("labels every state", () => {
    expect(micLabel("idle")).toMatch(/hold to talk/);
    expect(micLabel("starting")).toMatch(/Starting/);
    expect(micLabel("listening")).toMatch(/Stop dictation/);
    expect(micLabel("transcribing")).toBe("Transcribing…");
  });
});
