//! Rust-side conformance tests for the wire protocol.
//!
//! Each `ServerMsg` variant is serialized with serde, written to a fixture
//! file under `.tmp/wire-fixtures/`, and validated against the matching JTD
//! schema in `schemas/<variant>.jdt.json` using the `jtd` crate (RFC 8927).
//! Bad-data mutations stay in memory and must produce non-empty error lists.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use axonerai::rollout::{self, Rollout};
use axonerai::wire::{ClientMsg, RolloutRecord, ServerMsg};
use serde_json::{Value, json};

const VARIANTS: [&str; 7] = [
    "ready",
    "pong",
    "assistant",
    "error",
    "tool_call",
    "session_meta",
    "ack",
];

/// ServerMsg variants carrying only owned-free borrowed fields are sampled
/// here; client messages and rollout records are covered separately below.
fn sample_msg(variant: &str) -> ServerMsg<'static> {
    match variant {
        "ready" => ServerMsg::Ready {
            version: "0.1.1",
            websocket_path: "/ws",
        },
        "pong" => ServerMsg::Pong { id: Some("req_1") },
        "assistant" => ServerMsg::Assistant {
            id: Some("req_1"),
            text: "hello",
        },
        "error" => ServerMsg::Error {
            id: None,
            message: "boom",
        },
        "tool_call" => ServerMsg::ToolCall {
            id: None,
            session_id: "sess_1",
            tool: "WebSearch",
            args_pretty: "{\n  \"query\": \"rust\"\n}",
            result_pretty: "\"https://rust-lang.org\"",
            bytes_up: 20,
            bytes_down: 128,
            duration_ms: 350,
            ts: 1_700_000_000_000,
        },
        "session_meta" => ServerMsg::SessionMeta {
            session_id: "sess_1",
            title: "axonerai",
            created_at: 1_700_000_000_000,
        },
        "ack" => ServerMsg::Ack {
            for_type: "rename",
            ok: true,
            message: None,
        },
        other => panic!("unknown variant: {other}"),
    }
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".tmp")
        .join("wire-fixtures")
}

fn schema_path(variant: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(format!("{variant}.jdt.json"))
}

fn load_schema(variant: &str) -> jtd::Schema {
    let raw = fs::read_to_string(schema_path(variant))
        .unwrap_or_else(|e| panic!("failed to read schema for {variant}: {e}"));
    let serde_schema: jtd::SerdeSchema = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse schema for {variant}: {e}"));
    jtd::Schema::from_serde_schema(serde_schema)
        .unwrap_or_else(|e| panic!("invalid JTD schema for {variant}: {e}"))
}

fn validate<'a>(
    schema: &'a jtd::Schema,
    instance: &'a Value,
) -> Vec<jtd::ValidationErrorIndicator<'a>> {
    jtd::validate(schema, instance, Default::default()).expect("validate call failed")
}

fn fixture_value(variant: &str) -> Value {
    serde_json::to_value(sample_msg(variant)).expect("serialize ServerMsg")
}

fn load_fixture(variant: &str) -> Value {
    let path = fixture_dir().join(format!("{variant}.json"));
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path:?}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse fixture {path:?}: {e}"))
}

fn write_fixtures() {
    static FIXTURE_LOCK: Mutex<()> = Mutex::new(());
    let _guard = FIXTURE_LOCK.lock().expect("fixture lock poisoned");

    let dir = fixture_dir();
    fs::create_dir_all(&dir).expect("create fixture dir");
    for variant in VARIANTS {
        let path = dir.join(format!("{variant}.json"));
        let pretty =
            serde_json::to_string_pretty(&fixture_value(variant)).expect("pretty-print fixture");
        let tmp = dir.join(format!("{variant}.json.tmp"));
        fs::write(&tmp, pretty).unwrap_or_else(|e| panic!("failed to write {tmp:?}: {e}"));
        fs::rename(&tmp, &path).unwrap_or_else(|e| panic!("failed to finalize {path:?}: {e}"));
    }
}

#[test]
fn fixtures_are_written_and_validate_clean() {
    write_fixtures();

    for variant in VARIANTS {
        let value = fixture_value(variant);
        assert_eq!(
            value["_type"], variant,
            "serde tag for {variant} must match snake_case variant name"
        );

        let schema = load_schema(variant);
        let errors = validate(&schema, &value);
        assert!(
            errors.is_empty(),
            "{variant} fixture must have zero JTD errors, got: {errors:?}"
        );
    }
}

