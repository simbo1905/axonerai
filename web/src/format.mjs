// @ts-check
/**
 * Pure display formatters for durations and byte sizes.
 */

/**
 * Format a duration in milliseconds for compact display: `850ms`, `2m 3s`,
 * `1h 2m`. Leading zero units are omitted (`5s`, never `0m 5s`); sub-second
 * durations render as whole milliseconds.
 *
 * @param {number} ms duration in milliseconds
 * @returns {string} human-readable duration
 */
export function formatDuration(ms) {
  if (!Number.isFinite(ms) || ms < 0) return "0ms";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const totalSeconds = Math.floor(ms / 1000);
  const seconds = totalSeconds % 60;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const minutes = totalMinutes % 60;
  const hours = Math.floor(totalMinutes / 60);
  /** @type {string[]} */
  const parts = [];
  if (hours > 0) parts.push(`${hours}h`);
  if (minutes > 0) parts.push(`${minutes}m`);
  if (seconds > 0 || parts.length === 0) parts.push(`${seconds}s`);
  return parts.join(" ");
}

/**
 * Format a byte count for compact display on a binary (1024) base: `123B`,
 * `4.5KB`, `1.2MB`. Values below 10 in the chosen unit get one decimal,
 * larger ones none.
 *
 * @param {number} n byte count
 * @returns {string} human-readable byte size
 */
export function formatBytes(n) {
  if (!Number.isFinite(n) || n < 0) return "0B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${Math.round(value)}B`;
  const text = value < 10 ? value.toFixed(1) : String(Math.round(value));
  return `${text}${units[unit]}`;
}

/**
 * Format a unix-epoch millisecond timestamp as a local 24-hour wall clock
 * `hh:mm:ss`. Non-finite input renders as `--:--:--`.
 *
 * @param {number} ms unix epoch milliseconds
 * @returns {string} `hh:mm:ss` (zero-padded, local time)
 */
export function formatClock(ms) {
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "--:--:--";
  const pad = (/** @type {number} */ n) => String(n).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
