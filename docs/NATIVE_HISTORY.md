# Native desktop conversation history

The native desktop opens a recent page of a saved conversation. Use **Older
messages** above the transcript to read earlier pages, **Newer messages** to
retrace your steps, or **Latest messages** to return to the current task. Loading
a page moves the transcript to its top; returning to the latest messages restores
following at the bottom. Live work, cancellation and approvals remain active
while you browse saved history. A failed page request leaves the current page
visible and can be retried. Switching sessions discards late page responses.

Pages can start partway through a task or response. Earlier context is on the
previous page. The desktop retains the current live transcript and one historical
page; newer-page navigation retains cursors rather than caching each page body.
Following a queued task in the same conversation preserves the historical page
being read.

## Bounds and complete records

A saved page contains at most 128 events and 2 MiB of event payload, plus
row metadata. Individual saved events larger than 256 KiB are represented by an
explicit preview notice. Open the command palette (Ctrl+K) and choose **Export
this task as JSON** to retain the original event content, including complete tool
records. Markdown export is a readable conversation summary. Preview limits
never rewrite stored history. The existing 32 MB export limit
still applies and is reported if exceeded. Ordinary events, including Unicode
content, are not truncated to fill a page: the remaining events are available on
the next page.

These are saved-history/initial-load bounds, not a claim of constant memory for a
long-running live task or a virtualized Markdown renderer. Further live-stream,
large-single-message and WebKit memory stress remain native release gates.

## Service contract

The desktop requests `GET /api/sessions/ID?view=window` or
`POST /api/sessions/ID/activate?view=window`. These return the recent `events`, a
frozen `event_cursor`, empty `tasks` (the desktop does not consume that bulk list),
and `history_page: {first_cursor, has_older}`. The default session contract remains
available to existing CLI/API clients.

`GET /api/sessions/ID/events?view=window&before=CURSOR` returns the preceding page
with an exclusive positive cursor, `first_cursor`, `event_cursor`, and
`has_older`. Appended events do not change that upper boundary. The history index
scopes every page to one session; page limits are enforced by the store before
serializing data for the desktop. The existing event stream, CLI history and
export routes retain their contracts. The legacy browser transport is unchanged.
