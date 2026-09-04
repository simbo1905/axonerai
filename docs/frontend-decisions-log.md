# Frontend Architecture Decisions Log

Every architectural decision point the corpus yields, in time order. Later
decisions **overwrite** earlier ones: superseded entries are struck through
and their current successor is stated. The log states the CURRENT invariant
first in each entry, and records what it replaced where it changed.

Sources: session rollouts are quoted from user-role text in the axonerai
orchestrator session (`ses_fa2372257ffeyZ0X4GmoYrkj35`, 2026-09-01 → 2026-09-04);
gists are the owner's public gists; repo docs are AGENTS.md, README.md and
docs/ARCHITECTURE.md. Dates are the session timestamps of the source messages.

Two item55-spec entries could not be verified in the corpus and are listed in
the appendix ("Spec-listed, not corpus-verified") rather than asserted here.

---

## Standing conventions (pre-repo, carried in from the owner's gists)

**D01 — vanilla-js + JSDoc + `tsc --noEmit` (2026-04 → standing).**
Current: browser code is vanilla ES2023-era modules with JSDoc types; TS is a
checker only (no `.ts`, no emit); Chrome/Edge/Safari baseline, Safari is the
floor. Evidence: gist `1f7da3c9` (vanilla-js-jsdoc skill); session: "you must
use vanillajs for this" (2026-09-01); AGENTS.md "Web UI conventions".
Replaced: nothing earlier in this repo (the first-cut web UI did have
React/Babel — see D14).

**D02 — JTD-generated validators at every IO boundary (2026-04 → standing).**
Current: everything crossing IO — wire, storage, BroadcastChannel, workers,
fetches — is described by a JTD schema and validated by a generated validator
before use. Evidence: gists `c093d377` (Vanilla Pod JS), `e9ff1159` (view ==
JTD == validator == JSDoc invariant); jtd-wasm skill; AGENTS.md.

**D03 — validate → freeze → project → render perimeter (2026-07-10).**
Current: unknown JSON is validated, recursively frozen, and only then handed
to JSDoc-typed consumers; presentation shapes are derived by pure map/flatMap,
never taught to transport objects. Evidence: gist `c093d377`; repo
`web/src/wire.mjs` (validate + deepFreeze).

**D04 — Bun/bunx preferred JS runner (2026-07, stated 2026-09-03).**
Current in the owner's other projects: Bun as the runner and test harness
(Vanilla Pod: "Bun tests"); stated here as "unit test with bunx" (2026-09-03).
Repo reality: the axonerai suites run under `node:test` and headless Chrome
runners. Recorded as a standing preference that this repo has not adopted.

**D05 — Red/Green TDD for shared logic (standing).**
Current: failing test first for line-format parsing, pretty-printing, wire
validation, storage helpers. Evidence: session (2026-09-02, WASM
pretty-printer "Red/Green TDD thing"), item29 spec.

