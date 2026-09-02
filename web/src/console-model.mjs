// @ts-check
/**
 * Pure console model helpers (item32): devtools-style argument
 * serialization, envelope construction, merge/dedupe, ring-buffer trim
 * policy and autoscroll arithmetic. Node-testable — NO browser APIs and NO
 * root-absolute imports live here; everything crossing an IO boundary is
 * validated + frozen by the callers (console-bus / spool worker / console
 * screen), per the repo rule "JTD validators for things that go over IO".
 */

/**
 * A console envelope (`schemas/console_entry.jdt.json`).
 *
 * @typedef {object} ConsoleEntry
 * @property {string} id `${pageId}:${seq}` — dedupe key across contexts
 * @property {string} pageId uuid of the emitting context
 * @property {number} ts epoch milliseconds
 * @property {"log" | "info" | "warn" | "error"} level
 * @property {string} text space-joined, devtools-style serialization
 */

/**
 * Serialize console args like devtools: strings raw, everything else via
 * `JSON.stringify(value, null, 2)` (with safe fallbacks for undefined /
 * circular structures). Pure.
 *
 * @param {readonly unknown[]} args
 * @returns {string}
 */
export function formatConsoleArgs(args) {
  return args
    .map((arg) => {
      if (typeof arg === "string") return arg;
      try {
        const json = JSON.stringify(arg, null, 2);
        return json === undefined ? String(arg) : json;
      } catch {
        return String(arg);
      }
    })
    .join(" ");
}

/**
 * Build a fresh console envelope. Pure. The caller is responsible for
 * validating + freezing before the envelope crosses an IO boundary.
 *
 * @param {object} parts
 * @param {string} parts.pageId
 * @param {number} parts.seq per-page monotonic counter
 * @param {"log" | "info" | "warn" | "error"} parts.level
 * @param {string} parts.text
 * @returns {ConsoleEntry}
 */
export function createConsoleEntry({ pageId, seq, level, text }) {
  return {
    id: `${pageId}:${seq}`,
    pageId,
    ts: Date.now(),
    level,
    text,
  };
}

/**
 * Numeric seq from an envelope id `${page}:${seq}` (NaN-safe fallback 0).
 *
 * @param {string} id
 * @returns {number}
 */
export function seqOf(id) {
  const index = id.lastIndexOf(":");
  const seq = index >= 0 ? Number(id.slice(index + 1)) : NaN;
  return Number.isFinite(seq) ? seq : 0;
}

/**
 * Order envelopes by ts → seq (same-timestamp entries order by the numeric
 * seq in their id; pageIds may contain colons).
 *
 * @param {Readonly<ConsoleEntry>} a
 * @param {Readonly<ConsoleEntry>} b
 */
function byTsThenSeq(a, b) {
  return a.ts - b.ts || seqOf(a.id) - seqOf(b.id);
}

/**
 * Merge new envelopes into an append-only log: drop ids already present
 * (backlog vs live double-delivery), order by ts → seq, return a fresh
 * array. Pure — the caller freezes the result. Load-bearing belt: under the
 * persist-then-broadcast topology the ONLY expected duplicate mode is a
 * commit that precedes the backlog snapshot while its `tx.oncomplete`
 * re-broadcast lands after the subscription; zero-delivery windows are
 * closed by topology, not by this merge.
 *
 * @param {readonly Readonly<ConsoleEntry>[]} existing
 * @param {readonly Readonly<ConsoleEntry>[]} incoming
 * @returns {Readonly<ConsoleEntry>[]}
 */
export function mergeEntries(existing, incoming) {
  const seen = new Set(existing.map((entry) => entry.id));
  const fresh = incoming.filter((entry) => !seen.has(entry.id));
  if (fresh.length === 0) return [...existing];
  return [...existing, ...fresh].sort(byTsThenSeq);
}

/**
 * Ring-buffer trim POLICY: given entries in ts → seq order, return the ids
 * to evict so at most `max` remain (oldest first). Pure — the spool worker
 * applies it with an IndexedDB ts-index cursor; the unit suite pins the
 * decision itself.
 *
 * @param {readonly Readonly<ConsoleEntry>[]} entries oldest-first
 * @param {number} max ring capacity (2000 in production)
 * @returns {string[]}
 */
export function evictIds(entries, max) {
  const excess = entries.length - max;
  if (excess <= 0) return [];
  return entries.slice(0, excess).map((entry) => entry.id);
}

/**
 * Autoscroll arithmetic: is the view `slack` px or less from the bottom?
 * Pure — tested in node against a fake element.
 *
 * @param {{ scrollTop: number, scrollHeight: number, clientHeight: number }} view
 * @param {number} [slack] px (32 in production)
 * @returns {boolean}
 */
export function isAtBottom(view, slack = 32) {
  return view.scrollHeight - view.scrollTop - view.clientHeight <= slack;
}

/**
 * Autoscroll arithmetic: how the "N new" pill count evolves when entries
 * arrive. Stuck to bottom → the view follows and the count stays put
 * (zero); scrolled up → each fresh entry bumps the count. Pure.
 *
 * @param {number} count current "N new" count
 * @param {boolean} stick true while the view is stuck to the bottom
 * @param {number} freshCount entries that just arrived
 * @returns {number}
 */
export function newCountOnAppend(count, stick, freshCount) {
  return stick ? count : count + freshCount;
}
