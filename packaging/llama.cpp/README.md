# Managed llama.cpp runtime

`scripts/build-llama.cpp.sh` compiles a pinned
[llama.cpp](https://github.com/ggml-org/llama.cpp) and writes `llama-cli` plus
`llama-server` into `bin/` here, then installs them to
`~/.local/lib/shadowcode/`. The source checkout is `tools/llama.cpp/` and is
not committed. The pin file is `tools/llama.cpp.pin`.

This is a CPU build. It is not an Ollama or LM Studio daemon. Model weights are
never downloaded by the build.
