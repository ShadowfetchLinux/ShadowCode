ShadowCode 0.30.2 lets OpenRouter models search the web, keeps your
OpenRouter key away from other tools, and brings the documentation up to
date. The 0.30.1 notes follow below.

## Fixed in 0.30.2

- **Web for OpenRouter.** The **Web** toggle now appears for OpenRouter
  models, as the 0.29 notes promised. Tested live: an OpenRouter model searched
  the web and cited its source.
- **Your OpenRouter key stays put.** It is stripped from the environment of
  every subscription tool ShadowCode starts, like the other provider keys.
- **No secrets in the repository.** A new check scans every tracked file for
  real-looking API keys and runs on every push.
- **Docs** for OpenRouter, Antigravity, data locations and testing are current.

## Fixed in 0.30.1

- **Antigravity works end to end.** Tested live after signing in: it lists
  its 11 Gemini models, asks ShadowCode before running a command, answers, and
  resumes the conversation. A timing bug that made the first prompt end with
  no answer is fixed.
- **Answers appear once.** Replies from Codex, Claude Code, Cursor, Grok and
  Antigravity are no longer repeated under *Result*.
- **Clearer notes.** The model note reads "Using Cursor · Auto · Cloud", and a
  question that changed nothing ends with one quiet line instead of an empty
  report card.

## New in 0.30.0

- **Antigravity asks first.** It now runs through Google's official ACP agent
  server instead of the `agy` CLI's print mode, which silently denied any
  command. Commands and edits reach ShadowCode's approvals, images work, and
  conversations resume.
- **One-time install.** **Settings › Accounts › Antigravity › Install**
  downloads the server from Google (334 MB, checksum-verified). **Connect**
  then signs in with Google in your browser. This sign-in is ShadowCode's own
  and separate from the `agy` CLI.
- **No surprise browser tabs.** Status checks and tasks never open a sign-in
  page. If the sign-in expires, the row says *Sign in* and a task stops with
  that message.

## New in 0.29.0

- **OpenRouter.** Paste an OpenRouter API key in **Settings › Accounts** and
  pick any of its text models from the picker's new **API keys** group. Rows
  show the price per million tokens and whether the model supports images or
  tools. Every token is billed to your OpenRouter account, and the group says
  so. These models run on ShadowCode's own agent loop, so permissions,
  approvals, checkpoints, web tools, images and review all work. See
  [OpenRouter](https://github.com/Shadowfetchapps/ShadowCode/blob/main/docs/OPENROUTER.md).
- **Web search.** When DuckDuckGo answers with a bot check, `web_search` asks
  Marginalia Search's public API instead of giving up.
- **VPN users.** Sites that a VPN's DNS resolves into 192.0.0.0/24 (NordVPN
  does this for Google) are no longer refused by `web_fetch`.
- **Antigravity.** Tasks where Antigravity silently denied a command now fail
  with an explanation instead of reporting success with no answer.

## Fixed in 0.28.1

- **Accounts.** Each card shows the product name, its version once, and the
  signed-in email with the plan on one line. Usage windows are listed once, with
  reset times that stay current. Credits appear only when the vendor reports
  some.
- **Reset times** round to whole minutes ("resets in 3h", not "2h 60m").
- **Stopped tasks** that changed nothing show a single quiet line instead of a
  red card and without a "Needs attention" note. The partial reply is kept.
- **Newly added local models** always appear in the picker right away.
- **Local model names** come from the GGUF file's own name, plus its
  quantization, for example *Qwen3 14B · Q4_K_M*.
- **Notifications** appear at the top centre, clear of the composer and the
  drawer. "Permissions & network" no longer wraps in Settings, and the drawer
  close button is larger.
- **First run** no longer checks local servers or vendor tools that the
  onboarding screen does not use.

## ShadowCode 0.28.0

ShadowCode 0.28.0 is a Linux desktop coding agent with one window and one
model picker. It runs your existing subscriptions through the vendors' official
command-line tools, or GGUF models on your own hardware with a llama.cpp
runtime that ships with the app.

## What changed

- **One picker** lists **Subscriptions** and **On this computer**. Every row
  shows Local or Cloud, whether it is ready (and why not), Vision and Chat only
  badges, and the usage the vendor reports. Each conversation remembers its
  model.
