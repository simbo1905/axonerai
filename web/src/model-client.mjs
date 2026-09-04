// @ts-check
/**
 * ModelsClient — the domain specific client for the model domain (item58,
 * D38/D39/D40). ONE owner of the model domain's state: it fetches and
 * caches the services roster, reads the server-owned selection, builds and
 * owns the localStorage recency maps, OWNS the POST /api/model swap, and is
 * the ONLY producer of the canonical UI event `model_changed`. It runs at
 * page load BEFORE any web component mounts; components render only.
 *
 * The ASCII design encoded by this module:
 *
 * ```
 * ┌─────────────┐  GET /api/services   ┌──────────────── ModelsClient ────────────────┐
 * │   server    │  GET /api/state      │  roster:  sessionStorage (frozen cache)      │
 * │  (REST)     │  POST /api/model     │  current: {service, model}   ← from state    │
 * └─────┬───────┘                      │  recency: localStorage map per service       │
 *       │        ┌──────────────┐      │            (model → natural number)          │
 *       └───────►│ init() pre-  │◄─────┘  select(service, model):                     │
 *                │ mount, boot- │         1. POST /api/model            (owns REST)   │
 *                │ strap-ensure │         2. persist recency map (tx-like: local-     │
 *                └──────┬───────┘            Storage first)                            │
 *                       │ 3. emit model_changed (validate → deepFreeze)                │
 *                       ▼                                                              │
 *         BroadcastChannel('axonerai') + window echo  ──►  console tee (console bus)   │
 *                       │                                                              │
 *                       ▼                                                              │
 *         consumers: subscribe FIRST, then read getState() → render or blank          │
 * └──────────────────────────────────────────────────────────────────────────────────────┘
 * ```
 *
 * Storage ownership (docs/FRONTEND-ARCHITECTURE.md §(e), model-domain rows):
 *
 * | Store                | Owned by    | Holds                              | Lifetime     |
 * |----------------------|-------------|------------------------------------|--------------|
 * | sessionStorage       | this client | frozen roster cache                | per tab      |
 * | localStorage         | this client | recency map, one key per service   | durable      |
 * | server /api/state    | server      | current {service, model} selection | per server run |
 *
 * The current selection is deliberately NOT persisted locally: the server's
 * /api/state is the owner and late arriviers GET it at init. The
 * console-bus tee happens through `console.info` — the installed console
 * bus wraps the page console, so this module never imports console-bus.mjs
 * (which is browser-hostile via its `/generated/...` import) and the tee
 * cannot drift from what actually ran.
 *
 * Every value crossing an IO boundary (fetch response, sessionStorage,
 * localStorage, BroadcastChannel, window echo) is validated and
 * deep-frozen; malformed data is logged and dropped, never forwarded.
 *
 * Node-testable: all browser/IO access flows through the {@link Deps}
 * injection accepted by {@link init} (and the module defaults resolve to
 * the real globals in the browser).
 */
import { deepFreeze } from "./wire.mjs";
import { validateModel_changed } from "../generated/validators.mjs";

/**
 * BroadcastChannel name shared by the model client's producer channel and
 * every subscriber channel (the ASCII bus: `BroadcastChannel('axonerai')`).
 */
export const MODEL_CHANNEL = "axonerai";

/** Window event type used for the sender-side echo (BC never echoes to sender). */
export const MODEL_CHANGED_EVENT = "model_changed";

/** sessionStorage key holding the frozen roster cache (JSON array). */
export const ROSTER_CACHE_KEY = "agt.model-roster";

/** Prefix of the per-service localStorage recency key. */
export const RECENCY_KEY_PREFIX = "agt.model-recency:";

/**
 * One row of a service's model roster (as served by GET /api/services).
 *
 * @typedef {object} ModelRow
 * @property {string} id model id used in API calls
 * @property {string} display human name
 * @property {number} context_window context window in tokens
 */

/**
 * One entry of the GET /api/services payload (item57).
 *
 * @typedef {object} ServiceEntry
 * @property {string} service the service short name
 * @property {boolean} enabled not named in the settings disabled_services
 * @property {boolean} connected API key resolves
 * @property {readonly ModelRow[]} models the service's roster
 */

/** Roster indexed by service name (frozen). */
/** @typedef {Record<string, Readonly<ServiceEntry>>} Roster */

/** One service's recency map: model id → natural-number rank. */
/** @typedef {Record<string, number>} RecencyMap */

