# Models and routing in the native desktop

The native 0.20 development build can choose a registered model for each task
mode. Open **Health → Router**, choose the **Plan**, **Build**, **Review**, and
**Test** models, then enable routing. Each field saves immediately. Leave a field
on **Default** to follow the application model configured in Settings.

The composer shows **Automatic by task mode** while routing is enabled. Choosing
a specific model in the composer overrides the route for that task. The model
note above the response records the actual model, provider, and purpose, and
survives reload. The context meter uses that task's model context limit.

The engine normally reserves a quarter of that window for a response (up to
8,192 tokens). If essential input leaves less room after history compaction,
it can request a shorter response down to 256 tokens. The actual provider
request uses that reduced limit; the configured context window never grows
silently. If input, tools and the minimum response still cannot fit, the task
reports the estimated requirement and stops before sending an oversized request.

Default goal milestones use Plan for inspection, Build for implementation, and
Test for required command verification. Planning and review retain read-only
permissions regardless of the selected model. A different model never grants
additional workspace or command permissions.

## Models with the same name

Discovered and newly configured models receive IDs derived from their provider,
endpoint, and model name. Two servers can offer the same model name without
overwriting each other. The model picker includes the endpoint when otherwise
identical choices need distinguishing. Existing saved IDs remain usable aliases;
custom aliases cannot be reassigned to a different model or server.

Discovery refreshes capabilities without overwriting a configured credential
environment variable or context limit. Switching the default keeps its previous
registered configuration available for routing. Credential values remain in
private secret storage; they are not included in task routing events.

Use registry IDs for API configuration. A bare model name is accepted only when
it resolves unambiguously, unless it is already an exact saved ID. Add a model
through **Custom model…** or configure it in Settings before assigning it to a
route. The native service also supports `POST /api/models/register` through IPC
for integrations that need to register a model without changing the default.

## Changes and failures

Routing selects a configuration when a task enters the queue. Changes affect
newly queued tasks; tasks already running or queued retain their selected model,
endpoint, credentials reference, and context limit. A future goal milestone uses
the routing configuration in effect when that milestone is queued.

An explicit route edit must name a registered coding model. If an older saved
configuration refers to a missing or ambiguous model, automatic selection uses
the default and records a **Using default** notice with the reason. The Health
drawer also shows the fallback. An unknown explicit composer/API selection is
rejected instead of silently changing the requested model.

A network, authentication, or provider response failure remains a failure for
the chosen provider. ShadowCode does not retry that task against another host.
Normal bounded retries, when configured, use the same chosen provider.

The legacy `architecture`, `small_edits`, `vision`, and `local` routing keys are
retained for configuration/API compatibility. They do not add new tool or image
input capabilities. The desktop exposes the four task modes above.

See [native verification](NATIVE_VERIFICATION.md) for the tests and
[migration gates](NATIVE_MIGRATION.md) for remaining release work.
