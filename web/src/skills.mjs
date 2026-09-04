// @ts-check

/**
 * item49 `/api/skills` client. One entry per `<name>/SKILL.md` under
 * `.axonerai/skills` (local, wins) plus `~/.axonerai/skills` (user fallback)
 * plus the built-in skills shipped in the binary (item50); the server
 * already applies local-masks-user-masks-builtin and skips broken files.
 */

import { deepFreeze } from "./wire.mjs";

/**
 * One row of the `/api/skills` payload.
 *
 * @typedef {object} SkillRow
 * @property {string} name skill name (the folder name)
 * @property {string} description frontmatter description
 * @property {"local" | "user" | "builtin"} source where it was found
 * @property {string} path the SKILL.md file backing the entry
 */

/**
 * Fetch `/api/skills` — the skills listing for the /skills tree. Returns
 * `null` on any failure or malformed payload (callers surface the failure
 * on the console bus, never a crash).
 *
 * @returns {Promise<readonly SkillRow[] | null>}
 */
export async function fetchSkills() {
  try {
    const response = await fetch("/api/skills");
    if (!response.ok) return null;
    const data = await response.json();
    if (!Array.isArray(data)) return null;
    const skills = data.flatMap(
      (/** @type {any} */ entry) =>
        typeof entry?.name === "string" &&
        typeof entry?.description === "string" &&
        (entry?.source === "local" ||
          entry?.source === "user" ||
          entry?.source === "builtin") &&
        typeof entry?.path === "string"
          ? [
              {
                name: entry.name,
                description: entry.description,
                source: entry.source,
                path: entry.path,
              },
            ]
          : [],
    );
    return deepFreeze(skills);
  } catch {
    return null;
  }
}