/** Recency maps indexed by service name. */
/** @typedef {Record<string, Readonly<RecencyMap>>} Recency */

/**
 * The frozen world a component renders from. Built fresh per getState()
 * call (the recency copy is frozen per call so internal maps stay mutable).
 *
 * @typedef {object} StateSnapshot
 * @property {string | null} service current service (server-owned truth)
 * @property {string | null} model current model (server-owned truth)
 * @property {Roster} roster roster by service
 * @property {Recency} recency recency maps by service
 */

/**
 * The canonical UI-private `model_changed` event
 * (`web/schemas/model_changed.jdt.json` — D38).
 *
 * @typedef {object} ModelChangedEvent
 * @property {"model_changed"} _type
 * @property {string} service
 * @property {string} model
 * @property {string} ts ISO timestamp string
 */

/**
 * Result of {@link select}: on failure the error is returned to the caller
 * (the composer surfaces it) and NO state changes.
 *
 * @typedef {object} SelectResult
 * @property {boolean} ok
 * @property {Readonly<ModelChangedEvent>} [event] on success
 * @property {string} [error] on failure
 */

/**
 * Minimal KV storage surface satisfied by both real DOM Storage objects
 * and the in-memory test fakes.
 *
 * @typedef {object} KvStorage
 * @property {(key: string) => string | null} getItem
 * @property {(key: string, value: string) => void} setItem
 * @property {(key: string) => void} [removeItem]
 */

/**
 * Minimal window surface used for the sender-side echo. Satisfied by the
 * real `window` and by the test fake.
 *
 * @typedef {object} EchoWindow
 * @property {(type: string, listener: (event: { type: string, detail?: unknown }) => void) => void} addEventListener
 * @property {(type: string, listener: (event: { type: string, detail?: unknown }) => void) => void} [removeEventListener]
 * @property {(event: { type: string, detail?: unknown }) => boolean} [dispatchEvent]
 * @property {new (type: string, init?: { detail?: unknown }) => { type: string, detail?: unknown }} [CustomEvent]
 */

/**
 * Minimal BroadcastChannel surface satisfied by the real DOM
 * BroadcastChannel and by the in-memory test fake.
 *
 * @typedef {object} BroadcastChannelLike
 * @property {((event: any) => void) | null} onmessage
 * @property {(data: unknown) => void} postMessage
 * @property {() => void} close
 */

/**
 * Injectable IO dependencies (all required on a RESOLVED Deps — init()
 * merges {@link InitOptions} over the real browser globals). Tests inject
 * in-memory fakes (whole-unit tests — the boundaries are faked, never
 * orchestrated).
 *
 * @typedef {object} Deps
 * @property {((input: string, init?: { method?: string, headers?: Record<string, string>, body?: string }) => Promise<{ ok: boolean, status?: number, json: () => Promise<any> }>)} fetch
 * @property {{ new (name: string): BroadcastChannelLike }} BroadcastChannel
 * @property {KvStorage | null} localStorage
 * @property {KvStorage | null} sessionStorage
 * @property {EchoWindow | null} window
 * @property {() => string} now ISO timestamp source (tests pin it)
 */

/** @typedef {Partial<Deps>} InitOptions */

/** @typedef {Deps} ResolvedDeps */

/** @type {Deps | null} */
let deps = null;
/** @type {Roster} */
let roster = {};
/** @type {{ service: string | null, model: string | null } | null} */
let current = null;
/** @type {Record<string, RecencyMap>} */
let recency = {};
/** @type {BroadcastChannelLike | null} */
let postChannel = null;
/** @type {Promise<void>[]} */
let pending = [];

/**
 * Resolve the default dependency set from the real globals (browser).
 *
 * @returns {Deps}
 */
function defaultDeps() {
  const g = /** @type {any} */ (globalThis);
  return {
    fetch: typeof g.fetch === "function" ? g.fetch.bind(g) : /** @type {any} */ (undefined),
    BroadcastChannel: g.BroadcastChannel,
    localStorage: g.localStorage ?? null,
    sessionStorage: g.sessionStorage ?? null,
    window: /** @type {EchoWindow | null} */ (g.window ?? null),
    now: () => new Date().toISOString(),
  };
}

/**
 * Validate the GET /api/services payload (an array of {@link ServiceEntry}).
 * Malformed entries are logged and skipped; a non-array payload returns
 * null (drop the whole thing, log). There is deliberately no server wire
 * schema for this REST payload — the guard lives here, in its only reader.
 *
 * @param {unknown} data parsed JSON body
 * @returns {ServiceEntry[] | null}
 */
