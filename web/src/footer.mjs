// @ts-check

/**
 * Pure footer status-bar formatter (node-testable; no DOM, no network).
 * The panel component (web/src/components/agt-panel.js) renders the strings
 * produced here, using {@link footerSegments} for its styled left-side spans
 * (provider/think rendered fainter than the model).
 */

/** Mode slot: hardcoded "Chat" for now (a slot, not a config). */
export const FOOTER_MODE = "Chat";

/** Think slot: hardcoded "off" until a thinking/reasoning toggle exists. */
export const FOOTER_THINK = "off";

/**
 * The footer-relevant slice of a `/api/state` snapshot.
 *
 * @typedef {object} FooterSegments
 * @property {string} mode hardcoded mode slot
 * @property {string} model current model name
 * @property {string} provider current provider (rendered fainter)
 * @property {string} think think-slot state (rendered fainter)
 * @property {number | null} tokens context tokens (null when unavailable)
 */

/**
 * @param {{ model?: unknown, provider?: unknown, context?: { tokens?: unknown } | null } | null | undefined} snapshot
 * @returns {FooterSegments}
 */
export function footerSegments(snapshot) {
  const model =
    typeof snapshot?.model === "string" && snapshot.model
      ? snapshot.model
      : "(unknown)";
  const provider =
    typeof snapshot?.provider === "string" && snapshot.provider
      ? snapshot.provider
      : "(unknown)";
  const raw = snapshot?.context?.tokens;
  const tokens =
    typeof raw === "number" && Number.isFinite(raw) ? raw : null;
  return { mode: FOOTER_MODE, model, provider, think: FOOTER_THINK, tokens };
}

/**
 * Format the footer status bar.
 *
 * @param {{ model?: unknown, provider?: unknown, context?: { tokens?: unknown } | null } | null | undefined} snapshot
 *   a `/api/state` snapshot
 * @param {number | null} contextWindow the model's context window in tokens,
 *   or null when unknown (the percent is then omitted)
 * @returns {{ left: string, right: string }} `left` is
 *   `Chat · <model> <provider> · think <state>`; `right` is
 *   `<used_1dp>K (<percent>%)` or just `<used_1dp>K` for an unknown window
 *   (empty when tokens are unavailable).
 */
export function formatFooter(snapshot, contextWindow) {
  const segments = footerSegments(snapshot);
  const left = `${segments.mode} · ${segments.model} ${segments.provider} · think ${segments.think}`;
  let right = "";
  if (segments.tokens !== null) {
    const usedK = (segments.tokens / 1000).toFixed(1);
    right =
      typeof contextWindow === "number" && contextWindow > 0
        ? `${usedK}K (${Math.round((segments.tokens / contextWindow) * 100)}%)`
        : `${usedK}K`;
  }
  return { left, right };
}
