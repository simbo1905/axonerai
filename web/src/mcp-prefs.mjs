// @ts-check
/**
 * Per-folder MCP toggle preferences (item48): the BROWSER is the durable
 * store for which MCP servers are disabled. The localStorage key embeds the
 * repo folder (from /api/state `repo.path` — the same folder identity the
 * `?s=`/history boot uses), so two checkouts of different folders never
 * share toggles. The stored value is a JSON array of disabled server names;
 * `parseDisabledServers` validates and deep-freezes it, and invalid payloads
 * are returned as `null` so the caller can log-and-drop them per the
 * boundary-validation rule.
 */

/** Prefix of the per-folder localStorage key holding disabled MCP servers. */
export const MCP_DISABLED_KEY_PREFIX = "agt.mcp-disabled:";

/**
 * The folder-scoped localStorage key for a repo path.
 *
 * @param {string} repoPath absolute repo folder path (/api/state `repo.path`)
 * @returns {string} e.g. `agt.mcp-disabled:/Users/Shared/axonerai`
 */
export function mcpDisabledKey(repoPath) {
  return `${MCP_DISABLED_KEY_PREFIX}${repoPath}`;
}

/**
 * Parse → validate → freeze a stored disabled-servers payload.
 *
 * - `null`/`""` (no stored preference) → frozen empty array;
 * - a JSON array of non-empty strings → frozen array;
 * - anything else (invalid JSON, non-array, a non-string/empty element) →
 *   `null`, which the caller logs and drops.
 *
 * @param {string | null} raw raw localStorage value
 * @returns {readonly string[] | null} frozen list, or null when invalid
 */
export function parseDisabledServers(raw) {
  if (raw === null || raw === "") return Object.freeze([]);
  /** @type {unknown} */
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!Array.isArray(parsed)) return null;
  /** @type {string[]} */
  const names = [];
  for (const item of parsed) {
    if (typeof item !== "string" || item.length === 0) return null;
    names.push(item);
  }
  return Object.freeze(names);
}
