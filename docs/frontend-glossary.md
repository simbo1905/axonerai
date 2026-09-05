# Frontend Glossary

Terms as the project owner uses them, distilled from the decision log and the
corpus (session rollouts, gists, repo docs). Dependency-ordered: every term is
defined only after its prerequisites. The current architecture statement lives
in [FRONTEND-ARCHITECTURE.md](FRONTEND-ARCHITECTURE.md); the full decision
history lives in [frontend-decisions-log.md](frontend-decisions-log.md).

## Foundations

**vanilla-js** — Plain JavaScript (ES2023-era) ES modules with JSDoc type
annotations checked by `tsc --noEmit`. No TypeScript syntax, no React, no
Babel, no bundler, no build step. The owner's standing rule: the web UI is
vanilla JS, full stop.

**browser-baseline** — Current Chrome, Edge and Safari only (Safari is the
compatibility floor); no Firefox, no legacy browsers. What makes vanilla-js
viable without polyfills.

**tsc-checker-only** — TypeScript used as a checker only, on JavaScript: JSDoc
`@type`/`@param`/`@typedef` annotations, `allowJs`/`checkJs`, `noEmit`. Type
safety without a transpiled language.

**bun-runner-preference** — The owner's stated preference for Bun/bunx as the
JS runner (gists and session text: "unit test with bunx"; node is described as
"cancer ... everything is loaded into memory"). Current repo reality: test
suites are node:test-format `.mjs` run under `bun test`; the preference
governs tool choice elsewhere.

**red-green-tdd** — Write the failing test first, then the code. Applied to
Rust (line-format, pretty-printer), JS (bun suites) and WASM parity tests.

**delegation-process** — How the project is built: one agent per itemNN spec
in `.tmp/itemNN.md` (scratch, never committed); agents implement, verify, and
`git add` but never commit; the orchestrator reviews the diff and commits
(`wip:` prefix until the feature set is complete).

**andon-fact-check** — Emergency andon (行灯): if a user-named version or
dependency does not exist, fact-check before substituting anything; if the
user forgot to publish upstream work, raise a `gh` issue and halt. Never
silently downgrade.

## Typed data

**JTD** — RFC 8927 JSON Type Definition: a JSON-described schema format for
JSON data crossing the wire. Intentionally limited to what mainstream type
systems can express, which makes code generation possible.

**jtd-codegen** — The owner's tool (simbo1905/jtd-wasm) that generates
dependency-free validators from `.jdt.json` schemas. Pinned via `mise` to an
upstream release; `make validators` runs it; `web/generated/` holds the
committed output; a barrel module re-exports `validate` as
`validate<Stem>` names.

**typed-wire-protocol** — Every event crossing any IO boundary has a fixed
`_type` string discriminator backed by its own JTD schema and generated
validator. NOT one big event type with lots of optionality.

**per-event-jtd** — One `.jdt.json` per `_type` under `schemas/`; the `_type`
constant is part of the schema so a generated validator pins it.

**deep-frozen-algebraic-types** — Validated wire values are recursively
frozen (`deepFreeze`) before use: immutable algebraic data. Objects are
DAG-shaped; freezing is cheap and runtime-enforced.

**drop-malformed** — What happens to bad data: no `_type` → silent drop;
unknown `_type` → log as malformed/unsupported, then drop; validator failure
→ log with the validator's `{instancePath, schemaPath}` errors, then drop.
Bad data is logged AND DROPPED at the boundary, never forwarded.

**exhaustive-type-dispatch** — Pure-JS switch on `_type` that dispatches the
frozen, validated value to JSDoc-typed handler methods. Unknown `_type`
values are logged by the drop layer before dispatch, so dispatch never sees
bad data.

**wire-line-protocol** — The streaming wire format: `ts\0_type\0text\n` — a
line-oriented protocol (not JSON-oriented). WS egress for history/catch-up
uses it; the `_type` segment precedes the JSON payload.

**jsonlts-line-format** — Rollout files on disk: append-only lines of
`<epoch_ms>\0<json>\n`. Full fidelity; coreutils-filterable by time without
parsing JSON.

**abridged-egress** — The browser only ever receives tool payloads abridged
to ≤1024 bytes; the rollout keeps everything. Full-vs-abridged is an
invariant, not an optimisation.

**payload-last-field-order** — Serde field order puts the unbounded payload
field LAST in each event so streaming can truncate the payload without
losing metadata.

