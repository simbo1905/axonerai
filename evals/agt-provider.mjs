// promptfoo custom ESM provider: talks to a local axoner-web server over
// WebSocket. Config: { port: number, label: string }.
//
// Protocol (src/wire.rs): server sends a `ready` frame on connect; the client
// sends {"_type":"prompt","id":...,"text":...} and collects frames until an
// `assistant` (success) or `error` frame with the matching id (a null id on
// the frame matches any). Overall timeout is 120s. On transport failure the
// round-trip is retried once after a 5s sleep. The socket is always closed.

const OVERALL_TIMEOUT_MS = 120_000;
const RETRY_DELAY_MS = 5_000;

const EVAL_TIMEOUT = "EVAL_TIMEOUT";

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function openSocket(port, deadline) {
  return new Promise((resolve, reject) => {
    let ws;
    const timer = setTimeout(() => {
      cleanup();
      try {
        ws?.close();
      } catch {
        // ignore
      }
      reject(timeoutError());
    }, Math.max(deadline - Date.now(), 0));
    function cleanup() {
      clearTimeout(timer);
    }
    try {
      ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    } catch (err) {
      cleanup();
      reject(err);
      return;
    }
    ws.addEventListener("open", () => {
      cleanup();
      resolve(ws);
    });
    ws.addEventListener("error", () => {
      cleanup();
      reject(new Error("websocket connection failed"));
    });
    ws.addEventListener("close", () => {
      cleanup();
      reject(new Error("websocket closed before opening"));
    });
  });
}

function timeoutError() {
  const err = new Error("timeout");
  err.code = EVAL_TIMEOUT;
  return err;
}

function parseFrame(data) {
  if (typeof data !== "string") return undefined;
  try {
    const frame = JSON.parse(data);
    return frame && typeof frame._type === "string" ? frame : undefined;
  } catch {
    return undefined;
  }
}

/** Wait for the next frame matching `predicate`, or reject on deadline/transport failure. */
function waitForFrame(ws, predicate, deadline) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      cleanup();
      reject(timeoutError());
    }, Math.max(deadline - Date.now(), 0));
    function cleanup() {
      clearTimeout(timer);
      ws.removeEventListener("message", onMessage);
      ws.removeEventListener("error", onError);
      ws.removeEventListener("close", onClose);
    }
    function onMessage(event) {
      const frame = parseFrame(event.data);
      if (frame && predicate(frame)) {
        cleanup();
        resolve(frame);
      }
    }
    function onError() {
      cleanup();
      reject(new Error("websocket error"));
    }
    function onClose() {
      cleanup();
      reject(new Error("websocket closed unexpectedly"));
    }
    ws.addEventListener("message", onMessage);
    ws.addEventListener("error", onError);
    ws.addEventListener("close", onClose);
  });
}

export default class AgtWsProvider {
  constructor({ config }) {
    if (!config || typeof config.port !== "number") {
      throw new Error("agt-provider requires config.port (number)");
    }
    this.port = config.port;
    this.label = config.label ?? `agt-ws-${this.port}`;
    this.counter = 0;
  }

  id() {
    return `agt-ws-${this.port}`;
  }

  toString() {
    return this.label;
  }

  async callApi(prompt) {
    for (let attempt = 0; ; attempt++) {
      try {
        return await this.attemptOnce(prompt);
      } catch (err) {
        if (err && err.code === EVAL_TIMEOUT) {
          return { error: "timeout" };
        }
        if (attempt >= 1) {
          return { error: `transport failure: ${err?.message ?? String(err)}` };
        }
        await sleep(RETRY_DELAY_MS);
      }
    }
  }

  async attemptOnce(prompt) {
    const deadline = Date.now() + OVERALL_TIMEOUT_MS;
    const ws = await openSocket(this.port, deadline);
    try {
      const ready = await waitForFrame(ws, (f) => f._type === "ready", deadline);
      if (ready._type !== "ready") {
        throw new Error(`expected ready frame, got ${ready._type}`);
      }
      const id = `eval_${++this.counter}_${Math.random().toString(36).slice(2, 8)}`;
      ws.send(JSON.stringify({ _type: "prompt", id, text: prompt }));
      const frame = await waitForFrame(
        ws,
        (f) =>
          (f._type === "assistant" || f._type === "error") &&
          (f.id === id || f.id == null),
        deadline,
      );
      if (frame._type === "error") {
        return { output: `[error] ${frame.message ?? "unknown error"}` };
      }
      return { output: frame.text ?? "" };
    } finally {
      try {
        ws.close();
      } catch {
        // ignore
      }
    }
  }
}
