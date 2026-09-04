// @ts-check

import { deepFreeze } from "./wire.mjs";

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