#[test]
fn fixtures_on_disk_validate_against_schemas() {
    write_fixtures();

    for variant in VARIANTS {
        let fixture = load_fixture(variant);
        let schema = load_schema(variant);
        let errors = validate(&schema, &fixture);
        assert!(
            errors.is_empty(),
            "on-disk {variant} fixture must have zero JTD errors, got: {errors:?}"
        );
    }
}

#[test]
fn bad_case_a_wrong_type_constant_on_ready() {
    let schema = load_schema("ready");
    let mut bad = fixture_value("ready");
    bad["_type"] = json!("wrong");
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "ready with wrong _type constant must produce errors"
    );
}

#[test]
fn bad_case_b_missing_text_on_assistant() {
    let schema = load_schema("assistant");
    let mut bad = fixture_value("assistant");
    bad.as_object_mut()
        .expect("fixture is an object")
        .remove("text");
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "assistant without text must produce errors"
    );
}

#[test]
fn bad_case_c_id_as_number_on_pong() {
    let schema = load_schema("pong");
    let mut bad = fixture_value("pong");
    bad["id"] = json!(42);
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "pong with numeric id must produce errors"
    );
}

#[test]
fn bad_case_d_extra_field_on_ready() {
    let schema = load_schema("ready");
    let mut bad = fixture_value("ready");
    bad["unexpected"] = json!("nope");
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "ready with an extra field must produce errors (additionalProperties: false)"
    );
}

#[test]
fn bad_case_e_text_as_number_on_assistant() {
    let schema = load_schema("assistant");
    let mut bad = fixture_value("assistant");
    bad["text"] = json!(123);
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "assistant with numeric text must produce errors"
    );
}

#[test]
fn bad_case_f_wrong_type_on_tool_call() {
    let schema = load_schema("tool_call");
    let mut bad = fixture_value("tool_call");
    bad["_type"] = json!("toolcal");
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "tool_call with wrong _type constant must produce errors"
    );
}

#[test]
fn bad_case_g_missing_tool_on_tool_call() {
    let schema = load_schema("tool_call");
    let mut bad = fixture_value("tool_call");
    bad.as_object_mut()
        .expect("fixture is an object")
        .remove("tool");
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "tool_call without the tool field must produce errors"
    );
}

#[test]
fn bad_case_h_args_pretty_as_number_on_tool_call() {
    let schema = load_schema("tool_call");
    let mut bad = fixture_value("tool_call");
    bad["args_pretty"] = json!(42);
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "tool_call with numeric args_pretty must produce errors"
    );
}

#[test]
fn bad_case_i_ok_as_string_on_ack() {
    let schema = load_schema("ack");
    let mut bad = fixture_value("ack");
    bad["ok"] = json!("yes");
    let errors = validate(&schema, &bad);
    assert!(!errors.is_empty(), "ack with string ok must produce errors");
}

#[test]
fn bad_case_j_extra_field_on_session_meta() {
    let schema = load_schema("session_meta");
    let mut bad = fixture_value("session_meta");
    bad["unexpected"] = json!(1);
    let errors = validate(&schema, &bad);
    assert!(
        !errors.is_empty(),
        "session_meta with an extra field must produce errors (additionalProperties: false)"
    );
}

#[test]
fn client_rename_round_trips_against_schema() {
    let client = ClientMsg::Rename {
        title: "renamed session".to_string(),
    };
    let value = serde_json::to_value(&client).expect("serialize ClientMsg");
    assert_eq!(value["_type"], "rename");

    let raw = fs::read_to_string(schema_path("rename")).expect("failed to read rename schema");
    let serde_schema: jtd::SerdeSchema = serde_json::from_str(&raw).expect("parse rename schema");
    let schema = jtd::Schema::from_serde_schema(serde_schema).expect("invalid JTD rename schema");
    let errors = validate(&schema, &value);
    assert!(
        errors.is_empty(),
        "rename must validate against its JTD schema, got: {errors:?}"
    );

    let back: ClientMsg = serde_json::from_value(value).expect("deserialize ClientMsg");
    assert_eq!(back, client);
}