function validateRoster(data) {
  if (!Array.isArray(data)) return null;
  /** @type {ServiceEntry[]} */
  const out = [];
  for (const entry of data) {
    const e = /** @type {any} */ (entry);
    if (
      entry === null ||
      typeof entry !== "object" ||
      Array.isArray(entry) ||
      typeof e.service !== "string" ||
      e.service === "" ||
      typeof e.enabled !== "boolean" ||
      typeof e.connected !== "boolean" ||
      !Array.isArray(e.models)
    ) {
      console.error("[model-client] malformed /api/services entry (dropped)", entry);
      continue;
    }
    /** @type {ModelRow[]} */
    const models = [];
    for (const row of e.models) {
      const m = /** @type {any} */ (row);
      if (
        row === null ||
        typeof row !== "object" ||
        typeof m.id !== "string" ||
        m.id === "" ||
        typeof m.display !== "string" ||
        typeof m.context_window !== "number" ||
        !Number.isFinite(m.context_window)
      ) {
        console.error("[model-client] malformed roster model row (dropped)", row);
        continue;
      }
      models.push({ id: m.id, display: m.display, context_window: m.context_window });
    }
    out.push({ service: e.service, enabled: e.enabled, connected: e.connected, models });
  }
  return out;
}

/**
 * Fetch GET /api/services and return the validated, frozen roster array —
 * or null when the response is malformed/unavailable (logged, dropped).
 *
 * @param {Deps} d
 * @returns {Promise<Readonly<ServiceEntry[]> | null>}
 */
async function fetchServices(d) {
  if (!d.fetch) {
    console.error("[model-client] no fetch available for /api/services");
    return null;
  }
  try {
    const res = await d.fetch("/api/services");
    if (!res.ok) {
      console.error("[model-client] GET /api/services failed", res.status);
      return null;
    }
    const entries = validateRoster(await res.json());
    if (!entries) {
      console.error("[model-client] malformed /api/services payload (dropped)");
      return null;
    }
    return deepFreeze(entries);
  } catch (error) {
    console.error("[model-client] GET /api/services fetch failed", error);
    return null;
  }
}

/**
 * Fetch GET /api/state and extract the server-owned selection
 * `{service, model}` — or null when malformed/unavailable (logged, dropped).
 *
 * @param {Deps} d
 * @returns {Promise<Readonly<{ service: string, model: string | null }> | null>}
 */
async function fetchState(d) {
  if (!d.fetch) {
    console.error("[model-client] no fetch available for /api/state");
    return null;
  }
  try {
    const res = await d.fetch("/api/state");
    if (!res.ok) {
      console.error("[model-client] GET /api/state failed", res.status);
      return null;
    }
    const data = await res.json();
    if (data === null || typeof data !== "object" || typeof data.service !== "string") {
      console.error("[model-client] malformed /api/state payload (dropped)", data);
      return null;
    }
    const model =
      typeof data.model === "string" && data.model !== "" ? data.model : null;
    return deepFreeze({ service: data.service, model });
  } catch (error) {
    console.error("[model-client] GET /api/state fetch failed", error);
    return null;
  }
}

/**
 * Install a validated roster as the module's frozen roster-by-service.
 *
 * @param {Readonly<ServiceEntry[]>} entries
 * @returns {void}
 */
function applyRoster(entries) {
  /** @type {Record<string, Readonly<ServiceEntry>>} */
  const byService = {};
  for (const entry of entries) byService[entry.service] = entry;
  roster = deepFreeze(byService);
}

/**
 * Read the sessionStorage roster cache: on a hit, validate + install it and
 * return true. A malformed cache is logged and ignored (rebuilt at boot).
 *
 * @param {Deps} d
 * @returns {boolean}
 */
function readCachedRoster(d) {
  if (!d.sessionStorage) return false;
  let raw;
  try {
    raw = d.sessionStorage.getItem(ROSTER_CACHE_KEY);
  } catch (error) {
    console.error("[model-client] roster cache read failed", error);
    return false;
  }
  if (raw === null) return false;
  try {
    const entries = validateRoster(JSON.parse(raw));
    if (!entries) throw new Error("malformed cached roster");
    applyRoster(deepFreeze(entries));
    return true;
  } catch (error) {
    console.error("[model-client] malformed roster cache (ignored)", error);
    return false;
  }
}

