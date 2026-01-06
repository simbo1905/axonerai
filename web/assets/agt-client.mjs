export function createClient({ url } = {}) {
  const wsUrl =
    url ||
    (location.protocol === "https:" ? "wss://" : "ws://") +
      location.host +
      "/ws";

  let ws = null;
  /** @type {(msg:any)=>void} */
  let onMessage = () => {};
  /** @type {(ev:Event)=>void} */
  let onOpen = () => {};
  /** @type {(ev:CloseEvent)=>void} */
  let onClose = () => {};
  /** @type {(ev:Event)=>void} */
  let onError = () => {};

  function connect() {
    ws = new WebSocket(wsUrl);
    ws.addEventListener("open", (ev) => onOpen(ev));
    ws.addEventListener("close", (ev) => onClose(ev));
    ws.addEventListener("error", (ev) => onError(ev));
    ws.addEventListener("message", (ev) => {
      try {
        onMessage(JSON.parse(ev.data));
      } catch {
        onMessage({ type: "raw", data: ev.data });
      }
    });
  }

  function send(obj) {
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      throw new Error("websocket not connected");
    }
    ws.send(JSON.stringify(obj));
  }

  function chat(text) {
    send({ type: "chat", text });
  }

  return {
    wsUrl,
    connect,
    close: () => ws?.close(),
    chat,
    send,
    setHandlers: (h) => {
      if (h.onMessage) onMessage = h.onMessage;
      if (h.onOpen) onOpen = h.onOpen;
      if (h.onClose) onClose = h.onClose;
      if (h.onError) onError = h.onError;
    },
  };
}

// Convenience global for Babel/React inline scripts
window.AgtClient = { createClient };

