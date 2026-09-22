# Native inference — Ollama / OpenAI HTTP

ShadowCode talks to local and remote models over HTTP only. It does **not**
vendor candle, llama.cpp, or a second GPU runtime.

## Ollama keep-alive

For the `ollama` provider, chat requests include `keep_alive` (default `30m`,
configurable as `model.keep_alive` in config). This asks Ollama to keep the
loaded weights resident between turns so cold reloads are less frequent.

## Prefix caching — honest limits

Ollama's `/api/chat` does **not** expose a documented, safe prompt-prefix cache
control comparable to some hosted APIs. ShadowCode therefore:

- Still sends the full message list each turn (required for correctness).
- Does **not** claim a 3× speedup from prefix reuse.
- May compute an internal hash of the leading system text + tool schemas for
  diagnostics; that hash is not a guarantee the provider skipped re-prefill.

If a future Ollama API adds an explicit prefix-cache hint that can be tested
safely on this machine, it can be wired without inventing numbers.

## Local models on 16 GB

Expect one active local model at a time. Parallel worktrees are capped at two
workers for that reason; do not spawn architect+critic swarms against one GPU.
