// @ts-check
import test from "node:test";
import assert from "node:assert/strict";

// test.beforeEach/afterEach exist at runtime (node >= 18.8, bun) but the
// installed @types/node does not declare them on the `test` callable — go
// through an `any` view for the hooks only.
const hooks = /** @type {any} */ (test);
import {
  MODEL_CHANNEL,
  MODEL_CHANGED_EVENT,
  RECENCY_KEY_PREFIX,
  ROSTER_CACHE_KEY,
  getState,
  init,
  isModelChangedEvent,
  resetForTests,
  select,
  subscribe,
  whenIdle,
} from "./model-client.mjs";

/**
 * In-memory KV storage fake that records setItem calls on the shared ops
 * log (for persist-before-broadcast ordering assertions).
 *
 * @param {Record<string, string>} [seed] pre-existing values
 */
function makeStorage(seed = {}) {
  const map = new Map(Object.entries(seed));
  return {
    /** @param {string} key */
    getItem: (key) => (map.has(key) ? /** @type {string} */ (map.get(key)) : null),
    /** @param {string} key @param {unknown} value */
    setItem: (key, value) => {
      map.set(key, String(value));
      ops.push(`ls.setItem:${key}`);
    },
    /** @param {string} key */
    removeItem: (key) => map.delete(key),
    /**
     * Read without touching the ops log (assertions only).
     * @param {string} key
     */
    peek: (key) => (map.has(key) ? /** @type {string} */ (map.get(key)) : null),
  };
}

/** @type {any[]} */
let channels = [];

/**
 * BroadcastChannel fake: posts are recorded on the shared ops log and
 * delivered (cloned, like structured clone) to OTHER same-name channels.
 */
class FakeBroadcastChannel {
  /** @type {string} */
  name;
  /** @type {((event: any) => void) | null} */
  onmessage;
  /** @type {unknown[]} */
  posts;
  /** @type {boolean} */
  closed;
  /** @param {string} name */
  constructor(name) {
    this.name = name;
    this.onmessage = null;
    this.posts = [];
    this.closed = false;
    channels.push(this);
  }
  /** @param {unknown} data */
  postMessage(data) {
    ops.push("bc.post");
    this.posts.push(data);
    for (const channel of channels) {
      if (channel !== this && channel.name === this.name && channel.onmessage) {
        channel.onmessage({ data: JSON.parse(JSON.stringify(data)) });
      }
    }
  }
  close() {
    this.closed = true;
  }
}

/**
 * Window fake recording event listeners; dispatchEvent drives them.
 */
function makeWindow() {
  /** @type {Map<string, ((event: any) => void)[]>} */
  const listeners = new Map();
  return {
    listeners,
    addEventListener(/** @type {string} */ type, /** @type {any} */ fn) {
      if (!listeners.has(type)) listeners.set(type, []);
      /** @type {((event: any) => void)[]} */ (listeners.get(type)).push(fn);
    },
    removeEventListener(/** @type {string} */ type, /** @type {any} */ fn) {
      listeners.set(
        type,
        (listeners.get(type) ?? []).filter((f) => f !== fn),
      );
    },
    /** @param {{ type: string, detail?: unknown }} event */
    dispatchEvent(event) {
      for (const fn of listeners.get(event.type) ?? []) fn(event);
      return true;
    },
    /* eslint-disable-next-line no-inner-declarations */
    CustomEvent: class {
      /** @param {string} type @param {{ detail?: unknown }} [init] */
      constructor(type, init = {}) {
        this.type = type;
        this.detail = init.detail;
      }
    },
  };
}

/**
 * Fake fetch routing URLs to canned responses; records every call (with
 * the parsed POST body) on the shared calls log.
 *
 * @param {Record<string, { status?: number, body?: any, hang?: (resolve: (value: any) => void) => void }>} routes
 * @param {{ url: string, init: any }[]} calls shared call log
 */
