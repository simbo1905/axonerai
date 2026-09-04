// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import {
  fetchProviderModels,
} from "./models.mjs";

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
