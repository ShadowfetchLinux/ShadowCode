/**
 * Fake voice routes for the Playwright suite. Install it after
 * `installFakeBackend`: it wraps that bridge, answers `/api/voice/…` with a
 * scripted microphone and transcription, and passes everything else through.
 *
 * Self-contained like fakeBackend.ts (Playwright serialises it).
 */
export type FakeVoiceOptions = {
  /** A model is installed (otherwise start reports what is missing). */
  installed?: boolean;
  /** What stop returns. */
  transcript?: string;
};

export function installFakeVoice(options: FakeVoiceOptions = {}) {
  type Json = Record<string, any>;
  const inner = (window as any).__SHADOW_TEST_TRANSPORT__;
  if (!inner) throw new Error("installFakeBackend must run first");
  const fake = (window as any).__SHADOW_FAKE__;
  const log: { method: string; path: string; body: any }[] = fake.log;
  const catalog = [
    ["base.en", "Whisper base (English)", 147_964_211, true],
    ["tiny.en", "Whisper tiny (English)", 77_704_715, true],
    ["base", "Whisper base (multilingual)", 147_951_465, false],
  ] as const;
  const voice = {
    config: {
      engine: "local",
      model: "base.en",
      language: "en",
      openrouter_model: "google/gemini-3.1-flash-lite",
      voice_commands: true,
      live_preview: true,
      max_seconds: 120,
    } as Json,
    installed: new Set<string>(options.installed ? ["base.en"] : []),
    downloads: new Map<string, number>(),
    recording: null as null | { started: number },
    transcript: options.transcript ?? "Add a unit test for the parser.",
  };
  const active = () =>
    voice.installed.has(voice.config.model)
      ? voice.config.model
      : catalog.find(([id]) => voice.installed.has(id))?.[0];
  const blocked = () =>
    active()
      ? null
      : "No voice model is installed. Open Settings › Voice and install Whisper base (English) (147 MB).";
  const status = () => {
    // A download finishes on the third status poll.
    for (const [id, polls] of voice.downloads) {
      if (polls >= 2) {
        voice.downloads.delete(id);
        voice.installed.add(id);
      } else voice.downloads.set(id, polls + 1);
    }
    const using = active();
    return {
      config: voice.config,
      models: catalog.map(([id, name, bytes, english]) => ({
        id,
        name,
        bytes,
        license: "MIT",
        english_only: english,
        summary: `${name}.`,
        installed: voice.installed.has(id),
        active: using ? using === id : voice.config.model === id,
        progress: voice.downloads.has(id)
          ? {
              state: "downloading",
              done: (bytes * (voice.downloads.get(id)! + 1)) / 4,
              total: bytes,
            }
          : null,
      })),
      languages: [
        { code: "auto", name: "Detect automatically" },
        { code: "en", name: "English" },
        { code: "de", name: "German" },
      ],
      ready: !blocked(),
      blocked: blocked(),
      cpu_supported: true,
      whisper_version: "1.8.3",
      openrouter_key: false,
      offline: false,
      recording: Boolean(voice.recording),
    };
  };

  function route(method: string, path: string, body: any): unknown {
    if (!path.startsWith("/api/voice/")) return undefined;
    if (path === "/api/voice/status") return status();
    if (path === "/api/voice/config" && method === "POST") {
      Object.assign(voice.config, body);
      return { ok: true, config: voice.config };
    }
    if (path === "/api/voice/models/install") {
      voice.downloads.set(body.model, 0);
      return { ok: true, started: true };
    }
    if (path === "/api/voice/models/remove") {
      const removed = voice.installed.delete(body.model);
      return { ok: true, removed };
    }
    if (path === "/api/voice/start") {
      const reason = blocked();
      if (reason) throw new Error(reason);
      if (voice.recording) throw new Error("Already listening");
      voice.recording = { started: Date.now() };
      return { ok: true, engine: "local", device: "Fake microphone" };
    }
    if (path === "/api/voice/recording") {
      if (!voice.recording) return { active: false };
      const seconds = (Date.now() - voice.recording.started) / 1000;
      const words = voice.transcript.split(" ");
      return {
        active: true,
        engine: "local",
        device: "Fake microphone",
        level: 0.4 + 0.3 * Math.abs(Math.sin(seconds * 6)),
        seconds,
        max_seconds: voice.config.max_seconds,
        full: false,
        error: null,
        partial: words
          .slice(0, Math.min(words.length, 1 + Math.floor(seconds * 3)))
          .join(" "),
      };
    }
    if (path === "/api/voice/stop") {
      if (!voice.recording) throw new Error("Not listening");
      const seconds = (Date.now() - voice.recording.started) / 1000;
      voice.recording = null;
      return { text: voice.transcript, engine: "local", seconds, ms: 350 };
    }
    if (path === "/api/voice/cancel") {
      const cancelled = Boolean(voice.recording);
      voice.recording = null;
      return { ok: true, cancelled };
    }
    return undefined;
  }

  const bridge = {
    ...inner,
    async request(path: string, method: string, body: unknown) {
      if (!path.startsWith("/api/voice/"))
        return inner.request(path, method, body);
      log.push({ method, path, body });
      const answer = route(method, path, body);
      if (answer === undefined)
        throw new Error(`No fake voice route: ${method} ${path}`);
      return JSON.parse(JSON.stringify(answer));
    },
  };
  (window as any).__SHADOW_TEST_TRANSPORT__ = bridge;
  fake.voice = voice;
}
