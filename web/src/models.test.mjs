// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import {
  MODEL_CONTEXT_WINDOWS,
  PROVIDER_MODELS,
  contextWindowFor,
  modelsForProvider,
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