- **Accounts.** **Settings › Accounts** signs in and out with each vendor's
  official login and logout commands. ShadowCode never reads credential files,
  and it removes provider API keys from the environment of every vendor task,
  login and logout.
- **Usage.** Codex shows its rate-limit windows per quota pool, with reset
  times. Other vendors show *Usage unavailable* with the reason. The last
  snapshot survives a restart. When a vendor reports its plan limit, the task
  stops and you pick another model.
- **Local models.** A pinned llama.cpp (Vulkan and every x86-64 CPU level) is
  bundled in the AppImage and the deb. Models are read from their GGUF
  metadata. Context is sized to your VRAM or RAM. Models in an existing Ollama
  store can be imported by reference, without copying and without the daemon.
  Vision comes from a paired projector.
- **Web tools for local models.** `web_fetch` and `web_search` block private
  networks and cloud metadata addresses.
- **Network modes.** Online, Web tools off, Offline.
- **Permission modes.** *Ask before actions* (the default for new installs) and
  *Allow project edits*. Settings explains what each vendor enforces.
- **Switching providers mid-conversation.** A bounded summary of the earlier
  turns is handed over, and ShadowCode asks your consent before local content
  goes to a cloud provider.
- **Resuming vendor sessions.** Each vendor's own session is resumed per
  conversation.
- **Installer.** `scripts/install-appimage.sh` now requires `SHA256SUMS` and
  installs the llama.cpp runtime from the AppImage into
  `~/.local/lib/shadowcode`, rolling back if a step fails.
- **Removed.** The legacy Python harness and its tooling.

## Subscription status

| Vendor | Runtime | Image input | Approvals shown in ShadowCode | Usage |
| --- | --- | --- | --- | --- |
| Codex | `codex app-server` | Yes | Yes | Rate-limit windows per pool |
| Claude Code | `claude -p` stream-json | Yes | Yes | Unavailable |
| Cursor | `cursor-agent acp` | When advertised | Yes | Unavailable (plan tier only) |
| Antigravity | `agy --print=` stream-json | No | No (its own settings) | Unavailable |
| Grok | `grok agent stdio` (ACP) | No | Yes | Unavailable (session tokens only) |

## Local model support

- **Formats.** Any GGUF whose architecture the bundled llama.cpp supports.
  Unsupported ones (for example `gptoss` at this pin) are listed with the
  reason.
- **Hardware.** Vulkan GPUs, or any x86-64 CPU. One model is loaded at a time.
  If the GPU start fails, ShadowCode retries on the CPU and labels the row.
- **Tools and vision.** Models whose chat template has no tool support run as
  *Chat only*. Models with a paired projector accept images and get
  `view_image`.

## Known limits

- **Claude Code** was not verified end to end on the build machine, because it
  was not signed in there. The stream-json adapter is covered by unit
  tests with scripted protocol frames, and sign-in state by fake-CLI tests.
- **Antigravity** exposes no machine-readable usage. It never sends approval
  requests to ShadowCode: its own settings decide what it may do. Sign-in and
  sign-out happen only inside `agy`.
- **Cursor** reports its plan tier but no remaining allowance.
- **Grok**'s ACP interface accepts no images, and Grok has no read-only mode
  for Plan/Review.
- **`web_search`** uses DuckDuckGo's HTML page, which may block automated
  requests. The tool then reports that no results were retrieved.
- **Ollama stores outside `~/.ollama/models`** are found only through
  `OLLAMA_MODELS` or an `Environment=OLLAMA_MODELS` line in the Ollama systemd
  user unit. The system-wide service's store isn't found automatically.
- **ShadowCode is not an operating-system sandbox.** Shell commands run as
  your user.

## Downloads

- `ShadowCode_0.30.2_amd64.AppImage`
- `ShadowCode_0.30.2_amd64.deb`
- `ShadowCode_0.30.2_appimage-runtime-sources.tar.gz`
- `SHA256SUMS`

Builds are for x86_64 Linux with glibc 2.39 or newer (Ubuntu 24.04 or later).
Check the files with `sha256sum --ignore-missing -c SHA256SUMS`, then install
with `scripts/install-appimage.sh` (see the README).
