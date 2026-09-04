//! Pure-Rust conformance tests over the generated JTD validators.
//!
//! Ported from `tests/wire_protocol.rs`: the serde-serialized `ServerMsg`
//! fixtures (valid cases) plus the `bad_case_a..j` adversarial mutations.
//! These run BEFORE the crate is compiled to wasm (`make wasm-validators`),
//! so the browser runs exactly the validation logic proven here.

use serde_json::{json, Value};

use crate::generated::{ack, assistant, error, pong, ready, rename, session_meta, tool_call};

/// The serde-serialized `ServerMsg` fixtures from `sample_msg` in
/// `tests/wire_protocol.rs` (the `_type` serde tag matches the schema enum).
fn valid_fixture(variant: &str) -> Value {
    match variant {
        "ready" => json!({
            "_type": "ready",
            "version": "0.1.1",
            "websocket_path": "/ws",
        }),
        "pong" => json!({ "_type": "pong", "id": "req_1" }),
        "assistant" => json!({
            "_type": "assistant",
            "id": "req_1",
            "text": "hello",
        }),
        "error" => json!({
            "_type": "error",
            "id": null,
            "message": "boom",
        }),
        "tool_call" => json!({
            "_type": "tool_call",
            "id": null,
            "session_id": "sess_1",
            "tool": "WebSearch",
            "args_pretty": "{\n  \"query\": \"rust\"\n}",
            "result_pretty": "\"https://rust-lang.org\"",
            "bytes_up": 20,
            "bytes_down": 128,
            "duration_ms": 350,
            "ts": 1_700_000_000_000u64,
        }),
        "session_meta" => json!({
            "_type": "session_meta",
            "session_id": "sess_1",
            "title": "axonerai",
            "created_at": 1_700_000_000_000u64,
        }),
        "ack" => json!({
            "_type": "ack",
            "for_type": "rename",
            "ok": true,
            "message": null,
        }),
        "rename" => json!({
            "_type": "rename",
            "title": "renamed session",
        }),
        other => panic!("unknown variant: {other}"),
    }
}

#[test]
fn valid_fixtures_validate_clean() {
    let cases: Vec<(&str, Value, fn(&Value) -> Vec<(String, String)>)> = vec![
        ("ready", valid_fixture("ready"), ready::validate),
        ("pong", valid_fixture("pong"), pong::validate),
        ("assistant", valid_fixture("assistant"), assistant::validate),
        ("error", valid_fixture("error"), error::validate),
        ("tool_call", valid_fixture("tool_call"), tool_call::validate),
        (
            "session_meta",
            valid_fixture("session_meta"),
            session_meta::validate,
        ),
        ("ack", valid_fixture("ack"), ack::validate),
        ("rename", valid_fixture("rename"), rename::validate),
    ];
    for (variant, value, validate) in cases {
        assert!(
            validate(&value).is_empty(),
            "{variant} fixture must have zero JTD errors"
        );
    }
}

#[test]
fn valid_pong_with_null_id_validates_clean() {
    let value = json!({ "_type": "pong", "id": null });
    assert!(pong::validate(&value).is_empty(), "nullable id must accept null");
}

#[test]
fn valid_ack_with_null_message_validates_clean() {
    let value = json!({
        "_type": "ack",
        "for_type": "rename",
        "ok": true,
        "message": null,
    });
    assert!(ack::validate(&value).is_empty());
}

/// The browser's catch-up reconstruction synthesizes a partial `tool_call`
/// with an optional `abridged: true` flag (the pretty heads ARE truncated);
/// the schema sanctions that flag as optional, so it must validate clean.
#[test]
fn valid_tool_call_with_optional_abridged_flag_validates_clean() {
    let mut value = valid_fixture("tool_call");
    value["abridged"] = json!(true);
    assert!(
        tool_call::validate(&value).is_empty(),
        "optional abridged flag must be accepted"
    );
    let mut absent = valid_fixture("tool_call");
    absent
        .as_object_mut()
        .expect("fixture is an object")
        .remove("abridged");
    assert!(
        tool_call::validate(&absent).is_empty(),
        "absent abridged flag must be accepted"
    );
}

#[test]
fn bad_case_abridged_as_string_on_tool_call() {
    let mut bad = valid_fixture("tool_call");
    bad["abridged"] = json!("yes");
    assert!(
        !tool_call::validate(&bad).is_empty(),
        "tool_call with a non-boolean abridged must produce errors"
    );
}

#[test]
fn bad_case_a_wrong_type_constant_on_ready() {
    let mut bad = valid_fixture("ready");
    bad["_type"] = json!("wrong");
    assert!(
        !ready::validate(&bad).is_empty(),
        "ready with wrong _type constant must produce errors"
    );
}

#[test]
fn bad_case_b_missing_text_on_assistant() {
    let mut bad = valid_fixture("assistant");
    bad.as_object_mut().expect("fixture is an object").remove("text");
    assert!(
        !assistant::validate(&bad).is_empty(),
        "assistant without text must produce errors"
    );
}

