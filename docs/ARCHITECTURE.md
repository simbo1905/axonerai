# Architecture

This is the project's canonical architecture statement: the WHY, the test
strategy code review enforces, and how conformance is graded. It is not a
working-rules document for agents — see [AGENTS.md](../AGENTS.md) for those.
Operational details (running, providers, slash commands, rollouts) live in the
[README](../README.md).

## Part 1 — Project thesis

The major agent harnesses are memory-bloated TypeScript/JavaScript with vast
installs, largely because they bundle a sandbox in the commercial version.
AxonerAI deliberately does not compete on those terms.

It optimises for two things:

### Low runtime overhead

The target is hundreds of thousands of concurrent agents on a SINGLE
commodity VPS host. Every layer is chosen to serve that:

- **Rust server** — one release binary, no runtime, no VM, no GC.
- **Vanilla-JS browser layer** — plain ES modules with JSDoc annotations; no
  React, no Babel, no bundler, no build step (README: "Web UI Demo").
- **Strict JTD validation at every boundary** — one JTD schema per wire event
  in `schemas/*.jdt.json`; generated validators in `web/generated/`; data
  coming off the wire is validated and deep-frozen before use (AGENTS.md:
  "Web UI conventions").
- **Append-only line-format rollouts** — full-fidelity
  `<epoch_ms>\0<json>` traces on disk (README: "Sessions & Rollouts").
- **Abridged egress** — the browser only ever receives ≤1024-byte abridged
  tool payloads; the rollout keeps everything.

### More security

Agents can run on a REMOTE HOST without the API keys being present there.
Planned: a keyless proxy in the style of the codex rust-cli proxy, proxied
against the Mistral and OpenCode endpoints — the VPS runs the agents; the
keys stay with the client.

### Configuration stays out of the GUI

Config maintenance is deliberately NOT a GUI feature. Per-provider jsonc
files under `.axonerai/` are committable and shareable, and are maintained
through external tools: `--oneshot` prompts, skills, and the
`axonerai-models` bin, which writes with automatic backups and validates
against the JTD schemas. Consequences:

- The UI needs no privileged config features.
- In an enterprise deployment the `.axonerai` folder can be locked down
  read-only; nothing in the UI needs write access to it.

This decision is load-bearing for the test strategy — see Decision 1 below.

## Part 2 — Test strategy

**Required approach for code review.** Reviewers must hold new code to these
three decisions. They are the reason the test suite is small and stays small.

### Decision 1 — No complex config in the GUI

Complex config matters belong in external tools and `--oneshot` prompts with
skills — with backups and `jdt.json` validators — so we never have to test
complex graphical config features, and the UI stays unprivileged
(enterprise-safe `.axonerai` lockdown).

If a review comment would add config UI, route it through the external
tooling instead. There is no graphical-config test surface to build or
maintain because there is no graphical config feature.

### Decision 2 — No page-handoff tests

Each page/screen is decomposed via browser event-bus patterns with
late-arriver support. Everything crossing IO — session storage, IndexedDB,
WebSockets, fetches, BroadcastChannel, workers — is JTD-validated,
deep-frozen, immutable algebraic data with a consistent event `_type`
discriminator (`src/wire.rs`, `schemas/*.jdt.json`, `web/src/wire.mjs`).
Dispatch is an exhaustive match that logs unknown event types
(`web/src/dispatch.mjs`); JSDoc-typed methods consume the validated value
objects; web components accept those same types. Bad data is **logged AND
DROPPED at the boundary, never forwarded**.

Without TypeScript we still get full type checking: `tsc --noEmit` at author
time plus runtime JTD validation at every boundary. This removes an entire
class of bugs, which is why we do not integration-test across the
decoupling boundaries — the AGENTS.md rule ("Do not write tests that cross a
decoupling boundary") is the enforcement: BroadcastChannel, worker, and
IndexedDB boundaries exist so each side can be tested alone; producers
persist first and broadcast second; consumers subscribe first and read
second.

### Decision 3 — Pure JS tests over browser tests when there is no rendering

With first-class logging (`console.log`/`console.error` teed to the debug
console via `web/src/console-bus.mjs`) and boundary validation, headless
`bun test` suites (node:test-format sources) cover all logic. A browser is only
needed when something
actually renders. Corrupt/missing-data unit tests are unnecessary: the type
system plus boundary validation already handle those classes of defect.

## Part 3 — Conformance grading

A linting skill instructs a cheap agent to review all code and GRADE each
file for conformance with the rules above. The intent, stated honestly:

**TOTAL conformance is not the objective.** The objective is SUFFICIENT
coverage of the essential complexity, while `tsc --noEmit` type checking and
this architecture (boundary validation, immutable frozen data, exhaustive
`_type` dispatch) eliminate large classes of defects outright. Grading is a
signal for where the remaining risk concentrates, not a score to drive to
100.