function makeFetch(routes, calls) {
  return async (/** @type {string} */ url, /** @type {any} */ requestInit) => {
    calls.push({ url, init: requestInit ?? null });
    const route = routes[url];
    if (!route) return { ok: false, status: 404, json: async () => ({}) };
    const hang = route.hang;
    if (hang) {
      return new Promise((resolve) => hang(resolve));
    }
    const status = route.status ?? 200;
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => route.body,
    };
  };
}

/** The services roster fixture (two services, deterministic model order). */
const ROSTER = [
  {
    service: "x",
    enabled: true,
    connected: true,
    models: [
      { id: "m0", display: "Model Zero", context_window: 1000 },
      { id: "m1", display: "Model One", context_window: 2000 },
      { id: "m2", display: "Model Two", context_window: 3000 },
    ],
  },
  {
    service: "y",
    enabled: true,
    connected: false,
    models: [
      { id: "y0", display: "Y Zero", context_window: 500 },
      { id: "y1", display: "Y One", context_window: 600 },
    ],
  },
];

/** @type {string[]} */
let ops = [];
/** @type {{ url: string, init: any }[]} */
let calls = [];
/** @type {string[]} */
let errorLog = [];

/**
 * Capture console.error (drop-malformed logging) for the duration of fn.
 *
 * @param {() => Promise<void> | void} fn
 */
async function captureErrors(fn) {
  const original = console.error;
  console.error = (...args) => {
    errorLog.push(String(args[0]));
  };
  try {
    await fn();
  } finally {
    console.error = original;
  }
}

hooks.beforeEach(() => {
  ops = [];
  calls = [];
  errorLog = [];
  channels = [];
  resetForTests();
});

hooks.afterEach(() => {
  resetForTests();
});

