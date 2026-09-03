// @ts-check

/**
 * Hardcoded per-provider model roster + per-model context-window fallbacks.
 * Until item41 lands model config in /api/state, this is the ONE shared
 * module backing both the footer status bar (context-use percent) and the
 * /models panel tree (rows per current-provider model). The roster mirrors
 * the models in `.axonerai/axonerai.jsonc`.
 */

/**
 * Model ids per provider (order = /models tree row order).
 *
 * @type {Readonly<Record<string, readonly string[]>>}
 */
export const PROVIDER_MODELS = Object.freeze({
  mistral: Object.freeze(["zai-glm-5-2", "mistral-medium-latest"]),
  "opencode-zen": Object.freeze(["glm-5.2"]),
  "opencode-go": Object.freeze(["glm-5.2"]),
  groq: Object.freeze([
    "qwen/qwen3.8-27b",
    "openai/gpt-oss-20b",
    "openai/gpt-oss-120b",
  ]),
});

/**
 * Context-window size in tokens per model id. Unknown models get `null`
 * (the footer then omits the percent).
 *
 * @type {Readonly<Record<string, number>>}
 */
export const MODEL_CONTEXT_WINDOWS = Object.freeze({
  "zai-glm-5-2": 131072,
  "mistral-medium-latest": 131072,
  "glm-5.2": 131072,
  "qwen/qwen3.8-27b": 131072,
  "openai/gpt-oss-20b": 131072,
  "openai/gpt-oss-120b": 131072,
});

/**
 * @param {string} provider
 * @returns {readonly string[]}
 */
export function modelsForProvider(provider) {
  return PROVIDER_MODELS[provider] ?? [];
}

/**
 * @param {string} model
 * @returns {number | null} context window in tokens, or null when unknown
 */
export function contextWindowFor(model) {
  const contextWindow = MODEL_CONTEXT_WINDOWS[model];
  return typeof contextWindow === "number" ? contextWindow : null;
}