**D06 — docs drive code; notes are scratch (standing, stated 2026-09-04).**
Current: architectural decisions live in markdown that is committed
(AGENTS.md, docs/ARCHITECTURE.md, feature spec mds); scratch notes and item
specs live in `.tmp/` and are never committed; the UI architecture is also
documented in code (JSDoc design points, ASCII-art diagrams where needed).
Evidence: session (2026-09-04: "write the design points up of the model in the
js doc and in the code as ascii art… the UI architecture will be documented in
code"); AGENTS.md scratch rules; D42 (the mandate that produced this log).

---

## Phase 1 — repo boot (2026-09-01)

**D07 — drop Anthropic; provider roster from jsonc+JTD config (2026-09-01).**
Current: providers = Mistral, Groq, OpenCode Zen, OpenCode Go; a single
OpenCode key covers Zen and Go; config in `.axonerai/axonerai.jsonc` (else
`~/.config/axonerai/…`) naming per provider the endpoint, env key, model list,
thinking support/levels; config wins over built-in defaults. Superseded:
Anthropic provider (deleted).
Evidence: session (drop anthropic; "the config should look in .axoneria/
… jsonc … for each provider, name the env var … the list of models … if they
support thinking").

**D08 — the web UI is vanilla; delete React/Babel (2026-09-01).**
Current: vanilla JS only. Superseded: the initial React/Babel web UI —
deleted via the deleting-dead-code discipline ("CLEARLY WE ARE DELETING THE
React/Babel SO WE ARE NOT MAINTAINING IT"), then README updated to state the
vanilla approach.
Evidence: session 2026-09-01; README "Web UI Demo".

**D09 — per-event JTD wire protocol with `_type` enum constants (2026-09-01).**
Current: every WS event gets its own `.jdt.json` whose `_type` is a fixed
enum constant; `jtd-codegen` (mise-pinned from the upstream release) generates
`.mjs` validators wired through a barrel; Rust-side dev tests validate serde
samples. Superseded: "one complex event type" with optionality (explicitly
rejected). Evidence: session 2026-09-01; schemas/*.jdt.json; AGENTS.md.

**D10 — runtime safety tested outside the browser (2026-09-01).**
Current: wire plumbing (validate/freeze/drop/dispatch) is testable as pure JS
with JSDoc before a browser ever boots; `tsc --noEmit` aligns JSDoc types with
the JTD shapes. Evidence: session 2026-09-01; later codified as ARCHITECTURE
Decision 3.

**D11 — subagent delegation as the build process (2026-09-01).**
Current: itemNN specs in `.tmp/`, one agent per item, agents add/never commit,
orchestrator reviews and commits. Evidence: session; gist `1366172a`;
AGENTS.md.

**D12 — Emergency Andon for version fact-checks (2026-09-01).**
Current: fact-check before substituting; halt + upstream issue if the user
forgot to publish; never silently downgrade. False alarm on jtd-wasm resolved
the same day; the rule stayed. Evidence: session; AGENTS.md.

---

## Phase 2 — sessions, rollouts, streaming (2026-09-02)

**D13 — sessions are immutable UUID logs; `/rename` is an event (2026-09-02).**
Current: session id is a time-ordered UUID v7 (supersedes `sess_${xxx}`-style
ids); the rollout is an append-only jsonlts file; renaming writes a
`SESSION_RENAME` event; replay on load. Evidence: session 2026-09-02.

**D14 — jsonlts line format (2026-09-02).**
Current: `<epoch_ms>\0<json>\n`; `\0` between timestamp and JSON, `\n`
between records; time-filterable with coreutils without JSON parsing.
Superseded: plain JSONL rollout ("too expensive to parse to do offset").
Evidence: session 2026-09-02.

**D15 — full-vs-abridged invariant (2026-09-02).**
Current: the rollout keeps full-fidelity payloads; the browser receives
tool payloads abridged at ≤1024 bytes — on the live stream AND on session
load. Superseded: sending full payloads to the UI. Evidence: session
2026-09-02; README; AGENTS.md.

**D16 — payload-last field order (2026-09-02).**
Current: serde field order puts the unbounded payload last so streaming can
truncate the payload while metadata (tool, duration, bytes, ts) stays
complete. Evidence: session 2026-09-02 (manual-Serialize ordering plan,
adopted).

**D17 — line-oriented wire streaming `ts\0_type\0text` (2026-09-02).**
Current: the server streams lines, not JSON envelopes; the browser parses the
line format (WASM) and validates/dispatches locally. The server does no
payload parsing beyond linear scans; stateless session loading. Superseded:
server-side event shaping/parsing. Evidence: session 2026-09-02 ("the server
should not do anything about parsing and work on linear reads").

**D18 — WASM-shared line logic with Red/Green TDD (2026-09-02).**
Current: line-format and pretty-print logic written once in Rust, tested Red/
Green, compiled to WASM for the browser; committed glue under web/assets/.
Evidence: session 2026-09-02; `make wasm-lineformat`, `make wasm-pretty`.

**D19 — control plane REST / chat data plane WS (2026-09-02).**
Current: slash-command/settings state is REST (`/api/*`, openapi.yaml);
live chat is the WebSocket; the prompt for slash commands is never sent to
the model. Superseded: routing settings flows through the chat stream.
Evidence: session 2026-09-02 ("control plane not data plane"); README
"Control plane"; AGENTS.md.

**D20 — side panel + slash menu (2026-09-02).**
Current: collapsible right-hand panel (Context/MCP/LSP/Todo/Models/Slash/
Built-ins trees, terminal-style fonts) and a composer `/` menu (arrows/esc/
enter). Evidence: session 2026-09-02; README.

**D21 — verbose tool-line rendering (2026-09-02).**
Current: tool_call events render as dim terminal-style single lines with
bytes/duration/ts, expandable to the pretty-printed abridged payload; hidden
unless verbose is on; re-render toggles cover history too. Evidence: session
2026-09-02.

**D22 — built-in features exposed as an MCP facade (2026-09-02).**
Current: no MCP host process; Tavily (and later context7) appear as MCP
tools when their env key is present; per-session MCP/tool toggles persist to
`.axonerai/settings.jsonc`. Evidence: session 2026-09-02 and 2026-09-04
("USING BUILT IN FEATURES BUT EXPOSING THEM AS MCP"); README.

**D23 — build-time prompt composition (2026-09-02).**
Current: one true base prompt + per provider/model patch files (PREPEND/
APPEND/REPLACE) composed at build time into committed generated prompts; the
server loads the provider+model prompt if present else the default. No
runtime prompt bending. Evidence: session 2026-09-02; README "System
Prompts".

---

## Phase 3 — devtools console, test policy (2026-09-02 evening)

**D24 — console output moves to a devtools-style popup (2026-09-02).**
Current: a `/console` popup screen (separate page, own components) shows the
tee'd console stream; slash-command results go to the console bus. Superseded:
slash-command output rendered inside the panel tree (retired; the panel keeps
only the invocation echo). Evidence: session 2026-09-02.

**D25 — console teeing to the debug console (2026-09-02).**
Current: `console.log/info/warn/error` are wrapped to tee validated, frozen
envelopes to the console bus; the tee is real calls, never a simulated log.
Evidence: session 2026-09-02; AGENTS.md "Console-bus convention".

**D26 — worker spool + late-arriver design for the console (2026-09-02).**
Current: one worker reads the console bus and spools to IndexedDB (ring
buffer, newest 2000); the popup is a late arriver: subscribe, read the
backlog, render, then go live. Evidence: session 2026-09-02 ("so then we
need late arrivers").

**D27 — decoupling-boundary test rule (2026-09-02, codified in AGENTS.md
2026-09-03).**
Current, verbatim: "Do not write tests that cross a decoupling boundary.
BroadcastChannel, worker, and IndexedDB boundaries exist so that each side
can be tested alone. Test pure logic in node:test; test the DOM in a single
page. Never orchestrate two pages to simulate a race the event loop cannot
produce. Producers persist first and broadcast second; consumers subscribe
first and read second." Superseded: cross-page integration choreography
tests. Evidence: session; AGENTS.md.

**D28 — poison-test deletion (2026-09-03).**
Current: forbidden test code is deleted, not debugged; the discipline is
"delete the forbidden approach, git add, commit" (commit "wip removed
forbidden poison tests"). Evidence: session 2026-09-03.

**D29 — worker re-broadcast after commit on a second channel (2026-09-03).**
Current: the console page→worker hop crosses threads, so the consumer's live
stream is the worker's own channel, re-broadcast from inside
`tx.oncomplete` (not `request.onsuccess`). Superseded: the item32 first-cut
topology where the page broadcast and the worker persisted (a silent-drop
window between broadcast, subscription and commit); and broadcasting after
`request.onsuccess`. Evidence: session 2026-09-03 (adopted analysis + approved
mermaid design); AGENTS.md delegation sentence; README.

**D30 — `tx.oncomplete` commit-visibility rule (2026-09-03).**
Current: the durable broadcast fires at transaction commit; `onsuccess` may
still abort. Part of D29 but called out because it is a distinct semantic.
Evidence: session 2026-09-03; AGENTS.md.

**D31 — dedupe-by-id demoted to a cheap belt (2026-09-03).**
Current: dedupe absorbs only the overlap case (commit before snapshot read,
re-broadcast task after); the ordering rules carry the correctness.
Superseded: dedupe as the load-bearing mechanism. Evidence: session
2026-09-03.

**D32 — window echo for the sender (2026-09-03).**
Current: BroadcastChannel never echoes to the sender's own context, so the
producing page echoes through `window` for its own listeners. Evidence:
session 2026-09-03 (item32 alignment); AGENTS.md console-bus convention.

**D33 — ready handshake survives (2026-09-03).**
Current: the worker announces readiness; the bus buffers early envelopes
(handshake or timeout flush) so cold-start posts are not broadcast into the
void. This survived the poison-test deletion. Evidence: session 2026-09-03.

---

## Phase 4 — model state (2026-09-03 → 2026-09-04)

**D34 — footer status line (2026-09-03).**
Current: footer shows `Chat · <model> <provider> · think off` and context
use `<used>K (<p>%)` over the model's context window. Evidence: session
2026-09-03; README.

**D35 — /models swap + /model read from cached state (2026-09-03 → 04).**
Current: `/models` swaps the model (POST `/api/model`, validated against the
config roster); `/model` reads the startup-cached immutable roster state.
Superseded: the original `/model` read command (retired when the footer took
over display); then the panel-tree-as-switcher (not clickable, FYI only).
Evidence: sessions 2026-09-03/04; README.

**D36 — model config as a JTD-validated array + external maintenance bin
(2026-09-03).**
Current: per-provider models config (`.axonerai/models/<provider>-models.jsonc`,
`~/.axonerai/models` fallback, local masks user) with context windows, costs,
offers; the `axonerai-models` bin mutates only via backup → JTD validate →
write. Superseded: hand-edited config without validation. Evidence: session
2026-09-03 (model context/costs, backups, "offer"); item41 spec.

**D37 — config-out-of-GUI (2026-09-03, codified as ARCHITECTURE Decision 1).**
Current: complex config belongs in external tools and `--oneshot` prompts
with skills — never a GUI config feature; the UI stays unprivileged and the
`.axonerai` folder can be locked down read-only. Evidence: session
2026-09-03; docs/ARCHITECTURE.md Decision 1.

**D38 — `model_changed` as the canonical UI event (2026-09-04).**
Current: a UI-private `_type="model_changed"` event (own jdt.json, deep-
frozen, JSDoc-typed) is the ONLY channel by which UI surfaces learn the
selected model; components listen, validate, render. Superseded: direct
panel-to-panel / component-to-component state calls ("spaghetti").
Evidence: session 2026-09-04; docs/ARCHITECTURE.md Decision 2.

**D39 — domain-specific-client ownership (2026-09-04).**
Current: the model domain has ONE owner — a pure-JS ESM "domain specific
client" that saves/loads the selection, owns the JTD-validated IO against
storage, initialises state from the roster, and runs at page load BEFORE any
web component mounts; components render only; the composer never talks to
the backend. Superseded: input control calling the backend with the model
name ("distributed application state in the UI"). Evidence: session
2026-09-04; docs/FRONTEND-ARCHITECTURE.md.

**D40 — storage-cached roster + bootstrap-ensure (2026-09-04).**
Current: the roster is fetched at startup, validated, frozen, cached in
sessionStorage; the selection is durable in IndexedDB; startup ensures the
stored selection is in the roster, else first model. Evidence: session
2026-09-04.

**D41 — don't-mix-the-models; future frontends as separate screens
(2026-09-04).**
Current: model identity is never silently merged across providers; future
frontends are separate screens, each with their own domain clients, over the
same bus/storage/backends. The model is the pattern's worked example.
Evidence: session 2026-09-04.

**D42 — "name all things" documentation mandate (2026-09-04).**
Current: the architecture record must name every decision point, not just
recently restated ones — the owner builds from memory and is consistent
because these are the invariants; later decisions overwrite earlier ones in
the log. Evidence: session 2026-09-04 ("the docs must be all the
architectural descision points … these are all the invariants of my mind").

**D43 — no fraud affordances (2026-09-02 → 04).**
Current: every affordance does what it claims; fake/simulated tool results or
dead UI are fraud; slash commands get promptfoo proof that the agent does the
right thing. Evidence: sessions 2026-09-02/03 (fraud audit item); lint skill
rule e.

---

## Codified terminal state (2026-09-03/04)

**D44 — ARCHITECTURE Decisions 2 and 3 (2026-09-04).**
Current: Decision 2 — no page-handoff tests; boundary-validated frozen
algebraic data everywhere; exhaustive `_type` dispatch that logs unknown
types. Decision 3 — pure JS tests over browser tests when there is no
rendering; corrupt/missing-data unit tests unnecessary. Evidence:
docs/ARCHITECTURE.md; sessions as cited above.

**D45 — conformance grading, not 100% (2026-09-04).**
Current: a read-only lint skill grades each file A/B/C against rules a–e;
total conformance is not the objective. Evidence: docs/ARCHITECTURE.md Part 3;
.axonerai/skills/lint.

---

## Appendix — spec-listed, not corpus-verified

The item55 spec listed two decision points that could not be found in the
mined corpus (all axonerai sessions, repo docs, web/src, or gists). They are
recorded here so nothing is silently dropped, but they are NOT stated as
current invariants anywhere in these documents:

- "services model (N services from env keys, zen/go one-key gloss, disabled
  marking)" — no axonerai session, repo doc, or source file mentions a
  services model. Plausibly a planned/not-yet-spoken feature.
- "recency maps per service" — the word "recency" appears nowhere in the
  axonerai corpus or the repo's web/src.

If these are spoken decisions from a conversation this corpus does not
contain, they should be added once their source is provided.