#[test]
fn bad_case_c_id_as_number_on_pong() {
    let mut bad = valid_fixture("pong");
    bad["id"] = json!(42);
    assert!(
        !pong::validate(&bad).is_empty(),
        "pong with numeric id must produce errors"
    );
}

#[test]
fn bad_case_d_extra_field_on_ready() {
    let mut bad = valid_fixture("ready");
    bad["unexpected"] = json!("nope");
    let errors = ready::validate(&bad);
    assert!(
        !errors.is_empty(),
        "ready with an extra field must produce errors (additionalProperties: false)"
    );
    // The generated validator mirrors the mjs error shape: the extra-key
    // error carries the instance path of the offending property.
    assert!(
        errors.iter().any(|(ip, _)| ip == "/unexpected"),
        "extra-field error must point at /unexpected, got: {errors:?}"
    );
}

#[test]
fn bad_case_e_text_as_number_on_assistant() {
    let mut bad = valid_fixture("assistant");
    bad["text"] = json!(123);
    assert!(
        !assistant::validate(&bad).is_empty(),
        "assistant with numeric text must produce errors"
    );
}

#[test]
fn bad_case_f_wrong_type_on_tool_call() {
    let mut bad = valid_fixture("tool_call");
    bad["_type"] = json!("toolcal");
    assert!(
        !tool_call::validate(&bad).is_empty(),
        "tool_call with wrong _type constant must produce errors"
    );
}

#[test]
fn bad_case_g_missing_tool_on_tool_call() {
    let mut bad = valid_fixture("tool_call");
    bad.as_object_mut().expect("fixture is an object").remove("tool");
    assert!(
        !tool_call::validate(&bad).is_empty(),
        "tool_call without the tool field must produce errors"
    );
}

#[test]
fn bad_case_h_args_pretty_as_number_on_tool_call() {
    let mut bad = valid_fixture("tool_call");
    bad["args_pretty"] = json!(42);
    assert!(
        !tool_call::validate(&bad).is_empty(),
        "tool_call with numeric args_pretty must produce errors"
    );
}

#[test]
fn bad_case_i_ok_as_string_on_ack() {
    let mut bad = valid_fixture("ack");
    bad["ok"] = json!("yes");
    assert!(
        !ack::validate(&bad).is_empty(),
        "ack with string ok must produce errors"
    );
}

#[test]
fn bad_case_j_extra_field_on_session_meta() {
    let mut bad = valid_fixture("session_meta");
    bad["unexpected"] = json!(1);
    assert!(
        !session_meta::validate(&bad).is_empty(),
        "session_meta with an extra field must produce errors (additionalProperties: false)"
    );
}

#[test]
fn bad_case_k_non_object_instance_is_rejected() {
    // The mjs validators reject non-objects; the generated Rust must too.
    for instance in [json!(null), json!(42), json!("x"), json!([])] {
        assert!(
            !ready::validate(&instance).is_empty(),
            "non-object instance must produce errors, got clean for: {instance}"
        );
    }
}

#[test]
fn bad_case_l_uint32_boundaries_on_tool_call() {
    // `bytes_*`/`duration_ms` are uint32: negative and >u32::MAX must fail;
    // the exact boundaries must pass.
    let negative = json!({
        "_type": "tool_call",
        "id": null,
        "session_id": "s",
        "tool": "t",
        "args_pretty": "a",
        "result_pretty": "r",
        "bytes_up": -1,
        "bytes_down": 128,
        "duration_ms": 350,
        "ts": 1.0,
    });
    assert!(
        !tool_call::validate(&negative).is_empty(),
        "negative uint32 must produce errors"
    );

    let overflow = json!({
        "_type": "tool_call",
        "id": null,
        "session_id": "s",
        "tool": "t",
        "args_pretty": "a",
        "result_pretty": "r",
        "bytes_up": 4294967296f64,
        "bytes_down": 128,
        "duration_ms": 350,
        "ts": 1.0,
    });
    assert!(
        !tool_call::validate(&overflow).is_empty(),
        "uint32 overflow must produce errors"
    );

    let boundary = json!({
        "_type": "tool_call",
        "id": null,
        "session_id": "s",
        "tool": "t",
        "args_pretty": "a",
        "result_pretty": "r",
        "bytes_up": 4294967295f64,
        "bytes_down": 0,
        "duration_ms": 0,
        "ts": 0.0,
    });
    assert!(
        tool_call::validate(&boundary).is_empty(),
        "uint32 boundary values must validate clean"
    );
}

#[test]
fn error_tuples_carry_instance_and_schema_paths() {
    // The browser drop semantics log `{instancePath, schemaPath}`; the
    // generated Rust returns the same pair as a tuple.
    let mut bad = valid_fixture("assistant");
    bad["text"] = json!(123);
    let errors = assistant::validate(&bad);
    assert!(errors.len() >= 1);
    let (ip, sp) = &errors[0];
    assert_eq!(ip, "/text", "instancePath must point at the bad property");
    assert_eq!(
        sp, "/properties/text/type",
        "schemaPath must point at the failing schema keyword"
    );
}
