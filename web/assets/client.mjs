// Minimal ESM2020 websocket client for agt serve.
// Exposes:
//   - window.AgtClient.connect({ onOpen, onClose, onError })
//   - window.AgtClient.sendPrompt(text) -> Promise<string>

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

function cleanupPending(err) {
  for (const [, p] of pending) {
    p.reject(err);
  }
  pending.clear();
}

async function connect({ onOpen, onClose, onError } = {}) {
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
    return { dispose };
  }

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
    try {
      const msg = JSON.parse(ev.data);
      if (msg.type === "assistant") {
        const id = msg.id || null;
        if (id && pending.has(id)) {
          pending.get(id).resolve(msg.text || "");
          pending.delete(id);
        }
        return;
      }
      if (msg.type === "error") {
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
      // ready/pong/etc ignored by UI
    } catch (e) {
      console.error("bad ws message", e);
    }
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

async function sendPrompt(text) {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    throw new Error("not connected");
  }
  const id = randomID();
  const payload = { type: "prompt", id, text };

  const p = new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
  });

  socket.send(JSON.stringify(payload));
  return p;
}

window.AgtClient = {
  connect,
  dispose,
  sendPrompt,
};