/// Rollout round-trip: typed session_meta / abridged tool_call / rename
/// records appended, streamed back, shapes asserted; plus the
/// abridge-not-applied-to-rollout full-text invariant for a >1024 result —
/// the wire frame is abridged but the rollout's full-fidelity tool_trace
/// record keeps the whole result.
#[test]
fn rollout_round_trip_and_full_vs_abridged_invariant() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".tmp")
        .join(format!("wire-rollout-{}", uuid::Uuid::new_v4()));
    let session_id = uuid::Uuid::now_v7().to_string();
    let r = Rollout::create(&dir, &session_id).expect("create rollout");

    // session_meta first line (same typed serialization the example writes).
    let meta = ServerMsg::SessionMeta {
        session_id: &session_id,
        title: "wire test",
        created_at: 1_700_000_000_000,
    };
    let meta_value = serde_json::to_value(&meta).unwrap();
    r.append_event(&meta_value).unwrap();

    // A >1024-char tool result: abridged on the wire frame, full in rollout.
    let full_result = serde_json::to_string_pretty(&json!({
        "url": "https://rust-lang.org",
        "body": "x".repeat(2000),
    }))
    .unwrap();
    assert!(full_result.chars().count() > 1024);
    let full_args = "{\n  \"query\": \"rust lang\"\n}".to_string();

    let wire = ServerMsg::ToolCall {
        id: None,
        session_id: &session_id,
        tool: "WebSearch",
        args_pretty: &rollout::abridge(&full_args, 1024),
        result_pretty: &rollout::abridge(&full_result, 1024),
        bytes_up: full_args.len(),
        bytes_down: full_result.len(),
        duration_ms: 512,
        ts: 1_700_000_000_001,
    };
    let wire_value = serde_json::to_value(&wire).unwrap();
    r.append_event(&wire_value).unwrap();

    let trace = RolloutRecord::ToolTrace {
        tool: "WebSearch".to_string(),
        args_json: full_args.clone(),
        result_json: full_result.clone(),
        bytes_up: full_args.len(),
        bytes_down: full_result.len(),
        duration_ms: 512,
        ts: 1_700_000_000_001,
    };
    let trace_value = serde_json::to_value(&trace).unwrap();
    r.append_event(&trace_value).unwrap();

    let rename = RolloutRecord::SessionRename {
        title: "renamed by test".to_string(),
        ts: 1_700_000_000_002,
    };
    let rename_value = serde_json::to_value(&rename).unwrap();
    r.append_event(&rename_value).unwrap();

    // Stream everything back and assert shapes.
    let mut metas = Vec::new();
    let mut tool_calls = Vec::new();
    let mut traces = Vec::new();
    let mut renames = Vec::new();
    r.scan_from(0, |_, line| {
        let v: Value = serde_json::from_str(line).expect("rollout line is JSON");
        match v["_type"].as_str() {
            Some("session_meta") => metas.push(v),
            Some("tool_call") => tool_calls.push(v),
            Some("tool_trace") => traces.push(v),
            Some("session_rename") => renames.push(v),
            other => panic!("unexpected record type: {other:?}"),
        }
        Ok(())
    })
    .unwrap();

    assert_eq!(metas.len(), 1);
    assert_eq!(
        metas[0],
        json!({
            "_type": "session_meta",
            "session_id": session_id,
            "title": "wire test",
            "created_at": 1_700_000_000_000u64,
        })
    );

    assert_eq!(tool_calls.len(), 1);
    let wire_frame = &tool_calls[0];
    let wire_result = wire_frame["result_pretty"].as_str().unwrap();
    assert!(
        wire_result.chars().count() <= 1024,
        "wire frame result_pretty must be abridged to <=1024 chars, got {}",
        wire_result.chars().count()
    );
    assert!(wire_frame["args_pretty"].as_str().unwrap().len() <= 1024);
    assert_eq!(
        wire_frame["bytes_up"].as_u64(),
        Some(full_args.len() as u64)
    );
    assert_eq!(
        wire_frame["bytes_down"].as_u64(),
        Some(full_result.len() as u64)
    );
    assert_eq!(wire_frame["duration_ms"].as_u64(), Some(512));

    assert_eq!(traces.len(), 1);
    let stored_result = traces[0]["result_json"].as_str().unwrap();
    assert_eq!(
        stored_result, full_result,
        "rollout tool_trace record must keep the FULL untruncated result"
    );
    assert_eq!(traces[0]["args_json"].as_str(), Some(full_args.as_str()));
    assert_eq!(traces[0]["duration_ms"].as_u64(), Some(512));

    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0]["title"], json!("renamed by test"));
    assert!(renames[0]["ts"].is_u64());

    // Schemas also validate the full-fidelity record's serialized shape via
    // the serde round-trip of RolloutRecord.
    let back: RolloutRecord = serde_json::from_value(trace_value).unwrap();
    assert_eq!(back, trace);
    let back: RolloutRecord = serde_json::from_value(rename_value).unwrap();
    assert_eq!(back, rename);

    let _ = std::fs::remove_dir_all(&dir);
}