/**
 * Write the roster cache to sessionStorage (best-effort; frozen values
 * serialize like their unfrozen twins).
 *
 * @param {Deps} d
 * @param {Readonly<ServiceEntry[]>} entries
 * @returns {void}
 */
function writeRosterCache(d, entries) {
  if (!d.sessionStorage) return;
  try {
    d.sessionStorage.setItem(ROSTER_CACHE_KEY, JSON.stringify(entries));
  } catch (error) {
    console.error("[model-client] roster cache write failed", error);
  }
}

/**
 * Is `value` a valid recency map (plain object of natural-number ranks)?
 *
 * @param {unknown} value
 * @returns {boolean}
 */
function isValidRecency(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  for (const key in /** @type {Record<string, unknown>} */ (value)) {
    const rank = /** @type {Record<string, unknown>} */ (value)[key];
    if (typeof rank !== "number" || !Number.isInteger(rank) || rank < 1) return false;
  }
  return true;
}

/**
 * Persist one service's recency map to localStorage (the tx-like durable
 * write — ALWAYS before the model_changed broadcast).
 *
 * @param {Deps} d
 * @param {string} service
 * @param {RecencyMap} map
 * @returns {void}
 */
function persistRecency(d, service, map) {
  if (!d.localStorage) return;
  try {
    d.localStorage.setItem(RECENCY_KEY_PREFIX + service, JSON.stringify(map));
  } catch (error) {
    console.error("[model-client] recency persist failed", service, error);
  }
}

/**
 * Bootstrap-ensure one service's recency map: a valid stored map is kept
 * as-is; a missing or malformed one is rebuilt from the roster indexes
 * (model at index i gets rank i+1) and persisted immediately.
 *
 * @param {Deps} d
 * @param {Readonly<ServiceEntry>} entry
 * @returns {RecencyMap}
 */
function ensureRecencyMap(d, entry) {
  const key = RECENCY_KEY_PREFIX + entry.service;
  let stored = null;
  try {
    stored = d.localStorage ? d.localStorage.getItem(key) : null;
  } catch (error) {
    console.error("[model-client] recency read failed", entry.service, error);
  }
  if (stored !== null) {
    try {
      const parsed = JSON.parse(stored);
      if (isValidRecency(parsed)) return /** @type {RecencyMap} */ (parsed);
      console.error("[model-client] malformed recency map (rebuilt)", entry.service, parsed);
    } catch {
      console.error("[model-client] malformed recency JSON (rebuilt)", entry.service);
    }
  }
  /** @type {RecencyMap} */
  const built = {};
  entry.models.forEach((model, index) => {
    built[model.id] = index + 1;
  });
  persistRecency(d, entry.service, built);
  return built;
}

/**
 * Bootstrap-ensure the recency maps of every rostered service.
 *
 * @param {Deps} d
 * @returns {void}
 */
function ensureRecency(d) {
  for (const service in roster) {
    recency[service] = ensureRecencyMap(d, roster[service]);
  }
}

/**
 * The HIGHEST-rank model of a service (bootstrap-ensure target: rank falls
 * back to 0 for unranked models; the first maximum in roster order wins).
 *
 * @param {Readonly<ServiceEntry>} entry
 * @param {RecencyMap} map
 * @returns {string | null}
 */
function highestRankModel(entry, map) {
  let best = null;
  let bestRank = 0;
  for (const model of entry.models) {
    const rank = map[model.id] ?? 0;
    if (best === null || rank > bestRank) {
      best = model.id;
      bestRank = rank;
    }
  }
  return best;
}

/**
 * Resolve the in-memory current selection from the server state snapshot
 * (bootstrap-ensure): a state model present in the service's roster is
 * kept; a missing/bad one defaults to the HIGHEST-rank model of the
 * service. The default is in-memory only — the server owns the selection
 * and select() owns POST /api/model.
 *
 * @param {Readonly<{ service: string, model: string | null }> | null} stateSnap
 * @returns {{ service: string | null, model: string | null }}
 */
function resolveCurrent(stateSnap) {
  if (!stateSnap) return { service: null, model: null };
  const { service, model } = stateSnap;
  const entry = roster[service];
  if (entry && entry.models.length > 0 && !(model !== null && entry.models.some((m) => m.id === model))) {
    return { service, model: highestRankModel(entry, recency[service] ?? {}) };
  }
  return { service, model };
}