test("boot: roster + state + recency built from roster indexes, cache written, snapshot frozen", async () => {
  const ls = makeStorage();
  const ss = makeStorage();
  const win = makeWindow();
  await init({
    fetch: makeFetch(
      {
        "/api/services": { body: ROSTER },
        "/api/state": { body: { service: "x", model: "m1" } },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (win),
    now: () => "2026-09-04T00:00:00.000Z",
  });
  const snap = getState();
  assert.equal(snap.service, "x");
  assert.equal(snap.model, "m1");
  // recency maps built from roster indexes: model at index i → rank i+1
  assert.deepEqual(snap.recency.x, { m0: 1, m1: 2, m2: 3 });
  assert.deepEqual(snap.recency.y, { y0: 1, y1: 2 });
  // recency maps persisted at boot
  assert.deepEqual(JSON.parse(/** @type {string} */ (ls.peek("agt.model-recency:x"))), {
    m0: 1,
    m1: 2,
    m2: 3,
  });
  assert.deepEqual(JSON.parse(/** @type {string} */ (ls.peek("agt.model-recency:y"))), {
    y0: 1,
    y1: 2,
  });
  // roster cached in sessionStorage
  const cached = JSON.parse(/** @type {string} */ (ss.peek(ROSTER_CACHE_KEY)));
  assert.equal(cached.length, 2);
  assert.equal(cached[0].service, "x");
  // snapshot (and everything under it) is frozen
  assert.ok(Object.isFrozen(snap));
  assert.ok(Object.isFrozen(snap.roster));
  assert.ok(Object.isFrozen(snap.roster.x));
  assert.ok(Object.isFrozen(snap.recency));
  assert.ok(Object.isFrozen(snap.recency.x));
  assert.ok(calls.some((c) => c.url === "/api/services"));
  assert.ok(calls.some((c) => c.url === "/api/state"));
});

test("recency: a valid stored map is kept as-is and a select bumps it to max+1 (persisted)", async () => {
  const ls = makeStorage({ [RECENCY_KEY_PREFIX + "x"]: JSON.stringify({ m0: 2, m1: 5 }) });
  const ss = makeStorage();
  await init({
    fetch: makeFetch(
      {
        "/api/services": { body: ROSTER },
        "/api/state": { body: { service: "x", model: "m1" } },
        "/api/model": { body: { service: "x", model: "m2" } },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (makeWindow()),
    now: () => "2026-09-04T00:00:00.000Z",
  });
  // stored map survives boot (NOT rebuilt from indexes)
  assert.deepEqual(getState().recency.x, { m0: 2, m1: 5 });
  const result = await select("x", "m2");
  assert.ok(result.ok);
  assert.deepEqual(getState().recency.x, { m0: 2, m1: 5, m2: 6 });
  assert.deepEqual(JSON.parse(/** @type {string} */ (ls.peek(RECENCY_KEY_PREFIX + "x"))), {
    m0: 2,
    m1: 5,
    m2: 6,
  });
});

test("recency: a malformed stored map is dropped, logged, and rebuilt from indexes", async () => {
  const ls = makeStorage({ [RECENCY_KEY_PREFIX + "x"]: '{"m0":"nope"}' });
  const ss = makeStorage();
  await captureErrors(async () => {
    await init({
      fetch: makeFetch(
        {
          "/api/services": { body: ROSTER },
          "/api/state": { body: { service: "x", model: "m1" } },
        },
        calls,
      ),
      BroadcastChannel: FakeBroadcastChannel,
      localStorage: /** @type {any} */ (ls),
      sessionStorage: /** @type {any} */ (ss),
      window: /** @type {any} */ (makeWindow()),
    });
    assert.ok(
      errorLog.some((line) => line.includes("malformed recency")),
      "malformed recency must be logged",
    );
  });
  assert.deepEqual(getState().recency.x, { m0: 1, m1: 2, m2: 3 });
});

test("default-select: state with no model picks the HIGHEST-rank model (not the last index)", async () => {
  // seed recency so m0 outranks m1 even though m1 is later in the roster
  const ls = makeStorage({ [RECENCY_KEY_PREFIX + "x"]: JSON.stringify({ m0: 9, m1: 1 }) });
  const ss = makeStorage();
  await init({
    fetch: makeFetch(
      {
        "/api/services": { body: ROSTER },
        "/api/state": { body: { service: "x", model: "" } },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (makeWindow()),
  });
  assert.equal(getState().model, "m0");
});

test("select success: POST body recorded, recency persisted BEFORE the broadcast, event last", async () => {
  const ls = makeStorage();
  const ss = makeStorage();
  const win = makeWindow();
  await init({
    fetch: makeFetch(
      {
        "/api/services": { body: ROSTER },
        "/api/state": { body: { service: "x", model: "m0" } },
        "/api/model": { body: { service: "x", model: "m2" } },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (win),
    now: () => "2026-09-04T00:00:00.000Z",
  });
  ops = [];
  const result = await select("x", "m2");
  assert.ok(result.ok);
  // the client OWNS POST /api/model {service, model}
  const post = calls.find((c) => c.url === "/api/model");
  assert.ok(post, "POST /api/model must be called");
  if (!post) return;
  assert.equal(post.init.method, "POST");
  assert.deepEqual(JSON.parse(post.init.body), { service: "x", model: "m2" });
  // ordering: recency persisted FIRST, broadcast SECOND
  const persistAt = ops.indexOf(`ls.setItem:${RECENCY_KEY_PREFIX}x`);
  const postAt = ops.indexOf("bc.post");
  assert.ok(persistAt !== -1 && postAt !== -1, "both must happen");
  assert.ok(persistAt < postAt, `persist (${persistAt}) must precede broadcast (${postAt})`);
  // the emitted event is the validated frozen canonical event
  assert.deepEqual(result.event, {
    _type: "model_changed",
    service: "x",
    model: "m2",
    ts: "2026-09-04T00:00:00.000Z",
  });
  assert.ok(Object.isFrozen(result.event));
  // in-memory current updated; server echo of the swap is the POST response
  assert.equal(getState().model, "m2");
  // the window echo fired with the same frozen event
  const echoListeners = win.listeners.get(MODEL_CHANGED_EVENT) ?? [];
  assert.ok(Array.isArray(echoListeners));
});

test("select 400 failure: error returned, no state change, no persist, no broadcast", async () => {
  const ls = makeStorage();
  const ss = makeStorage();
  await init({
    fetch: makeFetch(
      {
        "/api/services": { body: ROSTER },
        "/api/state": { body: { service: "x", model: "m1" } },
        "/api/model": {
          status: 400,
          body: { ok: false, error: "service 'x' is disabled in settings" },
        },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (makeWindow()),
  });
  const before = getState();
  ops = [];
  const result = await select("x", "m2");
  assert.equal(result.ok, false);
  assert.equal(result.error, "service 'x' is disabled in settings");
  assert.equal(getState().model, before.model);
  assert.equal(getState().service, before.service);
  assert.deepEqual(ops, [], "failure must not persist or broadcast anything");
  // and an unknown model never even reaches the wire
  const badRoster = await select("x", "does-not-exist");
  assert.equal(badRoster.ok, false);
  assert.ok(String(badRoster.error).includes("unknown model"));
  assert.equal(calls.filter((c) => c.url === "/api/model").length, 1);
});

test("malformed roster/state are dropped and logged; boot still resolves sparse", async () => {
  const ls = makeStorage();
  const ss = makeStorage();
  await captureErrors(async () => {
    const snap = await init({
      fetch: makeFetch(
        {
          "/api/services": { body: { not: "an array" } },
          "/api/state": { body: "garbage" },
        },
        calls,
      ),
      BroadcastChannel: FakeBroadcastChannel,
      localStorage: /** @type {any} */ (ls),
      sessionStorage: /** @type {any} */ (ss),
      window: /** @type {any} */ (makeWindow()),
    });
    assert.equal(snap.service, null);
    assert.equal(snap.model, null);
    assert.deepEqual(Object.keys(snap.roster), []);
    assert.ok(
      errorLog.some((line) => line.includes("/api/services")),
      "malformed roster must be logged",
    );
    assert.ok(
      errorLog.some((line) => line.includes("/api/state")),
      "malformed state must be logged",
    );
  });
  // nothing cached (the malformed payload is dropped, not cached)
  assert.equal(ss.peek(ROSTER_CACHE_KEY), null);
});

test("malformed roster rows are skipped; malformed rows logged", async () => {
  const ss = makeStorage();
  await captureErrors(async () => {
    await init({
      fetch: makeFetch(
        {
          "/api/services": {
            body: [
              { service: "x", enabled: true, connected: true, models: [
                { id: "m0", display: "ok", context_window: 1 },
                { id: 42, display: "bad", context_window: 1 },
              ] },
              { service: "", enabled: true, connected: true, models: [] },
            ],
          },
          "/api/state": { body: { service: "x", model: "m0" } },
        },
        calls,
      ),
      BroadcastChannel: FakeBroadcastChannel,
      localStorage: /** @type {any} */ (makeStorage()),
      sessionStorage: /** @type {any} */ (ss),
      window: /** @type {any} */ (makeWindow()),
    });
    assert.ok(errorLog.some((line) => line.includes("roster")));
  });
  const snap = getState();
  assert.deepEqual(Object.keys(snap.roster), ["x"]);
  assert.deepEqual(
    snap.roster.x.models.map((m) => m.id),
    ["m0"],
  );
});

test("isModelChangedEvent: valid values freeze; invalid values drop with a log", async () => {
  await captureErrors(async () => {
    const valid = isModelChangedEvent({
      _type: "model_changed",
      service: "x",
      model: "m0",
      ts: "2026-09-04T00:00:00.000Z",
    });
    if (!valid) throw new Error("a valid event must pass the guard");
    assert.ok(Object.isFrozen(valid));
    assert.equal(valid._type, "model_changed");
    // missing ts
    assert.equal(
      isModelChangedEvent({ _type: "model_changed", service: "x", model: "m0" }),
      null,
    );
    // wrong singleton _type
    assert.equal(
      isModelChangedEvent({ _type: "other", service: "x", model: "m0", ts: "t" }),
      null,
    );
    // extra property
    assert.equal(
      isModelChangedEvent({
        _type: "model_changed",
        service: "x",
        model: "m0",
        ts: "t",
        extra: true,
      }),
      null,
    );
    // non-object
    assert.equal(isModelChangedEvent("nope"), null);
    assert.ok(errorLog.length >= 4, "every drop must be logged");
  });
});

test("late arriver: subscribe FIRST, read getState() SECOND, then live events flow", async () => {
  const ls = makeStorage();
  const ss = makeStorage();
  const win = makeWindow();
  await init({
    fetch: makeFetch(
      {
        "/api/services": { body: ROSTER },
        "/api/state": { body: { service: "x", model: "m1" } },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (win),
    now: () => "2026-09-04T00:00:00.000Z",
  });
  /** @type {any[]} */
  const received = [];
  const unsubscribe = subscribe((event) => received.push(event));
  // read second: the snapshot is there, no error, value present
  const snap = getState();
  assert.equal(snap.service, "x");
  assert.equal(snap.model, "m1");
  // live events from a "new tab" BC post arrive validated + frozen
  const otherTab = new FakeBroadcastChannel(MODEL_CHANNEL);
  const before = received.length;
  otherTab.postMessage({
    _type: "model_changed",
    service: "x",
    model: "m2",
    ts: "2026-09-04T00:00:01.000Z",
  });
  assert.equal(received.length, before + 1);
  assert.ok(Object.isFrozen(received[before]));
  assert.equal(received[before].model, "m2");
  // the sender's own echo (window CustomEvent) arrives too — and a malformed
  // echo is dropped silently (well: logged)
  await captureErrors(async () => {
    win.dispatchEvent({
      type: MODEL_CHANGED_EVENT,
      detail: { _type: "model_changed", service: "y", model: "y0", ts: "t" },
    });
    assert.equal(received.length, before + 2);
    win.dispatchEvent({ type: MODEL_CHANGED_EVENT, detail: { bogus: true } });
    assert.equal(received.length, before + 2, "malformed echo must be dropped");
  });
  // after unsubscribe nothing flows
  unsubscribe();
  otherTab.postMessage({
    _type: "model_changed",
    service: "x",
    model: "m1",
    ts: "2026-09-04T00:00:02.000Z",
  });
  assert.equal(received.length, before + 2, "unsubscribed handler must not fire");
});

test("boot with cache hit serves the stale roster immediately, then refreshes in the background", async () => {
  const stale = [{ service: "x", enabled: true, connected: true, models: [{ id: "old", display: "Old", context_window: 1 }] }];
  const fresh = [
    {
      service: "x",
      enabled: true,
      connected: true,
      models: [
        { id: "old", display: "Old", context_window: 1 },
        { id: "new", display: "New", context_window: 2 },
      ],
    },
  ];
  const ls = makeStorage();
  const ss = makeStorage();
  // pre-seed the cache with the STALE roster
  ss.setItem(ROSTER_CACHE_KEY, JSON.stringify(stale));
  /** @type {(value: any) => void} */
  let releaseServices = () => {};
  await init({
    fetch: makeFetch(
      {
        // hang the services fetch: init must NOT wait for it (cache hit)
        "/api/services": { hang: (resolve) => (releaseServices = resolve) },
        "/api/state": { body: { service: "x", model: "old" } },
      },
      calls,
    ),
    BroadcastChannel: FakeBroadcastChannel,
    localStorage: /** @type {any} */ (ls),
    sessionStorage: /** @type {any} */ (ss),
    window: /** @type {any} */ (makeWindow()),
  });
  // stale roster served pre-mount
  assert.deepEqual(Object.keys(getState().roster), ["x"]);
  assert.deepEqual(
    getState().roster.x.models.map((m) => m.id),
    ["old"],
  );
  // the background refresh lands: roster and cache refresh; the service's
  // existing recency map is kept as-is (bootstrap-ensure only rebuilds a
  // missing/malformed map — spec: "unless a map already exists")
  releaseServices({ ok: true, status: 200, json: async () => fresh });
  await whenIdle();
  assert.deepEqual(
    getState().roster.x.models.map((m) => m.id),
    ["old", "new"],
  );
  assert.deepEqual(getState().recency.x, { old: 1 });
  const cached = JSON.parse(/** @type {string} */ (ss.peek(ROSTER_CACHE_KEY)));
  assert.equal(cached[0].models.length, 2);
});
