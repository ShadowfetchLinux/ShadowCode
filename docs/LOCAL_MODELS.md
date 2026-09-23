# Local models

ShadowCode runs GGUF models on this computer with a pinned build of
[llama.cpp](https://github.com/ggml-org/llama.cpp) that ships with the app. It
needs no Ollama, LM Studio or other daemon. It never downloads weights and never
deletes them. Local rows appear under **On this computer** in the picker and
use ShadowCode's own agent loop, tools and permissions.

## The runtime

- **Location.** The AppImage and the deb carry the runtime in
  `usr/lib/shadowcode`: `llama-server`, its libraries (`$ORIGIN` links),
  loadable CPU variants for every x86-64 level, a Vulkan module, `COMMIT`,
  `architectures.txt` and `NOTICES/`. `scripts/install-appimage.sh` installs
  the AppImage's copy into `~/.local/lib/shadowcode`.
- **Which copy is used.** ShadowCode uses the first of these that exists:
  1. `local_engine.llama_binary`, if set.
  2. The runtime bundled with the running executable, if it is newer than the
     managed copy.
  3. `~/.local/lib/shadowcode`.
  4. The bundle.
  5. `SHADOWCODE_LLAMA_SERVER`.

  It never uses `llama-cli` or a `llama-server` found on `PATH`.
- **Readiness.** The runtime is *Ready* only after `llama-server --version`
  succeeds. **Settings › Local models › Runtime** shows its path, origin
  (bundled, managed or other), version, commit and backend.
- **Hardware.** Devices come from `llama-server --list-devices`, and RAM from
  `/proc/meminfo`. The Vulkan module loads only when a Vulkan loader and driver
  are present (`libvulkan1` plus your GPU driver). Otherwise the runtime stays
  on the CPU.

## Add models

In **Settings › Local models › Add a model**, choose a GGUF file or a folder.
The catalog lists at most 256 files. Removing a folder's file from the list
excludes it and leaves it on disk. Projector, vocabulary-only and embedding
GGUFs are never listed as models, and adding one directly is refused with the
reason.

### Import from Ollama

**Found in Ollama** lists the models in an existing Ollama store. ShadowCode
looks for the store in `OLLAMA_MODELS`, then `Environment=OLLAMA_MODELS` in the
Ollama systemd user unit, then `~/.ollama/models`. The first of these that has
manifests wins.

- **Import registers paths.** ShadowCode records the paths of the model blob
  and any projector blob. It never copies them and never writes to the store,
  and the Ollama daemon doesn't need to run.
- **Incompatible tags** are shown with the reason, for example
  `unsupported architecture gptoss`, and can't be imported.
- **Other stores.** A store in another location (such as a system-wide
  `/usr/share/ollama` install) is found only if `OLLAMA_MODELS` points to it.

## What ShadowCode reads from a GGUF

Everything comes from the file's header. The file name is never used.

- **Compatibility.** The architecture must appear in the runtime's
  `architectures.txt`, and the model must have a token embedding tensor.
- **Tools.** The chat template must support tool calls. If it doesn't, the row
  is **Chat only**: no tool schemas are sent, so the model can answer but can't
  read or change files.
- **Thinking switch.** If the template has an `enable_thinking` switch, it is
  sent as `false`.
- **Vision.** The model needs a paired projector (mmproj). Pairing is by name
  (`<model>.mmproj.gguf`, `<model>-mmproj.gguf`, `mmproj-<model>.gguf`,
  `<model>.mmproj`), or when a folder holds exactly one model and one
  projector. Either way, the projector must match the model. Once the model is
  loaded, the Vision badge follows what the server reports in `/props`. Vision
  rows accept image attachments and get a `view_image` tool. Rows without
  vision refuse images before a job is created.

## Memory estimate and context

Each row shows an estimate: weights, KV cache, compute buffers, projector and
overhead. The KV cache follows the model's per-layer KV heads and
sliding-window layers.

- **Context.** ShadowCode starts from the lower of the trained context and
  `local_engine.context_size` (default 16,384 tokens). It halves the context
  until the estimate fits the GPU (leaving 1 GiB of VRAM free), otherwise RAM
  (leaving 2 GiB free), but not below 4,096 tokens unless the model's own
  context is smaller.
- **One number.** The result is used both as the server's `--ctx-size` and as
  the agent's context limit.
- **Fit.** The row reports whether the model fits the GPU, fits only in RAM
  (CPU), or doesn't fit at all (*Not enough memory on this computer*).

## Loading

Choose **Load**, or just start a task on the row. ShadowCode starts one server:

```text
llama-server -m <gguf> --host 127.0.0.1 --port <free> --no-webui --jinja \
  --ctx-size <N> --parallel 1 [--mmproj <projector>] [-ngl 999]
```

- **Server security.** The environment is cleared except for an allow-list,
  and a new random key is passed as `LLAMA_API_KEY`. The key is never in argv
  and never stored. The web UI is off.
- **GPU placement.** If the estimate fits the GPU, all layers are offloaded
  (`-ngl 999`). If it fits only in RAM and a GPU exists, llama.cpp offloads what
  it can. Without a GPU, the server runs on the CPU (`--device none -ngl 0`).
- **CPU fallback.** If the server exits during a GPU start, it is retried once
  on the CPU. The row then says *Loaded · CPU fallback (GPU load failed)*.
  Otherwise the last error, including the server's own stderr, is shown on the
  row.
- **Cancel.** Loading waits up to 90 seconds for `/health` and can be
  cancelled with **Unload**.
- **One at a time.** Only one model is loaded. A task holds the loaded model
  until it ends. While any task holds it, loading another model or unloading
  is refused with "Another task is using …".
- **Stop.** Unload, a switch or quitting ShadowCode sends SIGTERM to the
  server's process group, then SIGKILL after 5 seconds. The server also dies
  with ShadowCode (`PR_SET_PDEATHSIG`).

## Web tools

With **Web** on for a task and the network mode *Online*, local models can use
`web_fetch` (a readable-text version of a page) and `web_search` (DuckDuckGo's
HTML results, at most 8). Results list their sources in the activity timeline.
Private, loopback, link-local and metadata addresses are blocked. To let the
agent reach a local dev server, add its exact `host:port` to
`network.allow_local_dev`. Full rules: [SECURITY.md](../SECURITY.md#web-tools).

If DuckDuckGo shows a bot check or an error, `web_search` returns
`blocked: true` with no results. The model is told that nothing was retrieved.

## Configuration

These settings are the `local_engine` section of `config.yaml`. **Settings ›
Local models** writes the lists for you when you add, remove or import models.
`llama_binary` and `context_size` can only be set in the file or with
`shadowcode config`.

```yaml
local_engine:
  directories: []    # folders to scan
  files: []          # individual GGUF files
  imports: []        # [{path, mmproj, name, source: ollama}] from Ollama imports
  excluded: []       # files removed from the list but still in a directory
  llama_binary: ""   # explicit llama-server path (optional)
  context_size: 0    # upper bound for the context; 0 = 16384
```

## Building the runtime yourself

[`scripts/build-llama.cpp.sh`](../scripts/build-llama.cpp.sh) builds the commit
in [`tools/llama.cpp.pin`](../tools/llama.cpp.pin) without root. The
prerequisites are in the [README](../README.md#build-from-source).
