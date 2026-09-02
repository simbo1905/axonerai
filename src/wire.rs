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
#[derive(Deserialize, Debug, Clone, PartialEq)]
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
    /// MANUAL Serialize: these payload fields serialize LAST so metadata
    /// survives a truncated line (see `impl Serialize for ServerMsg`).
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
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "_type", rename_all = "snake_case")]
pub enum RolloutRecord {
    /// schemas/session_rename.jdt.json
    SessionRename { title: String, ts: u64 },
    /// Full-fidelity tool trace — the rollout counterpart of the abridged
    /// `ServerMsg::ToolCall` frame. `args_json`/`result_json` are pretty JSON.
    /// MANUAL Serialize: these payload fields serialize LAST so metadata
    /// survives a truncated line (see `impl Serialize for RolloutRecord`).
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

/// Manual serialization with a fixed field order: the `_type` tag and the
/// metadata fields FIRST, the big payload fields (`args_pretty`/`result_pretty`,
/// resp. `args_json`/`result_json`) LAST. If a crash truncates the rollout
/// line mid-payload, everything before the cut is still a usable, parseable
/// prefix of the metadata. Deserialization is field-order independent.
impl Serialize for ServerMsg<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        match self {
            ServerMsg::Ready {
                version,
                websocket_path,
            } => {
                let mut s = serializer.serialize_struct("Ready", 3)?;
                s.serialize_field("_type", "ready")?;
                s.serialize_field("version", version)?;
                s.serialize_field("websocket_path", websocket_path)?;
                s.end()
            }
            ServerMsg::Pong { id } => {
                let mut s = serializer.serialize_struct("Pong", 2)?;
                s.serialize_field("_type", "pong")?;
                s.serialize_field("id", id)?;
                s.end()
            }
            ServerMsg::Assistant { id, text } => {
                let mut s = serializer.serialize_struct("Assistant", 3)?;
                s.serialize_field("_type", "assistant")?;
                s.serialize_field("id", id)?;
                s.serialize_field("text", text)?;
                s.end()
            }
            ServerMsg::Error { id, message } => {
                let mut s = serializer.serialize_struct("Error", 3)?;
                s.serialize_field("_type", "error")?;
                s.serialize_field("id", id)?;
                s.serialize_field("message", message)?;
                s.end()
            }
            ServerMsg::ToolCall {
                id,
                session_id,
                tool,
                args_pretty,
                result_pretty,
                bytes_up,
                bytes_down,
                duration_ms,
                ts,
            } => {
                let mut s = serializer.serialize_struct("ToolCall", 10)?;
                s.serialize_field("_type", "tool_call")?;
                s.serialize_field("id", id)?;
                s.serialize_field("session_id", session_id)?;
                s.serialize_field("tool", tool)?;
                s.serialize_field("bytes_up", bytes_up)?;
                s.serialize_field("bytes_down", bytes_down)?;
                s.serialize_field("duration_ms", duration_ms)?;
                s.serialize_field("ts", ts)?;
                // Payloads LAST: a crash-truncated line keeps usable metadata.
                s.serialize_field("args_pretty", args_pretty)?;
                s.serialize_field("result_pretty", result_pretty)?;
                s.end()
            }
            ServerMsg::SessionMeta {
                session_id,
                title,
                created_at,
            } => {
                let mut s = serializer.serialize_struct("SessionMeta", 4)?;
                s.serialize_field("_type", "session_meta")?;
                s.serialize_field("session_id", session_id)?;
                s.serialize_field("title", title)?;
                s.serialize_field("created_at", created_at)?;
                s.end()
            }
            ServerMsg::Ack {
                for_type,
                ok,
                message,
            } => {
                let mut s = serializer.serialize_struct("Ack", 4)?;
                s.serialize_field("_type", "ack")?;
                s.serialize_field("for_type", for_type)?;
                s.serialize_field("ok", ok)?;
                s.serialize_field("message", message)?;
                s.end()
            }
        }
    }
}

