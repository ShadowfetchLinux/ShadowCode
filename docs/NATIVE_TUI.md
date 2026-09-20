# Native terminal interface — 0.20 development

`shadowcode tui` opens a fullscreen terminal workspace in the same Rust executable
as the desktop and command line. It requires a real, cursor-addressable terminal;
it does not start GTK, WebKit, Python, Node, or a browser. Model inference still
runs in Ollama or your configured compatible provider.

```sh
shadowcode --workspace /path/to/project tui
shadowcode --workspace /path/to/project tui --session FULL_CONVERSATION_ID
```

Global `--profile` selects an isolated profile for development. The AppImage accepts
these arguments after `--appimage-extract-and-run`. `--json` is for the ordinary
[CLI](NATIVE_CLI.md), not the fullscreen interface.

The terminal attaches to an existing native desktop, terminal, or `serve` engine
through its private Unix socket, or owns an engine if none is running. Conversations,
project trust, model configuration, permissions, tool approvals, checkpoints and
history use the same service. Changing the terminal's project or conversation does
not navigate an attached desktop. New tasks queue behind active work in that project.

## Controls

- Enter sends; Alt-Enter or Ctrl-J adds a newline. Bracketed paste inserts text
  without submitting it. Up/Down recall sent prompts; Left/Right and deletion
  respect Unicode graphemes. The composer holds at most 64 KB.
- F1 opens help; Esc closes a dialog. Page Up/Down scroll help and trust
  details; Home returns to the start. Scrolling stays in range after resizing.
- F2 selects a model. F3 or Ctrl-L cycles Build, Plan, Review and Test routing.
  Plan and Review retain the engine's read-only permissions.
- Ctrl-P selects saved conversations. Type to filter the recent list; Ctrl-R
  searches saved history with that text. `/resume FULL_ID` opens an exact ID.
- Ctrl-N starts a conversation. Ctrl-G selects a known project;
  `/open /absolute/path` opens another. Project switching requires all work owned
  by this terminal to finish or stop first, including tasks in other conversations.
- Ctrl-T shows the exact project before Enter confirms trust. A queued project
  change invalidates that confirmation; reopen the dialog to review the new path.
- F4 opens the pending approval. It defaults to **Deny**. Tab changes the choice;
  Enter applies it to the reviewed request. Page Up/Down scroll the exact request.
  Typing in the composer never approves anything. An oversized request cannot be
  approved in the terminal review dialog.
- Ctrl-O expands or collapses tool details. Page Up/Down scroll the conversation.
  Ctrl-B loads an older saved page; Ctrl-F returns to live events.
- Ctrl-C or Esc outside a dialog stops the selected task. Ctrl-Q/Ctrl-D quits.

Tab opens the slash-command catalog. Skills and custom model workflows run through
owned tasks, with the same instructions, routing and permissions as the desktop.
`/run COMMAND` and `/test COMMAND` start approval-controlled command jobs.
`/theme light|dark` changes appearance. `/export /absolute/file.md` writes the full
conversation privately and refuses to replace an existing destination.
Detailed settings and credential references remain available through the CLI.

## Persistence and lifecycle

The screen holds a bounded window of events; tool details and very long responses
may be shortened for display. Older pages and full Markdown export preserve access
to saved content. Output control characters and bidirectional overrides are displayed
as escapes so model or subprocess text cannot inject terminal commands.

Closing a terminal cancels its unfinished owned tasks, including queued workflows.
When attached to another engine, unrelated work continues. When the terminal itself
owns the engine, quitting shuts down the engine and its managed processes. Durable
goals and background services use their existing shared engine lifecycle; they are
not converted into terminal-owned jobs. Ordinary exit and handled signals restore
terminal settings. Drafts and prompt recall are currently in memory.

## Verification and remaining work

`node scripts/test-native-tui.mjs` drives a real PTY and scripted compatible model
with no display. It covers multiline Unicode paste, actual file inspection, shared
CLI attachment, explicit command approval, planning mode, owned task cancellation
and terminal restoration. Rust tests cover grapheme editing, bounded event replay,
small-terminal rendering, approval isolation, queue-full draft retention, workflow
ownership and exclusive history pagination. CI runs the PTY check against source
and AppImage builds.

This is a development interface. Broader picker/navigation, resizing, failure and
long-session stress, terminal visual review and final packaged verification remain
part of the [native release gates](NATIVE_MIGRATION.md). The full native release is
not declared complete by this interface's initial checks.
