// @ts-check

/**
 * Typed slash-command registry + parser. Pure module: no DOM, no network —
 * node-testable. The composer uses {@link filterCommands} for its menu and
 * {@link parseInput} to classify input; agt-app implements the `run` side of
 * each command (fetch/WS control plane — commands NEVER go to the model).
 *
 * Run signatures (implemented in web/src/components/agt-app.js):
 * - model:     `run() => Promise<string>` — GET /api/state →
 *              `model: <model> (provider: <provider>)` into the panel Slash.
 * - built-ins: `run() => void` — opens the panel Built-ins tree.
 * - verbose:   `run() => string` — toggles the UI verbose flag and reports
 *              `verbose: on|off`; fires `agt-verbose-changed` on window.
 * - rename:    `run(title: string) => Promise<string>` — WS rename, waits for
 *              the server ack, reports `renamed: <title>` or the ack error.
 * - help:      `run() => string` — the command list with one-line
 *              descriptions.
 * - console:   `run() => void` — opens the devtools console popup
 *              (/console.html). Since item32 all slash RESULTS go to the
 *              console bus; the panel Slash tree keeps only the invocation
 *              echo.
 */

/**
 * Metadata for one slash command.
 *
 * @typedef {object} CommandMeta
 * @property {string} name Lowercase command name (without the leading `/`).
 * @property {string} description One-line description for the menu and /help.
 * @property {boolean} [argsRequired] When true, `parseInput` reports
 *   `missing-args` if the command is invoked without arguments.
 */

/**
 * A successful command parse.
 *
 * @typedef {object} CommandInput
 * @property {"command"} kind
 * @property {string} name Command name as typed (`""` for `/` alone).
 * @property {string} args Remainder after the name, whitespace-collapsed.
 * @property {string} [error] `empty` | `unknown` | `missing-args`.
 */

/**
 * Plain chat input (not a command).
 *
 * @typedef {object} ChatInput
 * @property {"chat"} kind
 */

/** @typedef {ChatInput | CommandInput} ParsedInput */

/**
 * The v1 command registry (order = menu order).
 *
 * @type {ReadonlyArray<Readonly<CommandMeta>>}
 */
export const COMMANDS = Object.freeze([
  Object.freeze({
    name: "model",
    description: "show the current model and provider",
  }),
  Object.freeze({
    name: "built-ins",
    description: "show the built-in tools with on/off toggles",
  }),
  Object.freeze({
    name: "verbose",
    description: "toggle verbose output rendering",
  }),
  Object.freeze({
    name: "rename",
    description: "rename the session: /rename <title>",
    argsRequired: true,
  }),
  Object.freeze({
    name: "help",
    description: "list the available commands",
  }),
  Object.freeze({
    name: "console",
    description: "open the devtools console popup",
  }),
]);

/**
 * @param {string} name
 * @returns {Readonly<CommandMeta> | undefined}
 */
function findCommand(name) {
  return COMMANDS.find((command) => command.name === name);
}

/**
 * Classify raw composer input. Only input whose FIRST character is `/` is a
 * command; everything else (including a slash mid-text) is chat.
 *
 * @param {string} text raw composer text
 * @returns {ParsedInput}
 */
export function parseInput(text) {
  const trimmed = typeof text === "string" ? text.trim() : "";
  if (!trimmed.startsWith("/")) {
    return { kind: "chat" };
  }
  const body = trimmed.slice(1).trim();
  if (body === "") {
    return { kind: "command", name: "", args: "", error: "empty" };
  }
  const parts = body.split(/\s+/);
  const name = parts[0];
  const args = parts.slice(1).join(" ");
  const meta = findCommand(name);
  if (!meta) {
    return { kind: "command", name, args, error: "unknown" };
  }
  if (meta.argsRequired && args === "") {
    return { kind: "command", name, args, error: "missing-args" };
  }
  return { kind: "command", name, args };
}

/**
 * Commands whose name starts with `prefix` (a leading `/` on the prefix is
 * tolerated; matching is case-insensitive). Used by the composer's slash
 * menu — `filterCommands("")` returns every command.
 *
 * @param {string} prefix
 * @returns {ReadonlyArray<Readonly<CommandMeta>>}
 */
export function filterCommands(prefix) {
  const needle = (typeof prefix === "string" ? prefix : "")
    .replace(/^\//, "")
    .toLowerCase();
  return COMMANDS.filter((command) => command.name.startsWith(needle));
}
