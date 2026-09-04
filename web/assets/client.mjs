// @ts-check
// Minimal ESM2020 websocket client for agt serve.
// Exposes:
//   - window.AgtClient.connect({ onOpen, onClose, onError, onEvent })
//   - window.AgtClient.sendPrompt(text, id?) -> Promise<string>
//   - window.AgtClient.sendRename(title)
//
// IO-boundary rule (docs/ARCHITECTURE.md): EVERY frame crossing the
// WebSocket is JTD-validated and deep-frozen — incoming via
// {@link parseWireEventText} (wire.mjs owns the validator registry), outgoing
// via the generated validators (`web/generated/validators.mjs`) before
// `send`. An invalid outgoing frame is logged and NOT sent.

import { deepFreeze, parseWireEventText } from "/src/wire.mjs";
import {
  validatePrompt,
  validateRename,
} from "/generated/validators.mjs";

const WS_PATH = "/ws";

/**
 * A single failed JTD validation ({@link validatePrompt}/{@link validateRename}).
 *
 * @typedef {import("/generated/validators.mjs").ValidationError} ValidationError
 */

function wsUrl() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${WS_PATH}`;
}

function randomID() {
  return `req_${Math.random().toString(16).slice(2)}_${Date.now().toString(16)}`;
}

/** @type {WebSocket | null} */
let socket = null;
/** @type {Map<string, { resolve: (text: string) => void, reject: (error: Error) => void }>} */
let pending = new Map(); // id -> { resolve, reject }
/** @type {((event: import("/src/wire.mjs").WireEvent) => void) | null} */
let onEventCb = null; // optional listener receiving validated wire events

/**
 * @param {Error} err
 * @returns {void}
 */
function cleanupPending(err) {
  for (const [, p] of pending) {
    p.reject(err);
  }
  pending.clear();
}

/**
 * @param {{ onOpen?: () => void, onClose?: () => void, onError?: (e: Event) => void, onEvent?: (event: import("/src/wire.mjs").WireEvent) => void }} [options]
 * @returns {Promise<{ dispose: () => void }>}
 */
async function connect({ onOpen, onClose, onError, onEvent } = {}) {
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
    return { dispose };
  }

  onEventCb = onEvent || null;
  socket = new WebSocket(wsUrl());

  socket.onopen = () => onOpen && onOpen();
  socket.onclose = () => {
    cleanupPending(new Error("socket closed"));
    onClose && onClose();
  };
  socket.onerror = (e) => {
    onError && onError(e);
  };

  socket.onmessage = (ev) => {
    // Validate + deep-freeze every incoming frame before any handling.
    // Dropped (null) frames are already logged by wire.mjs, which owns the
    // full validator registry (ready/pong/assistant/error/ack/session_meta/
    // tool_call).
    const msg = parseWireEventText(
      /** @type {MessageEvent<string>} */ (ev).data,
    );
    if (msg === null) {
      return;
    }
    // Hand the frozen event to the UI first, then do client-internal handling.
    if (onEventCb) {
      try {
        onEventCb(msg);
      } catch (e) {
        console.warn("agt: onEvent listener failed", e);
      }
    }
    if (msg._type === "assistant") {
      const id = msg.id || null;
      if (id && pending.has(id)) {
        pending.get(id)?.resolve(msg.text || "");
        pending.delete(id);
      }
      return;
    }
    if (msg._type === "error") {
      const id = msg.id || null;
      const err = new Error(msg.message || "error");
      if (id && pending.has(id)) {
        pending.get(id)?.reject(err);
        pending.delete(id);
      } else {
        // Nothing pending; surface on console.
        console.error(err);
      }
      return;
    }
    // ready/pong/etc: already dispatched to the UI listener above.
  };

  // Wait briefly for connection establishment.
  await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("timeout connecting websocket")), 8000);
    socket?.addEventListener("open", () => {
      clearTimeout(t);
      resolve(/** @type {void} */ (undefined));
    }, { once: true });
    socket?.addEventListener("error", () => {
      clearTimeout(t);
      reject(new Error("websocket error"));
    }, { once: true });
  });

  return { dispose };
}

/** @returns {void} */
function dispose() {
  if (!socket) return;
  try {
    socket.close();
  } catch (_) {}
  socket = null;
}

/**
 * Send a `prompt` frame: JTD-validate + deep-freeze the outgoing payload
 * BEFORE `send`; an invalid frame is logged and rejected without touching
 * the socket (the reply resolves/rejects the pending promise by id).
 *
 * @param {string} text
 * @param {string} [id] prompt id; a random one is generated when omitted
 * @returns {Promise<string>} resolves with the matching `assistant` text
 */
async function sendPrompt(text, id) {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    throw new Error("not connected");
  }
  const wireId = typeof id === "string" && id.length > 0 ? id : randomID();
  const payload = deepFreeze({ _type: "prompt", id: wireId, text });
  const errors = validatePrompt(payload);
  if (errors.length > 0) {
    console.error("[client] malformed outgoing prompt frame", errors, payload);
    throw new Error("malformed outgoing prompt frame");
  }

  const p = new Promise((resolve, reject) => {
    pending.set(wireId, { resolve, reject });
  });

  socket.send(JSON.stringify(payload));
  return p;
}

/**
 * Send a `rename` control-plane frame: JTD-validate + deep-freeze the
 * outgoing payload BEFORE `send`; an invalid frame is logged and NOT sent
 * (the reply arrives as an `ack` event with for_type === "rename", delivered
 * to the UI via onEvent).
 *
 * @param {string} title
 * @returns {void}
 */
function sendRename(title) {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    throw new Error("not connected");
  }
  const payload = deepFreeze({ _type: "rename", title });
  const errors = validateRename(payload);
  if (errors.length > 0) {
    console.error("[client] malformed outgoing rename frame", errors, payload);
    return;
  }
  socket.send(JSON.stringify(payload));
}

window.AgtClient = {
  connect,
  dispose,
  sendPrompt,
  sendRename,
};