**wasm-shared-logic** — Logic the browser and server both need (line parsing,
pretty-printing) is written once in Rust, tested Red/Green, and compiled to
WASM for the browser; the native CLI is the parity oracle.

**wasm-pretty-printer** — The WASM module that pretty-prints (possibly
abridged) JSON for rendering in the tool-line component.

**vanilla-pod-perimeter** — The validated pipeline from the owner's earlier
work, generalised here: unknown → generated JTD validator → deep-frozen
algebraic type → JSDoc-typed consumer → render. Trust begins at the
validator.

**contract-triangulation** — Three descriptions of one contract must agree:
JTD schema, generated validator, and a sample/fixture typed in JSDoc. Drift
in any one is caught by a different check (tsc, validator test, runtime
drop).

## Concurrency & storage

**broadcast-channel-bus** — `BroadcastChannel` used as the browser-local
event bus between pages/tabs/workers. Producers tee validated, frozen
envelopes onto it.

**storage-ownership** — Which store owns what: `sessionStorage` for
session-scoped caches (model roster); `localStorage` for small durable UI
state (selected model, MCP toggles by folder); IndexedDB for append-only
history and the console backlog; the server for the rollout and control
plane. Each store has exactly one intended use.

**console-tee** — The page's own `console.log/info/warn/error` are wrapped so
every call is also posted as a validated, frozen envelope to the console
BroadcastChannel. The debug console is a tee of real calls, never a
simulated log.

**worker-spool** — A dedicated Web Worker that receives console envelopes,
persists them into IndexedDB (ring buffer of the newest 2000), and is the
only component that re-broadcasts the durable stream.

**persist-first-broadcast-second** — Producers persist to storage first and
broadcast second; consumers subscribe first and read second. When a worker
persists on a producer's behalf, the worker re-broadcasts after commit
(`tx.oncomplete`, not `request.onsuccess`) and consumers subscribe to that
stream. Verbatim AGENTS.md rule.

**tx-oncomplete-not-onsuccess** — IndexedDB commit visibility begins at
`tx.oncomplete`; `request.onsuccess` is still uncommitted (can abort). The
broadcast fires only at commit.

**second-spooled-channel** — Because the page→worker hop crosses threads,
the consumer's live stream is the worker's post-commit re-broadcast on its
own channel, not the producer's original broadcast.

**window-echo** — `BroadcastChannel` never echoes to the sender's own
context; the producing page additionally echoes through `window` so its own
listeners see its own events.

**subscribe-first-read-second** — A consumer registers on the bus before it
reads its storage snapshot; then live-and-snapshot are gap-free (commit
before the read → in snapshot; after → in live stream; overlap → dedupe).

**dedupe-cheap-belt** — Deduplicate by envelope id. After the topology fix it
absorbs only the overlap case; it is a belt, not the load-bearing mechanism.

**late-arriver** — A screen/tab that opens after events have already flowed:
it subscribes first, then reads storage, then renders the value or a blank
placeholder — never an error. Every UI surface observes shared state this
way.

**ready-handshake** — The worker announces readiness (cold start takes
unbounded time); the bus buffers early envelopes until the handshake or a
timeout, so live delivery never broadcasts into the void.

**frontier-catch-up** — The UI records what it has seen in IndexedDB; on
reboot with `?s=<uuid>` it replays locally, then fetches only lines newer
than its frontier timestamp from the server. The server stays stateless.

**session-log** — A session is an immutable append-only log (time-ordered
UUID v7 id, one jsonlts file); `/rename` is itself an event in the log.

**frontier** — The max timestamp the UI has durably consumed for a session
(lives in IndexedDB); the catch-up cursor.

## UI structure

**domain-specific-client** — The owner's central pattern: for each domain of
state (the model today; other state domains later), ONE non-UI pure-JS ESM
module owns that state — it validates (JTD-generated validators) on load and
save, owns the storage reads/writes, and runs at page load BEFORE any web
component mounts. Components never own state; the composer never talks to
the backend.

**one-state-owner** — Exactly one owner per piece of UI state. Rendering
components receive frozen state or subscribe to events; they never mutate
shared state or call each other.

**components-render-only** — Web components render validated, typed state.
They accept JSDoc-typed parameters and never perform IO themselves.

**pre-mount-init** — The domain client's init (fetch services/roster →
validate → freeze → bootstrap-ensure storage) runs before the web components
are mounted, so components mount into an already-consistent world.

