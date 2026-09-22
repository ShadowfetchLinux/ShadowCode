# Native inference settings

ShadowCode connects to local or hosted providers over HTTP. Model weights and
inference runtimes remain external; the app does not bundle a GPU inference
engine.

## Context and memory

Settings → Model exposes the context window in tokens. The default recommendation
for a newly discovered local model is 16,384 tokens. Choose a value supported by
the model and available memory. The configured limit is sent to Ollama as
`options.num_ctx`; ShadowCode does not automatically prove that a chosen value
fits the model or GPU. Larger windows can substantially increase memory usage.

## Ollama residency

**Keep model loaded** controls `model.keep_alive`, default `30m`. Supported values
are positive durations (`500ms`, `5m`, `30m`, `1h`), `0` to unload after a reply,
or `-1` for indefinite residency. Zero and minus one are sent as JSON numbers.
Settings, saved model records and routed model selection preserve this value.
Keeping weights resident reduces cold starts but reserves memory.

The app sends the full message list each turn. It exposes no provider-specific
prompt-prefix cache control and makes no prefix-caching speedup claim. Preparing
two worktrees does not automatically start two inference jobs or load two models.
