// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import {
  MODEL_CONTEXT_WINDOWS,
  PROVIDER_MODELS,
  contextWindowFor,
  fetchProviderModels,
  modelsForProvider,
  resolveContextWindow,
} from "./models.mjs";

test("roster matches the .axonerai/axonerai.jsonc models per provider", () => {
  assert.deepEqual(modelsForProvider("mistral"), [
    "zai-glm-5-2",
    "mistral-medium-latest",
  ]);
  assert.deepEqual(modelsForProvider("opencode-zen"), ["glm-5.2"]);
  assert.deepEqual(modelsForProvider("opencode-go"), ["glm-5.2"]);
  assert.deepEqual(modelsForProvider("groq"), [
    "qwen/qwen3.8-27b",
    "openai/gpt-oss-20b",
    "openai/gpt-oss-120b",
  ]);
});

test("unknown provider yields an empty roster", () => {
  assert.deepEqual(modelsForProvider("nope"), []);
  assert.deepEqual(modelsForProvider(""), []);
});

test("every roster model has a context window; unknown models get null", () => {
  for (const models of Object.values(PROVIDER_MODELS)) {
    for (const id of models) {
      assert.equal(typeof contextWindowFor(id), "number", `context window for ${id}`);
    }
  }
  assert.equal(contextWindowFor("not-a-model"), null);
  assert.equal(contextWindowFor(""), null);
});

test("the roster and window maps agree on their keys", () => {
  const roster = new Set(Object.values(PROVIDER_MODELS).flat());
  for (const id of Object.keys(MODEL_CONTEXT_WINDOWS)) {
    assert.ok(roster.has(id), `window map key ${id} is not in the roster`);
  }
});

// --- item41: config-driven context window + /api/models fetch ---------------

test("resolveContextWindow prefers the snapshot's config window", () => {
  const snapshot = {
    model: "zai-glm-5-2",
    context: { tokens: 12345, context_window: 32768 },
  };
  assert.equal(resolveContextWindow(snapshot), 32768);
});

test("resolveContextWindow falls back to the hardcoded map, then null", () => {
  assert.equal(resolveContextWindow({ model: "zai-glm-5-2", context: {} }), 131072);
  assert.equal(resolveContextWindow({ model: "not-a-model", context: {} }), null);
  assert.equal(resolveContextWindow(null), null);
  assert.equal(resolveContextWindow({ model: "x", context: { context_window: 0 } }), null);
});

/** @param {unknown} payload @param {number} [status] */
function stubFetchOnce(payload, status = 200) {
  const original = globalThis.fetch;
  globalThis.fetch = /** @type {typeof fetch} */ (
    async () =>
      new Response(JSON.stringify(payload), {
        status,
        headers: { "Content-Type": "application/json" },
      })
  );
  return () => {
    globalThis.fetch = original;
  };
}

test("fetchProviderModels normalizes the /api/models payload", async () => {
  const restore = stubFetchOnce({
    provider: "mistral",
    source: "local",
    models: [
      {
        id: "zai-glm-5-2",
        display: "GLM-5.2",
        context_window: 32768,
        costs: { input_per_mtok: "$0.50" },
        offer: null,
      },
      { id: "no-window", display: "No Window" },
    ],
  });
  try {
    const payload = await fetchProviderModels();
    if (!payload) throw new Error("valid payload must not be null");
    assert.equal(payload.provider, "mistral");
    assert.equal(payload.source, "local");
    assert.equal(payload.models.length, 1, "rows without a window are dropped");
    assert.deepEqual(payload.models[0], {
      id: "zai-glm-5-2",
      display: "GLM-5.2",
      contextWindow: 32768,
    });
  } finally {
    restore();
  }
});

test("fetchProviderModels returns null on HTTP errors and malformed payloads", async () => {
  for (const [label, payload, status] of /** @type {const} */ ([
    ["500", { provider: "mistral", models: [] }, 500],
    ["wrong shape", { models: "nope" }, 200],
    ["null body", null, 200],
    ["missing models", { provider: "mistral" }, 200],
  ])) {
    const restore = stubFetchOnce(payload, status);
    try {
      const result = await fetchProviderModels();
      assert.equal(result, null, `${label} must yield null (fallback wins)`);
    } finally {
      restore();
    }
  }
});

test("fetchProviderModels swallows network failures", async () => {
  const original = globalThis.fetch;
  globalThis.fetch = /** @type {typeof fetch} */ (
    async () => {
      throw new TypeError("network down");
    }
  );
  try {
    assert.equal(await fetchProviderModels(), null);
  } finally {
    globalThis.fetch = original;
  }
});
