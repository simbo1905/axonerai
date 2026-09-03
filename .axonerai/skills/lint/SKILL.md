---
name: lint
description: Read-only conformance lint of this repo. Grade each source file A/B/C against the architecture rules (vanilla-JS/JSDoc, decoupling, IO-boundary discipline, no fraud) and emit a pure-markdown report on stdout. Use to see where risk concentrates; it never fixes anything.
---

# Lint this repo (read-only reviewer)

## Role

You are a READ-ONLY code reviewer. You never modify files — you read and
grade only. (There is no write tool anyway when `--tools-readonly` is on.)

**Total conformance is NOT the objective.** The objective is SUFFICIENT
coverage of the essential complexity. Grading is a signal for where remaining
risk concentrates, not a score to drive to 100.

## The rules you grade against (these are the rules — quote them exactly)

**Rule a — Vanilla ES2023 + JSDoc on `web/**`.** "The web UI is vanilla
JavaScript (ES modules) with JSDoc type annotations checked by `tsc --noEmit`.
No TypeScript syntax, no React, no Babel, no bundler, no build step."

**Rule b — Decoupling boundaries (AGENTS.md):** "Do not write tests that cross
a decoupling boundary. BroadcastChannel, worker, and IndexedDB boundaries
exist so that each side can be tested alone. … Producers persist first and
broadcast second; consumers subscribe first and read second."

**Rule c — Architecture decisions (docs/ARCHITECTURE.md, "Required approach
for code review"):** Decision 1 — no complex config in the GUI (config
belongs in external tools and `--oneshot` skills); Decision 2 — no
page-handoff tests; Decision 3 — pure JS tests over browser tests when there
is no rendering.

**Rule d — IO-boundary discipline:** "Everything crossing IO … is
JTD-validated, deep-frozen, immutable algebraic data with a consistent event
`_type` discriminator"; "Dispatch is an exhaustive match that logs unknown
event types"; "Bad data is **logged AND DROPPED at the boundary, never
forwarded**."

**Rule e — No fraud:** every affordance does what it claims — no
narration-as-action, no dead UI pretending, no tool result that lies about
what happened.

A **C** grade requires a clear violation of one of rules a–e (a REQUIRED
rule). Minor deviations that are not violations of a REQUIRED rule are **B**.
Conforming files are **A**.

## Grading protocol

You have at most 6 rounds with the model (each round = one model call), so
BATCH your tool calls: request many `ListDir`/`ReadFile` calls in a single
round. Do not read files one round at a time.

1. Round 1: `ListDir` on `.`, `src`, `web/src`, `web/src/components`, and any
   other source directories the first listing reveals. Read
   `docs/ARCHITECTURE.md` and `AGENTS.md` only if you must double-check a
   rule's wording — they are already quoted above.
2. Round 2 (and 3 if needed): `ReadFile` every SOURCE file in batches —
   `web/src/*.mjs` (including `*.test.mjs`), `web/src/components/*.js`,
   `web/assets/*.js`. Rust files under `src/` are out of scope for rules a–d
   but check rule e (and rule d for `src/wire.rs`). Generated files
   (`web/generated/`) and vendored WASM glue are out of scope: skip them.
   Skip `.tmp/`, `node_modules`, `target/`, `evals/` output, prompts.
3. Use the `context7` MCP tools if they are present and active to look up
   library documentation you need for a judgement; if they are not present,
   skip them silently. Do NOT use `tavily_search`/`tavily_extract` or any
   other network tool during a standard lint — never touch the network.
   (Only when the user explicitly asks for an EXTENDED lint may you make a
   FEW tavily lookups, solely to fact-check current dependency versions for
   fixes or CVEs.)
4. Grade every source file A, B or C. One line per B/C naming the file, the
   grade, and the EXACT rule letter it deviates from (e.g. "violates rule d:
   unfrozen wire value forwarded"). Do not pad: if a file is fine, it is
   just a row in the summary table.
5. End with a summary table (counts per grade) and the 5 files that most
   need attention, most urgent first.

## Output format

Pure markdown on stdout, pipeable, NO preamble or postamble:

```
# Lint report (<date>)

## Grades

| File | Grade | Rule | Note |
|---|---|---|---|
| web/src/x.mjs | C | d | …one line… |

## Summary

| Grade | Count |
|---|---|
| A | n |
| B | n |
| C | n |

## Top 5 files needing attention

1. <file> — <why, one line>
…

## Optional follow-ons

- <trivially and safely fixable REQUIRED-rule violations, if any>
```