/**
 * Validate + deep-freeze a `model_changed` value (the guard every
 * consumer and this module's own emitter go through). Invalid values are
 * logged and dropped (null) — repo drop/malformed semantics.
 *
 * @param {unknown} value
 * @returns {Readonly<ModelChangedEvent> | null}
 */
export function isModelChangedEvent(value) {
  const errors = validateModel_changed(value);
  if (errors.length > 0) {
    console.error("[model-client] malformed model_changed (dropped)", errors, value);
    return null;
  }
  return deepFreeze(/** @type {ModelChangedEvent} */ (value));
}

/**
 * Dispatch the sender-side echo: BroadcastChannel never delivers a post to
 * its own context, so the emitter also dispatches on the window (where the
 * subscribers of THIS context listen). Best-effort: a missing window (node
 * tests without one) skips the echo.
 *
 * @param {EchoWindow | null} win
 * @param {Readonly<ModelChangedEvent>} event
 * @returns {void}
 */
function dispatchEcho(win, event) {
  if (!win || typeof win.dispatchEvent !== "function") return;
  const echo =
    typeof win.CustomEvent === "function"
      ? new win.CustomEvent(MODEL_CHANGED_EVENT, { detail: event })
      : { type: MODEL_CHANGED_EVENT, detail: event };
  win.dispatchEvent(echo);
}

/**
 * Emit `model_changed`: build the envelope, validate + deep-freeze it,
 * THEN broadcast (BC post → window echo → console tee). The console tee
 * rides the installed console bus via console.info — the bus wraps the
 * page console, so the swap is visible in /console like any other event.
 *
 * @param {string} service
 * @param {string} model
 * @returns {Readonly<ModelChangedEvent> | null}
 */
function emitModelChanged(service, model) {
  const event = isModelChangedEvent({
    _type: "model_changed",
    service,
    model,
    ts: deps ? deps.now() : new Date().toISOString(),
  });
  if (!event) return null; // logged by the guard; unreachable for valid inputs
  try {
    postChannel?.postMessage(event);
  } catch (error) {
    console.error("[model-client] broadcast failed", error);
  }
  dispatchEcho(deps ? deps.window : null, event);
  console.info("[model-client] model_changed:", event.model, "(service:", event.service + ")");
  return event;
}

/**
 * Bump one service's recency: the selected model moves to max(rank)+1 and
 * the map is persisted to localStorage FIRST (persist-then-broadcast).
 *
 * @param {Deps} d
 * @param {string} service
 * @param {string} model
 * @returns {void}
 */
function bumpRecency(d, service, model) {
  const map = recency[service] ?? {};
  let max = 0;
  for (const key in map) {
    if (map[key] > max) max = map[key];
  }
  map[model] = max + 1;
  recency[service] = map;
  persistRecency(d, service, map);
}

/**
 * Pre-mount boot (D39: runs BEFORE any web component mounts):
 *
 * 1. roster: sessionStorage cache hit → install it and refresh in the
 *    background; miss → GET /api/services in the foreground. The fetched
 *    roster replaces the cache either way.
 * 2. recency: per-service localStorage maps bootstrap-ensured against the
 *    roster (model at index i → rank i+1 unless a valid map already
 *    exists).
 * 3. selection: GET /api/state (the server is the owner); a missing or
 *    not-in-roster model default-selects the HIGHEST-rank model of the
 *    service, in memory only.
 *
 * Never throws; always resolves with the (possibly sparse) snapshot so a
 * late arriver renders value-or-blank, never an error.
 *
 * @param {InitOptions} [options] injectable deps (tests fake the boundaries)
 * @returns {Promise<Readonly<StateSnapshot>>}
 */
export async function init(options = {}) {
  const d = /** @type {Deps} */ ({ ...defaultDeps(), ...options });
  deps = d;
  roster = {};
  recency = {};
  current = null;
  postChannel = new d.BroadcastChannel(MODEL_CHANNEL);
  const cached = readCachedRoster(d);
  const services = fetchServices(d).then((entries) => {
    if (entries) {
      applyRoster(entries);
      writeRosterCache(d, entries);
      ensureRecency(d);
    }
  });
  pending.push(services);
  if (!cached) {
    // Cache miss: the roster is part of the foreground boot.
    await services;
  } // Cache hit: `services` keeps refreshing in the background (whenIdle()).
  ensureRecency(d);
  const stateSnap = await fetchState(d);
  current = resolveCurrent(stateSnap);
  return getState();
}

