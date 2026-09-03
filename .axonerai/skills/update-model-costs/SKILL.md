---
name: update-model-costs
description: Update per-provider model costs, context windows and offers in the axonerai-models jsonc configs. Use when pricing/model pages change, when refreshing context windows from a provider's API, when adding or removing a model entry, or when auditing the per-provider model config files.
---

# Update axonerai model costs

## Concept

Model-specific data (context windows, token costs, offers) lives in
per-provider JSONC files — NOT in the main app config or the GUI:

- Local: `.axonerai/models/<provider>-models.jsonc` (repo, shareable)
- User fallback: `~/.axonerai/models/<provider>-models.jsonc`
- LOCAL MASKS USER when both exist.

Each file is JTD-validated against `schemas/provider_models.jdt.json`
(`additionalProperties: false`) before use, and every mutation goes
serialize → validate → write (a failed validation writes nothing). Every
mutation auto-backs-up every `*-models.jsonc` to `<name>.<unix-epoch>` first.

NEVER hand-edit these files. Always use the `axonerai-models` bin (repo root):

```bash
cargo run --example axonerai-models -- <subcommand> …
```

## Providers

- `mistral`
- `opencode-go`
- `opencode-zen`

Note: opencode-zen and opencode-go are ONE provider (one `OPENCODE_API_KEY`)
with two endpoints — the accepted exception; they still get one config file
each.

## Where the costs live (pages DO move)

Prices change constantly and models appear/disappear. The `updated` date
field records freshness. Verify these URLs live today (re-locate with
WebSearch/WebFetch whenever one 404s — do not trust a stale URL):

- OpenCode Zen models page: https://opencode.ai/docs/zen
  (verified 2026-09-03, HTTP 200)
- OpenCode Go models page: https://opencode.ai/docs/go
  (verified 2026-09-03, HTTP 200)
- Mistral pricing page: https://mistral.ai/pricing
  (verified 2026-09-03, HTTP 200)

## The `offer` field

Each model entry has an optional freeform `offer` string, e.g. `"2x usage"`
or `"half price to end of year"`. As context only: GLM-5.3-Flash currently
carries a special "2x usage" offer — do NOT store or display it yet.

## Workflow

1. WebFetch the pricing/model pages above for current per-MTok costs.
2. `cargo run --example axonerai-models -- fetch <provider>` — pull the
   provider's models endpoint and fill in `context_window` where reported
   (backup first, diff printed). Only documented endpoints are wired up;
   unknown providers print a note, never a guessed URL.
3. `cargo run --example axonerai-models -- set-cost <provider> <model-id> <in> <out>`
   for each model, using the strings from the page (e.g. `$1.40` `$4.40`).
4. `cargo run --example axonerai-models -- set-context <provider> <model-id> <tokens>`
   when the API/page disagrees with the config's context window.
5. `cargo run --example axonerai-models -- dump <provider>` to verify.
6. Commit the changed jsonc.

Prices added/removed/changed follow the pages:

- A model gone from the page gets its costs removed (its `context_window`
  may stay).
- A new model gets a new entry with costs from the page.
