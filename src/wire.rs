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
    /// Per-tool-call trace event. `args_pretty`/`result_pretty` are already
    /// abridged (≤ egress limit) by the sender; the rollout keeps full text.
    ToolCall {
        id: Option<&'a str>,
        session_id: &'a str,
        tool: &'a str,
        args_pretty: &'a str,
        result_pretty: &'a str,
        bytes_up: usize,
        bytes_down: usize,
        duration_ms: u64,
        ts: u64,
    },
    /// Session metadata (id, title, creation time). Also the on-disk first
    /// record of a fresh rollout — the shape must stay identical to the
    /// provisional JSON line it replaces so existing rollouts stay readable.
    SessionMeta {
        session_id: &'a str,
        title: &'a str,
        created_at: u64,
    },
    /// Generic acknowledgement (rename, future settings ops).
    Ack {
        for_type: &'a str,
        ok: bool,
        message: Option<&'a str>,
    },
}

/// Rollout-internal records. Formalised by JTD schemas where noted but never
/// broadcast as wire events — the renaming client gets a `ServerMsg::Ack`
/// instead, and tool traces go out abridged as `ServerMsg::ToolCall`.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "_type", rename_all = "snake_case")]
pub enum RolloutRecord {
    /// schemas/session_rename.jdt.json
    SessionRename { title: String, ts: u64 },
    /// Full-fidelity tool trace — the rollout counterpart of the abridged
    /// `ServerMsg::ToolCall` frame. `args_json`/`result_json` are pretty JSON.
    ToolTrace {
        tool: String,
        args_json: String,
        result_json: String,
        bytes_up: usize,
        bytes_down: usize,
        duration_ms: u64,
        ts: u64,
    },
}

/// Events sent by the browser to the server over the `/ws` WebSocket.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "_type", rename_all = "snake_case")]
pub enum ClientMsg {
    Prompt { id: Option<String>, text: String },
    Ping { id: Option<String> },
    Rename { title: String },
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

    #[test]
    fn tool_call_serializes_with_type_tag() {
        let msg = ServerMsg::ToolCall {
            id: None,
            session_id: "sess_1",
            tool: "WebSearch",
            args_pretty: "{\n  \"query\": \"rust\"\n}",
            result_pretty: "\"https://rust-lang.org\"",
            bytes_up: 20,
            bytes_down: 128,
            duration_ms: 350,
            ts: 1_700_000_000_000,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["_type"], "tool_call");
        assert_eq!(json["session_id"], "sess_1");
        assert_eq!(json["tool"], "WebSearch");
        assert_eq!(json["bytes_up"], 20);
        assert_eq!(json["duration_ms"], 350);
        assert_eq!(json["ts"], 1_700_000_000_000u64);
        assert!(json.get("id").is_some(), "id must serialize as null");
        assert_eq!(json["id"], serde_json::Value::Null);
    }

    #[test]
    fn session_meta_serializes_with_item25_on_disk_shape() {
        let msg = ServerMsg::SessionMeta {
            session_id: "sess_1",
            title: "axonerai",
            created_at: 1_700_000_000_000,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "_type": "session_meta",
                "session_id": "sess_1",
                "title": "axonerai",
                "created_at": 1_700_000_000_000u64,
            }),
            "on-disk shape must match item25's provisional JSON line"
        );
    }

    #[test]
    fn ack_serializes_with_type_tag() {
        let msg = ServerMsg::Ack {
            for_type: "rename",
            ok: true,
            message: None,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["_type"], "ack");
        assert_eq!(json["for_type"], "rename");
        assert_eq!(json["ok"], true);
        assert_eq!(json["message"], serde_json::Value::Null);
    }

    #[test]
    fn client_rename_deserializes_with_type_tag() {
        let msg: ClientMsg =
            serde_json::from_str(r#"{"_type":"rename","title":"new name"}"#).unwrap();
        match msg {
            ClientMsg::Rename { title } => assert_eq!(title, "new name"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rollout_record_session_rename_serializes_with_type_tag() {
        let record = RolloutRecord::SessionRename {
            title: "renamed".to_string(),
            ts: 1_700_000_000_000,
        };
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "_type": "session_rename",
                "title": "renamed",
                "ts": 1_700_000_000_000u64,
            })
        );
        let back: RolloutRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back, record);
    }
}