/**
 * The frozen snapshot of the model domain: current selection, roster by
 * service, recency by service. Components render only — they never mutate
 * this (it is deep-frozen) and never own the state behind it.
 *
 * @returns {Readonly<StateSnapshot>}
 */
export function getState() {
  if (!deps) throw new Error("[model-client] init() must run before getState()");
  /** @type {Recency} */
  const recencyCopy = {};
  for (const service in recency) {
    recencyCopy[service] = { ...recency[service] };
  }
  return deepFreeze({
    service: current ? current.service : null,
    model: current ? current.model : null,
    roster,
    recency: recencyCopy,
  });
}

/**
 * Select a service+model — the model client OWNS POST /api/model
 * {service, model}. On success: update the in-memory current, bump the
 * service's recency to max+1 and PERSIST it to localStorage first, then
 * emit `model_changed` (validate → deepFreeze → BC post → window echo →
 * console tee). On any failure the error is returned to the caller and
 * NOTHING changes (no state, no persist, no broadcast).
 *
 * @param {string} service
 * @param {string} model
 * @returns {Promise<SelectResult>}
 */
export async function select(service, model) {
  if (!deps) throw new Error("[model-client] init() must run before select()");
  const entry = roster[service];
  if (!entry || !entry.models.some((m) => m.id === model)) {
    return { ok: false, error: `unknown model '${model}' for service '${service}'` };
  }
  if (!deps.fetch) return { ok: false, error: "no fetch available" };
  /** @type {{ ok: boolean, status?: number, json: () => Promise<any> }} */
  let res;
  try {
    res = await deps.fetch("/api/model", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ service, model }),
    });
  } catch (error) {
    return { ok: false, error: `POST /api/model failed: ${String(error)}` };
  }
  if (!res.ok) {
    let message = `POST /api/model failed (status ${res.status ?? "unknown"})`;
    try {
      const body = await res.json();
      if (body && typeof body.error === "string") message = body.error;
    } catch {
      // keep the status-based message
    }
    return { ok: false, error: message };
  }
  current = { service, model };
  bumpRecency(deps, service, model); // localStorage FIRST …
  const event = emitModelChanged(service, model); // … broadcast SECOND
  return event ? { ok: true, event } : { ok: false, error: "invalid event (dropped)" };
}

/**
 * Subscribe to the live `model_changed` stream (BroadcastChannel + window
 * echo pair). Delivers ONLY validated, deep-frozen events; malformed
 * payloads are logged and dropped. Late-arriver rule: subscribe FIRST,
 * read getState() SECOND — the snapshot and the live stream are gap-free.
 *
 * @param {(event: Readonly<ModelChangedEvent>) => void} fn
 * @returns {() => void} unsubscribe
 */
export function subscribe(fn) {
  if (!deps) throw new Error("[model-client] init() must run before subscribe()");
  // A DEDICATED channel per subscriber — a BC instance never receives its
  // own posts, so this must differ from the emitter's posting channel.
  const channel = new deps.BroadcastChannel(MODEL_CHANNEL);
  channel.onmessage = (event) => {
    const valid = isModelChangedEvent(event.data);
    if (valid) fn(valid);
  };
  const win = deps.window;
  /** @type {(event: { type: string, detail?: unknown }) => void} */
  const onEcho = (event) => {
    const valid = isModelChangedEvent(event.detail);
    if (valid) fn(valid);
  };
  if (win && typeof win.addEventListener === "function") {
    win.addEventListener(MODEL_CHANGED_EVENT, onEcho);
  }
  return () => {
    channel.onmessage = null;
    channel.close();
    if (win && typeof win.removeEventListener === "function") {
      win.removeEventListener(MODEL_CHANGED_EVENT, onEcho);
    }
  };
}

/**
 * Resolves when all in-flight boot/background work (the background roster
 * refresh) has settled. Consumers never need this; tests use it to await
 * the fire-and-forget refresh deterministically.
 *
 * @returns {Promise<void>}
 */
export async function whenIdle() {
  const work = pending;
  pending = [];
  await Promise.all(work);
}

/**
 * Tear the singleton down (test seam only — production code never calls
 * this; a page owns exactly one model client for its lifetime).
 *
 * @returns {void}
 */
export function resetForTests() {
  try {
    postChannel?.close();
  } catch {
    // already closed
  }
  deps = null;
  roster = {};
  recency = {};
  current = null;
  postChannel = null;
  pending = [];
}
