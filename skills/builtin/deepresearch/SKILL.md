---
name: deepresearch
description: Deep, source-backed research on a named topic. Plan the question, discover sources broadly with tavily_search, fetch the key articles and SAVE them into a corpus under .axonerai/scratch/deepresearch/<topic>/, read/grep that saved corpus, then deliver a short report in which every claim cites a saved file or a search result. Inspired by Mistral's deep-research skill in Le Chat / Vibe Work — adapted to axonerai's tools, not copied.
system-prompt-append: You are in deep research mode: be exhaustive, cite a source for every claim, and prefer saved evidence over memory.
---

# Deep research (axonerai flagship builtin)

Turn a question into a short, CITED report built on evidence you fetched and
saved yourself. The corpus is the deliverable's backbone: a claim without a
citation to a saved file or a search result does not go in the report.

You have a SMALL budget of model rounds — FIVE, and the fifth is the
report itself. One model call happens per round, so BATCH your tool calls
ferociously: several tool calls in a single round whenever they do not
depend on each other's results. The exact plan is below; follow it round by
round and never add a sixth round. If a tool fails, degrade gracefully
(fewer sources, say so in the report) — never spend extra rounds recovering.

## The pipeline (five rounds, exactly)

### Round 1 — Plan + search broad (tavily_search)

Write a 1-2 sentence plan: what the question means and what an
authoritative answer needs. Then TWO `tavily_search` calls, batched in this
same round, on different angles (definition/primary source; practical or
critical angle). From the combined results pick the TWO most authoritative,
distinct URLs (a spec/RFC beats a blog; two pages saying the same thing is
one source too many). Keep every result's URL + title — even unfetched ones
are citable as "search result" evidence.

### Round 2 — Fetch (WebFetch)

`WebFetch` both chosen URLs, batched (use the `query` field to focus
extraction on the question). You now hold both articles in context — this
is the evidence.

### Round 3 — Save the corpus (jailed write_file)

`write_file` each article, batched, into the corpus:

    deepresearch/<topic-slug>/<NN>-<slug>.md

where `<topic-slug>` is a short kebab-case slug of the topic (for the
question "what is JTD (RFC 8927)" use `jtd-rfc-8927`). Begin each saved
file with a provenance header:

    source: <url>
    fetched-for: <the research question>
    ---

followed by the article text you fetched. Two files, one round, done.

### Round 4 — Read the corpus back (batched read)

`ReadFile` both saved files — and nothing else, no tools beyond the reads.
The read-back exists to verify the corpus landed and to quote it precisely.
Check each file against your plan: what does it actually establish?

### Round 5 — Synthesize and deliver (no tools)

Write the report:

- Short (this is a report, not a book): definition first, then the points
  the evidence supports, then open questions if the corpus is thin.
- EVERY claim ends with a citation marker pointing at saved evidence:
  the saved-file path (e.g. `.axonerai/scratch/deepresearch/<topic-slug>/01-<slug>.md`)
  and/or the source URL. A claim with no marker must be cut.
- If two sources disagree, say so and cite both.
- If the evidence does not cover part of the question, say "the saved
  corpus does not cover X" — do NOT fill the gap from memory.

## Honesty rules

- Quote saved files, never invent quotes. If you did not fetch it, you may
  cite it only as a search result (URL + title), not as read evidence.
- Prefer saved evidence over memory, always.
- The corpus directory must stay under `.axonerai/scratch/` — the write
  tool is jailed there and will reject anything else.
