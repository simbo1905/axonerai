# AGENTS.md

Working rules for agents operating in this repository.

## Tooling

- `mise.toml` pins project tools (e.g. `jtd-codegen` from simbo1905/jtd-wasm releases).
  Run tools through mise: `mise exec -- jtd-codegen --target js <schema>`.
- The `jtd-codegen` pin uses the plain github backend form
  `"github:simbo1905/jtd-wasm" = "release-0.3.0"` — mise autodetects the correct
  release asset (asset names must follow `<bin>_<version>_<rust-triple>.<ext>` with a
  bare binary at the archive root) and verifies GitHub attestations, so no
  `matching`/`version_prefix`/`asset_pattern` options are needed.
- Scratch files go in `.tmp/` (gitignored). **Avoid `/tmp`** — it trips sandbox
  permission errors on this host; use `.tmp/` for all scratch output.

## Web UI conventions

- The web UI is vanilla JavaScript (ES modules) with JSDoc type annotations checked
  by `tsc --noEmit`. No TypeScript syntax, no React, no Babel, no bundler, no build step.
- Wire protocol events are defined in `src/wire.rs` (serde tag `_type`) with one JTD
  schema per event in `schemas/*.jdt.json`. Generated validators live in
  `web/generated/` (see the `validators` Makefile target). Data coming off the wire
  in the browser must be deep-frozen and JTD-validated before use.

## Task delegation process

Major tasks are delegated to subagents to keep the orchestrator's context lean:

1. Number each todo item as `item00`, `item01`, … and write its spec to
   `.tmp/itemNN.md` before launching the agent.
2. Launch each agent (one spec per agent) with the work to do. The agent must:
   - implement per the spec,
   - verify its work is green (run the relevant tests/builds),
   - `git add` its changes, but **never `git commit`**.
3. On the agent's return it reports whether the work was fully done or lists
   follow-on work. The orchestrator must then:
   - mark the todo item done,
   - add any follow-on work as new todo items,
   - review the diff (`git status`, `git diff --cached`),
   - if the code is green with respect to the current tests (the current TDD bar),
     `git commit`. Use the message prefix `"wip: <summary>"` while the full feature
     set is not yet complete; use a normal message once it is.

## Todo list ordering

- New items the user adds while work is in progress go to the **bottom** of the
  todo list by default, to be done last — unless the user says to do them next,
  or to do them before/after a specific existing item.
- To insert an item at a position, use UK library book filing (Dewey decimal)
  numbering: to file a new book between books 1 and 2 you call it **1.5**. So items
  are numbered 1, 2, 3, … and an insertion between 1 and 2 becomes 1.5 (between 1
  and 1.5 becomes 1.25, and so on — 1.2.3.4-style nesting is allowed). This lets any
  issue be inserted at an exact position in the list without renumbering.

## Emergency Andon (version fact-check)

If an agent finds that a version or dependency the user specified does not exist
(e.g. not on crates.io, not in a registry, not in a release feed), it must fact-check
before substituting anything:

1. Confirm the actual latest published version(s) of the named package.
2. Check the user's own upstream repos for newer work that exists locally but was
   never published.
3. If the user appears to have forgotten to publish their latest upstream work:
   raise a `gh` issue against that upstream repo describing exactly what is missing
   and what downstream needs, then **halt**. The user goes and publishes; downstream
   work resumes only against the latest published version.
4. Never silently downgrade to an older third-party lookalike just to keep moving.

