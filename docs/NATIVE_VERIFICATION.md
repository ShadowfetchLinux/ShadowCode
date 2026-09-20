# Native engine verification — development branch

Recorded on 2026-09-20. This is evidence for the Rust engine under development,
not a claim that the native desktop release is complete. The full release gates
remain in [NATIVE_MIGRATION.md](NATIVE_MIGRATION.md).

## Automated checks

`cargo fmt --all --check`, Clippy with warnings denied, and all **50 native
integration tests** pass on the development machine. The suite covers:

- Config validation, private secrets, untrusted project overlays, profile locks,
  legacy SQLite backups and goal import, session branching, and durable replay.
- 2,000 events from eight concurrent writers; 32 concurrent workspaces plus 24
  immediate follow-ups, then restart and exact completion/usage checks.
- Native tool execution, stale edits, ambiguous/malformed multi-file patches,
  CRLF and missing final newlines, partial-write checkpoint recovery, and rewind
  conflict preflight that preserves unrelated edits.
- Approval isolation, expiry, cancellation, aborted futures, read-only tasks,
  queued cancellation, hung providers, and managed shutdown.
- Tool-call/result pairing across compaction and recovery, live file attachments,
  verification retries, token limits, and labelled estimates for absent usage.
- Streaming UTF-8/SSE/NDJSON, malformed/truncated provider responses, large frame
  batches, bounded process output, concurrent commands, and process-group cleanup.

The host's distro `rustdoc` needs its LLVM library directory in the loader path
for doc tests. The full suite was run with:

```sh
LD_LIBRARY_PATH=/usr/lib/rustlib/x86_64-unknown-linux-gnu/lib cargo test --workspace --locked
```

The pinned CI toolchain does not require this host-specific workaround.

## Real local models

`native/core/examples/probe_task.rs` creates a disposable Rust package and an
isolated ShadowCode profile. Each model must inspect a broken addition function,
make the minimal edit without changing package metadata or tests, and run
`cargo test --offline --lib` through an approval restricted to that command in
the disposable project. The probe independently repeats the assertions, runs a
read-only follow-up, checks actual fresh-file inspection, and rewinds the edit.

Both installed models, **Ollama gpt-oss:20b** and **qwen3:14b**, completed the
coding workflow. Qwen initially produced an ambiguous replacement and then an
invalid Rust edit. The tools rejected the ambiguity, the compiler exposed the
invalid edit, and a stale hash forced a fresh read before the model repaired it.
That run passed in 12 model steps with 27,188 reported tokens.
The final gpt-oss run passed in six model steps with 8,292 reported tokens.

Earlier probe runs exposed two harness issues: an overly strict test-only
approval rule rejected equivalent project-directory arguments, and unrestricted
`cargo test` hit the host's `rustdoc` loader problem. The probe now accepts only
equivalent project directories and exercises library tests explicitly. Another
run exposed a model answering a follow-up from conversation history instead of
reading the current file. Named-file requests now attach current native reads;
the probe asserts that fresh inspection actually occurred.

These small coding probes establish tool interoperability, recovery from a real
compiler failure, continuation, and rewind. They do not establish performance
on large repositories or native-window usability. Desktop/UI tests, integrations,
release artifact checks, and installation remain mandatory before release.