**model-changed-event** — The canonical UI event (`_type="model_changed"`,
UI-private JTD schema, deep-frozen, JSDoc-typed). State changes are
broadcast, not wired panel-to-panel; any UI that wants to render the model
listens for it, validates it, and renders — no spaghetti.

**don't-mix-the-models** — Model identity (provider, model) travels with the
events and is never silently merged across providers; swapping the model is
an explicit, visible act.

**future-frontends-separate-screens** — Future frontends are separate screens
(popup console, future mobile/desktop clients), each with their own clients
and late-arriver logic, composed over the same bus/storage/backends. The
model is the worked example of the pattern.

**bootstrap-ensure** — At startup the domain client loads the roster (from
the server at boot), checks the stored selected model is still in the
roster, and if it is absent or bad replaces it with the first model — so
storage can never pin a stale model.

**storage-cached-roster** — The model roster is fetched once at startup,
validated, frozen and cached (sessionStorage); `/model` reads the cached
immutable state instead of hitting the server; a server-side model change
requires a page bounce, which clears it.

**reusable-picker** — The slash-menu component generalised: the same
component instantiated as the `/` command menu or as an inline model picker.
The right-panel model tree is read-only FYI, never clickable.

**footer-status** — The status footer: `Chat · <model> <provider> · think
off` plus context use `<used>K (<p>%)` against the model's context window.
It observes model_changed (late-arriver safe).

**control-plane-rest** — Settings/slash-command state is a REST control
plane (`/api/*`, openapi.yaml), deliberately separate from the chat data
plane (WS). Slash command results go to the console bus; the prompt is never
sent to the model.

**chat-dataplane-ws** — Live chat traffic flows over the WebSocket as
validated line-format frames.

**builtin-as-mcp-facade** — Built-in server features (Tavily web tools,
context7) are exposed AS MCP tool names so agents can run them, without any
MCP host process.

**config-out-of-gui** — Complex configuration is NOT a GUI feature. jsonc
files under `.axonerai/` are maintained by external tools (`--oneshot`
prompts, skills, the `axonerai-models` bin with backups and JTD
validation). The UI stays unprivileged; enterprise deployments can lock the
folder down.

**ascii-jsdoc-design-docs** — Architecture is documented in code: JSDoc
design points plus ASCII-art diagrams where needed, alongside the markdown
docs (docs drive code; notes are scratch).

**no-fraud-affordances** — Every affordance does what it claims: no
narration-as-action, no dead UI pretending to work, no tool result that lies
about what happened. Enforced by promptfoo evals and the lint skill's rule e.

**conformance-grading** — A read-only lint skill grades each file A/B/C
against the rules. Total conformance is not the objective; sufficient
coverage of essential complexity is. Grading shows where risk concentrates.

## Test policy terms

**decoupling-boundary-test-rule** — "Do not write tests that cross a
decoupling boundary. BroadcastChannel, worker, and IndexedDB boundaries exist
so that each side can be tested alone. Test pure logic in node:test; test
the DOM in a single page. Never orchestrate two pages to simulate a race the
event loop cannot produce. Producers persist first and broadcast second;
consumers subscribe first and read second." (AGENTS.md, verbatim.)

**no-page-handoff-tests** — The architecture rule behind the boundary rule:
pages are decomputed via the event-bus pattern, so there is no cross-page
test surface to build.

**pure-js-tests-over-browser** — With boundary validation and teed logging,
headless `bun test` covers all logic; a browser is only needed when
something actually renders. Corrupt/missing-data unit tests are unnecessary:
the type system plus boundary validation already cover that class.

**per-page-test-suites** — Each screen gets a single-page injection-style
headless suite (`web/test/<page>.headless.mjs` + runner) with stubbed
transport and injected IDB/frozen records; console hygiene (clean console,
deterministic, PASS/FAIL summary).

**poison-tests** — The forbidden class: tests that choreograph multiple
pages/contexts across a decoupling boundary to manufacture races the runtime
cannot produce. Named so they are recognised and deleted, not debugged.

**manual-integration-testing** — What replaces the forbidden suites: pieces
are tested alone; the assembled system is verified by a human picking at the
screen in Chrome in real time.

**japanese-knotweed-frameworks** — The owner's name for creeping framework
code: component-to-component calls, distributed application state in the UI,
script-kiddie React patterns. Named so it is rejected by reflex.
