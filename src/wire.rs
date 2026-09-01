//! Wire protocol for the agt serve WebSocket channel.
//!
//! Every event is a separate type on the wire, discriminated by a fixed
//! `_type` string that mirrors the Rust enum variant. Each variant has a
//! matching JTD schema in `schemas/<variant>.jdt.json`; the generated
//! JavaScript validators in `web/generated/` validate incoming frames
//! against those schemas. Rust-side conformance tests live in
//! `tests/wire_protocol.rs` using the same schemas via the `jtd` crate.

use serde::{Deserialize, Serialize};

/// Events sent by the server to the browser over the `/ws` WebSocket.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "_type", rename_all = "snake_case")]
pub enum ServerMsg<'a> {
    Ready {
        version: &'a str,
        websocket_path: &'a str,
    },
    Pong {
        id: Option<&'a str>,
    },
    Assistant {
        id: Option<&'a str>,
        text: &'a str,
    },
    Error {
        id: Option<&'a str>,
        message: &'a str,
    },
}

/// Events sent by the browser to the server over the `/ws` WebSocket.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "_type", rename_all = "snake_case")]
pub enum ClientMsg {
    Prompt { id: Option<String>, text: String },
    Ping { id: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_serializes_with_type_tag() {
        let msg = ServerMsg::Ready {
            version: "0.1.1",
            websocket_path: "/ws",
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["_type"], "ready");
        assert_eq!(json["version"], "0.1.1");
        assert_eq!(json["websocket_path"], "/ws");
    }

    #[test]
    fn assistant_serializes_with_type_tag() {
        let msg = ServerMsg::Assistant {
            id: Some("req_1"),
            text: "hello",
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["_type"], "assistant");
        assert_eq!(json["id"], "req_1");
        assert_eq!(json["text"], "hello");
    }

    #[test]
    fn client_prompt_deserializes_with_type_tag() {
        let msg: ClientMsg =
            serde_json::from_str(r#"{"_type":"prompt","id":"req_1","text":"hi"}"#).unwrap();
        match msg {
            ClientMsg::Prompt { id, text } => {
                assert_eq!(id.as_deref(), Some("req_1"));
                assert_eq!(text, "hi");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