/// Manual serialization for rollout records — same payload-last contract as
/// [`ServerMsg`]: metadata first, `args_json`/`result_json` LAST.
impl Serialize for RolloutRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        match self {
            RolloutRecord::SessionRename { title, ts } => {
                let mut s = serializer.serialize_struct("SessionRename", 3)?;
                s.serialize_field("_type", "session_rename")?;
                s.serialize_field("title", title)?;
                s.serialize_field("ts", ts)?;
                s.end()
            }
            RolloutRecord::ToolTrace {
                tool,
                args_json,
                result_json,
                bytes_up,
                bytes_down,
                duration_ms,
                ts,
            } => {
                let mut s = serializer.serialize_struct("ToolTrace", 8)?;
                s.serialize_field("_type", "tool_trace")?;
                s.serialize_field("tool", tool)?;
                s.serialize_field("bytes_up", bytes_up)?;
                s.serialize_field("bytes_down", bytes_down)?;
                s.serialize_field("duration_ms", duration_ms)?;
                s.serialize_field("ts", ts)?;
                // Payloads LAST: a crash-truncated line keeps usable metadata.
                s.serialize_field("args_json", args_json)?;
                s.serialize_field("result_json", result_json)?;
                s.end()
            }
        }
    }
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

    /// The payload-last contract: big data fields serialize LAST so metadata
    /// survives a crash truncation of the line. Asserted on the byte stream
    /// (serde_json::to_string), not on Value, because Value loses order.
    #[test]
    fn tool_trace_serializes_metadata_before_payload() {
        let record = RolloutRecord::ToolTrace {
            tool: "WebSearch".to_string(),
            args_json: "{}".to_string(),
            result_json: "\"https://example.com\"".to_string(),
            bytes_up: 20,
            bytes_down: 128,
            duration_ms: 350,
            ts: 1_700_000_000_000,
        };
        let bytes = serde_json::to_string(&record).unwrap();
        let pos = |needle: &str| bytes.find(needle).expect(needle);
        assert!(
            pos("\"tool\"") < pos("\"args_json\""),
            "args_json after tool: {bytes}"
        );
        assert!(
            pos("\"duration_ms\"") < pos("\"args_json\""),
            "metadata duration_ms must come before payload args_json: {bytes}"
        );
        assert!(
            pos("\"args_json\"") < pos("\"result_json\""),
            "result_json must serialize LAST: {bytes}"
        );
        // JSON shape stays identical (order-independent view).
        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["_type"], "tool_trace");
        assert_eq!(value["tool"], "WebSearch");
        assert_eq!(value["bytes_up"], 20);
        assert_eq!(value["duration_ms"], 350);
        let back: RolloutRecord = serde_json::from_value(value).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn tool_call_serializes_metadata_before_payload() {
        let msg = ServerMsg::ToolCall {
            id: None,
            session_id: "sess_1",
            tool: "WebSearch",
            args_pretty: "{}",
            result_pretty: "\"https://example.com\"",
            bytes_up: 20,
            bytes_down: 128,
            duration_ms: 350,
            ts: 1_700_000_000_000,
        };
        let bytes = serde_json::to_string(&msg).unwrap();
        let pos = |needle: &str| bytes.find(needle).expect(needle);
        assert!(
            pos("\"session_id\"") < pos("\"tool\""),
            "tag metadata first: {bytes}"
        );
        assert!(
            pos("\"tool\"") < pos("\"args_pretty\""),
            "args_pretty after tool: {bytes}"
        );
        assert!(
            pos("\"duration_ms\"") < pos("\"args_pretty\""),
            "metadata duration_ms must come before payload args_pretty: {bytes}"
        );
        assert!(
            pos("\"args_pretty\"") < pos("\"result_pretty\""),
            "result_pretty must serialize LAST: {bytes}"
        );
        // JSON shape stays identical (order-independent view).
        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["_type"], "tool_call");
        assert_eq!(value["id"], serde_json::Value::Null);
        assert_eq!(value["ts"], 1_700_000_000_000u64);
    }
}
