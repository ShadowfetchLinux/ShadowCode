# OpenRouter

[OpenRouter](https://openrouter.ai) gives one API key access to hundreds of
hosted models. ShadowCode offers it for people without a subscription CLI, or
for trying a model before committing to one. Every token is billed to your
OpenRouter account. ShadowCode never presents these rows as a subscription.

## Add a key

1. Create a key at [openrouter.ai/keys](https://openrouter.ai/keys). Set a
   credit limit on the key there if you want a hard cap.
2. Open **Settings › Accounts**, find the **OpenRouter** card, paste the key
   and choose **Save**.

ShadowCode checks the key with OpenRouter (`GET /api/v1/key`) before storing
it. A rejected key is not saved. A saved key lives in the profile's
`secrets.env` (readable only by you) as `OPENROUTER_API_KEY`, and is never
shown again, logged, or passed to other tools. **Remove key** deletes it.

The card shows the key's label, credits used, its limit and what is left, and
how many models are available. **Open OpenRouter activity** links to
OpenRouter's own usage page.

## Pick a model

The picker's **API keys** group lists OpenRouter's text models, refreshed from
OpenRouter's public model list at most every six hours (or with **Refresh
models**). Each row shows:

- the price per million input and output tokens, or *free*;
- *Vision* when the model accepts images;
- *Chat only* when the model doesn't support tool calls. Such a model can
  answer questions but can't read or edit files, because ShadowCode sends it
  no tools.

The group starts collapsed; use **Show all** or type in the search box (name
or slug, for example `qwen coder` or `qwen/qwen3-coder`).

## How tasks run

OpenRouter models run on ShadowCode's own agent loop, the one local models
use. Your permission mode and approvals, checkpoints, the Web toggle
(`web_fetch`, `web_search`), image attachments for *Vision* models, the
activity timeline and the review drawer all work the same way. The model's
context window comes from OpenRouter's list, capped at 200,000 tokens.

Switching a conversation from a local model to OpenRouter sends earlier turns
to the cloud, so ShadowCode asks first, as it does for subscriptions.

## Offline and failures

- **Offline mode** turns the rows off and sends nothing to OpenRouter.
- **No key**: rows say *Add API key* and open the Accounts card.
- **Rejected or out of credit**: OpenRouter's error is shown on the task.
