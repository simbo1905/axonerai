// @ts-check

/**
 * Per-provider model roster + per-model context-window fallbacks, plus the
 * item41 `/api/models` client. The hardcoded roster below mirrors
 * `.axonerai/axonerai.jsonc` and stays as the FALLBACK: when the server's
 * per-provider models config (`.axonerai/models/<provider>-models.jsonc`)
 * knows the model, its values win.
 */

import { deepFreeze } from "./wire.mjs";

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

/**
 * The context window for the footer percent: the config window carried by
 * the `/api/state` snapshot (item41, per-model) wins; the hardcoded map is
 * the fallback. `null` → the footer omits the percent.
 *
 * @param {{ model?: unknown, context?: { context_window?: unknown } | null } | null | undefined} snapshot
 * @returns {number | null}
 */
export function resolveContextWindow(snapshot) {
  const fromConfig = snapshot?.context?.context_window;
  if (typeof fromConfig === "number" && fromConfig > 0) {
    return fromConfig;
  }
  return contextWindowFor(
    typeof snapshot?.model === "string" ? snapshot.model : "",
  );
}

/**
 * One row of the `/api/models` payload.
 *
 * @typedef {object} ProviderModelRow
 * @property {string} id model id used in API calls
 * @property {string} display human name
 * @property {number} contextWindow context window in tokens
 */

/**
 * The `/api/models` payload shape served by axoner-web (item41).
 *
 * @typedef {object} ProviderModelsPayload
 * @property {string} provider
 * @property {string | null} source "local" | "user" | null
 * @property {readonly ProviderModelRow[]} models
 */

/**
 * Fetch `/api/models` — the current provider's model list from the server's
 * per-provider models config. Returns `null` on any failure or malformed
 * payload (callers keep the hardcoded roster as fallback).
 *
 * @returns {Promise<ProviderModelsPayload | null>}
 */
export async function fetchProviderModels() {
  try {
    const response = await fetch("/api/models");
    if (!response.ok) return null;
    const data = await response.json();
    if (
      data === null ||
      typeof data !== "object" ||
      typeof data.provider !== "string" ||
      !Array.isArray(data.models)
    ) {
      return null;
    }
    const models = data.models.flatMap(
      (/** @type {any} */ entry) =>
        typeof entry?.id === "string" &&
        typeof entry?.context_window === "number"
          ? [
              {
                id: entry.id,
                display:
                  typeof entry.display === "string" ? entry.display : entry.id,
                contextWindow: entry.context_window,
              },
            ]
          : [],
    );
    return deepFreeze({
      provider: data.provider,
      source: typeof data.source === "string" ? data.source : null,
      models,
    });
  } catch {
    return null;
  }
}
