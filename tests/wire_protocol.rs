//! Rust-side conformance tests for the wire protocol.
//!
//! Each `ServerMsg` variant is serialized with serde, written to a fixture
//! file under `.tmp/wire-fixtures/`, and validated against the matching JTD
//! schema in `schemas/<variant>.jdt.json` using the `jtd` crate (RFC 8927).
//! Bad-data mutations stay in memory and must produce non-empty error lists.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use axonerai::wire::ServerMsg;
use serde_json::{Value, json};

const VARIANTS: [&str; 4] = ["ready", "pong", "assistant", "error"];

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
        other => panic!("unknown variant: {other}"),
    }
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
