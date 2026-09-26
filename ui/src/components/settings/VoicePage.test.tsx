import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { voiceApi, type VoiceStatus } from "../../lib/voice";
import { VoicePage } from "./VoicePage";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function status(overrides: Partial<VoiceStatus> = {}): VoiceStatus {
  const model = (id: string, name: string, bytes: number, english = true) => ({
    id,
    name,
    bytes,
    license: "MIT",
    english_only: english,
    summary: `${name} summary.`,
    installed: false,
    active: id === "base.en",
    progress: null,
  });
  return {
    config: {
      engine: "local",
      model: "base.en",
      language: "en",
      openrouter_model: "google/gemini-3.1-flash-lite",
      voice_commands: true,
      live_preview: true,
      max_seconds: 120,
    },
    models: [
      model("base.en", "Whisper base (English)", 147_964_211),
      model("tiny.en", "Whisper tiny (English)", 77_704_715),
      model("base", "Whisper base (multilingual)", 147_951_465, false),
    ],
    languages: [
      { code: "auto", name: "Detect automatically" },
      { code: "en", name: "English" },
      { code: "de", name: "German" },
    ],
    ready: false,
    blocked:
      "No voice model is installed. Open Settings › Voice and install Whisper base (English) (141 MB).",
    cpu_supported: true,
    whisper_version: "1.8.3",
    openrouter_key: false,
    offline: false,
    recording: false,
    ...overrides,
  };
}

it("lists models with sizes and installs only on click", async () => {
  vi.spyOn(voiceApi, "status").mockResolvedValue(status());
  const install = vi
    .spyOn(voiceApi, "install")
    .mockResolvedValue({ ok: true, started: true });
  render(<VoicePage onToast={() => undefined} />);
  const tiny = await screen.findByRole("article", {
    name: "Whisper tiny (English)",
  });
  expect(install).not.toHaveBeenCalled();
  expect(screen.getByText(/No voice model is installed/)).toBeTruthy();
  fireEvent.click(
    within(tiny).getByRole("button", { name: "Install (74 MB)" }),
  );
  await waitFor(() => expect(install).toHaveBeenCalledWith("tiny.en"));
  // English-only model: the language is fixed.
  const language = screen.getByLabelText("Language") as HTMLSelectElement;
  expect(language.disabled).toBe(true);
  expect(language.value).toBe("en");
});

it("keeps OpenRouter off without a key and saves the engine when chosen", async () => {
  vi.spyOn(voiceApi, "status").mockResolvedValue(status());
  render(<VoicePage onToast={() => undefined} />);
  const cloud = (await screen.findByLabelText(
    /OpenRouter/,
  )) as HTMLInputElement;
  expect(cloud.disabled).toBe(true);
  expect(screen.getByText(/Add an OpenRouter API key/)).toBeTruthy();
  cleanup();

  vi.spyOn(voiceApi, "status").mockResolvedValue(
    status({ openrouter_key: true }),
  );
  const save = vi.spyOn(voiceApi, "save").mockResolvedValue({
    ok: true,
    config: status().config,
  });
  render(<VoicePage onToast={() => undefined} />);
  const enabled = (await screen.findByLabelText(
    /OpenRouter/,
  )) as HTMLInputElement;
  expect(enabled.disabled).toBe(false);
  fireEvent.click(enabled);
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith({ engine: "openrouter" }),
  );
});

it("offers remove and use for installed models; offline blocks installs", async () => {
  const base = status({ offline: true, ready: true, blocked: null });
  base.models[1] = { ...base.models[1], installed: true, active: true };
  base.models[0] = { ...base.models[0], active: false };
  vi.spyOn(voiceApi, "status").mockResolvedValue(base);
  const remove = vi
    .spyOn(voiceApi, "remove")
    .mockResolvedValue({ ok: true, removed: true });
  render(<VoicePage onToast={() => undefined} />);
  const tiny = await screen.findByRole("article", {
    name: "Whisper tiny (English)",
  });
  expect(within(tiny).getByText("In use")).toBeTruthy();
  fireEvent.click(within(tiny).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(remove).toHaveBeenCalledWith("tiny.en"));
  const multi = screen.getByRole("article", {
    name: "Whisper base (multilingual)",
  });
  expect(
    (
      within(multi).getByRole("button", {
        name: /Install/,
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});
