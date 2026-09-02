// Minimal ESM2020 websocket client for agt serve.
// Exposes:
//   - window.AgtClient.connect({ onOpen, onClose, onError, onEvent })
//   - window.AgtClient.sendPrompt(text, id?) -> Promise<string>

import { parseWireEventText } from "/src/wire.mjs";

const WS_PATH = "/ws";

function wsUrl() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${WS_PATH}`;
}

function randomID() {
  return `req_${Math.random().toString(16).slice(2)}_${Date.now().toString(16)}`;
}

let socket = null;
let pending = new Map(); // id -> { resolve, reject }
let onEventCb = null; // optional listener receiving validated wire events

function cleanupPending(err) {
  for (const [, p] of pending) {
    p.reject(err);
  }
  pending.clear();
}

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
    // Dropped (null) frames are already logged by wire.mjs.
    const msg = parseWireEventText(ev.data);
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
        pending.get(id).resolve(msg.text || "");
        pending.delete(id);
      }
      return;
    }
    if (msg._type === "error") {
      const id = msg.id || null;
      const err = new Error(msg.message || "error");
      if (id && pending.has(id)) {
        pending.get(id).reject(err);
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
    socket.addEventListener("open", () => {
      clearTimeout(t);
      resolve();
    }, { once: true });
    socket.addEventListener("error", () => {
      clearTimeout(t);
      reject(new Error("websocket error"));
    }, { once: true });
  });

  return { dispose };
}

function dispose() {
  if (!socket) return;
  try {
    socket.close();
  } catch (_) {}
  socket = null;
}

async function sendPrompt(text, id) {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    throw new Error("not connected");
  }
  const wireId = typeof id === "string" && id.length > 0 ? id : randomID();
  const payload = { _type: "prompt", id: wireId, text };

  const p = new Promise((resolve, reject) => {
    pending.set(wireId, { resolve, reject });
  });

  socket.send(JSON.stringify(payload));
  return p;
}

window.AgtClient = {
  connect,
  dispose,
  sendPrompt,
};

