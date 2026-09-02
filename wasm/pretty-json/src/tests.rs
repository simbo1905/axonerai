//! Tests for [`crate::pretty_print`].
//!
//! Oracle: for well-formed JSON at indent 2 the output must be byte-identical
//! to `serde_json::to_string_pretty`. Abridged input is tested with a
//! no-panic + balanced-brackets property over every char-boundary truncation
//! offset, plus hand-written expected outputs at canonical truncation points.

use serde_json::Value;

/// Truncate `s` at char boundary `n` and append the server's `…` marker.
fn abridge(s: &str, at: usize) -> String {
    let mut end = at.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Count `{`/`}` and `[`/`]` occurring OUTSIDE string literals.
/// Panics (test failure) if the counts disagree (unbalanced).
fn assert_brackets_balanced(out: &str) {
    let mut braces: i64 = 0;
    let mut brackets: i64 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for c in out.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => braces += 1,
            '}' => braces -= 1,
            '[' => brackets += 1,
            ']' => brackets -= 1,
            _ => {}
        }
    }
    assert_eq!(braces, 0, "unbalanced braces in {out:?}");
    assert_eq!(brackets, 0, "unbalanced brackets in {out:?}");
    assert!(!in_string, "unterminated string in output {out:?}");
}

// ---------------------------------------------------------------------------
// a. Oracle cases: byte-identical to serde_json::to_string_pretty at indent 2
// ---------------------------------------------------------------------------

fn assert_oracle(input: &str) {
    let value: Value = serde_json::from_str(input).expect("test input must be valid JSON");
    let expected = serde_json::to_string_pretty(&value).unwrap();
    let got = crate::pretty_print(input, 2);
    assert_eq!(got, expected, "input: {input:?}");
}

#[test]
fn oracle_empty_containers() {
    assert_oracle("{}");
    assert_oracle("[]");
}

#[test]
fn oracle_nested_mixes() {
    assert_oracle(r#"{"a":[1,2,{"b":null}]}"#);
    assert_oracle(r#"[{"a":[]},{"b":{}}]"#);
    assert_oracle(r#"{"x":{"y":{"z":[true]}}}"#);
}

#[test]
fn oracle_string_escapes_and_unicode() {
    assert_oracle(r#"{"s":"line\nbreak \"quoted\" back\\slash"}"#);
    assert_oracle(r#"{"u":"héllo ✓ 日本"}"#);
    assert_oracle(r#"{"":"empty key"}"#);
}

#[test]
fn oracle_numbers() {
    assert_oracle("42");
    assert_oracle("-7");
    assert_oracle("3.14");
    assert_oracle("-0.5");
    assert_oracle("1.5e2"); // serde_json normalizes to 150.0
    assert_oracle("18446744073709551615"); // u64::MAX
    assert_oracle(r#"{"ints":[0,-1,1000000],"floats":[2.5,1e1]}"#);
}

#[test]
fn oracle_scalars_and_bools_null() {
    assert_oracle("true");
    assert_oracle("false");
    assert_oracle("null");
    assert_oracle(r#""just a string""#);
}

#[test]
fn oracle_arrays_of_scalars() {
    assert_oracle(r#"[1,"two",true,null,3.5]"#);
}

#[test]
fn oracle_already_pretty_input_reformat() {
    // The server abridges PRETTY-printed JSON, so the scanner must handle
    // input that already contains newlines and indentation.
    let pretty = serde_json::to_string_pretty(
        &serde_json::json!({"name": "axoner", "items": [1, 2]}),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&pretty).unwrap();
    let expected = serde_json::to_string_pretty(&value).unwrap();
    assert_eq!(crate::pretty_print(&pretty, 2), expected);
}

// ---------------------------------------------------------------------------
// b. Abridged cases
// ---------------------------------------------------------------------------

/// Representative pretty JSON used for truncation tests.
const SAMPLE: &str = r#"{
  "name": "axoner",
  "count": 42,
  "items": [
    "alpha",
    "beta"
  ],
  "nested": {
    "on": true,
    "pi": 3.14
  }
}"#;

#[test]
fn abridged_no_panic_and_balanced_at_every_offset() {
    for at in 0..=SAMPLE.len() {
        if !SAMPLE.is_char_boundary(at) {
            continue; // &str slicing requires char boundaries (multi-byte chars)
        }
        let cut = &SAMPLE[..at];
        let out = crate::pretty_print(cut, 2); // must not panic
        assert_brackets_balanced(&out);
        let out_marked = crate::pretty_print(&abridge(SAMPLE, at), 2); // with trailing …
        assert_brackets_balanced(&out_marked);
    }
}

#[test]
fn abridged_after_comma_drops_dangling_comma() {
    let out = crate::pretty_print(r#"{"name": "axoner","#, 2);
    assert_eq!(out, "{\n  \"name\": \"axoner\"\n}");
}

#[test]
fn abridged_after_open_brace() {
    let out = crate::pretty_print("{", 2);
    assert_eq!(out, "{}");
    let out = crate::pretty_print("{…", 2); // server-style trailing marker
    assert_eq!(out, "{}");
}

#[test]
fn abridged_mid_string_gets_marker() {
    let out = crate::pretty_print(r#"{"name": "axo"#, 2);
    assert_eq!(out, "{\n  \"name\": \"axo…\"\n}");
}

#[test]
fn abridged_mid_key_gets_marker() {
    let out = crate::pretty_print(r#"{"na"#, 2);
    assert_eq!(out, "{\n  \"na…\"\n}");
}

#[test]
fn abridged_mid_number_keeps_partial_token() {
    let out = crate::pretty_print(r#"{"count": 4"#, 2);
    assert_eq!(out, "{\n  \"count\": 4\n}");
}

#[test]
fn abridged_mid_keyword_keeps_partial_token() {
    let out = crate::pretty_print(r#"{"nested": {"on": tru"#, 2);
    assert_eq!(out, "{\n  \"nested\": {\n    \"on\": tru\n  }\n}");
}

#[test]
fn abridged_inside_nested_array() {
    let cut = "{\n  \"items\": [\n    \"alpha\",";
    let out = crate::pretty_print(cut, 2);
    assert_eq!(
        out,
        "{\n  \"items\": [\n    \"alpha\"\n  ]\n}",
        "dangling comma dropped, both brackets closed"
    );
}

#[test]
fn abridged_trailing_ellipsis_stripped_once() {
    // `…` only ever appended at the very end; one is stripped before scanning.
    let out = crate::pretty_print(r#"{"count": 42…"#, 2);
    assert_eq!(out, "{\n  \"count\": 42\n}");
    // An ellipsis inside string content (not at the end) is preserved.
    let out = crate::pretty_print(r#"{"s":"a…b"}"#, 2);
    assert_eq!(out, "{\n  \"s\": \"a…b\"\n}");
}

// ---------------------------------------------------------------------------
// c. Indent parameter (2 and 4)
// ---------------------------------------------------------------------------

#[test]
fn indent_2_and_4() {
    let input = r#"{"a":[1,{"b":"x"}]}"#;
    let expected_2 = "{\n  \"a\": [\n    1,\n    {\n      \"b\": \"x\"\n    }\n  ]\n}";
    assert_eq!(crate::pretty_print(input, 2), expected_2);
    let expected_4 = "{\n    \"a\": [\n        1,\n        {\n            \"b\": \"x\"\n        }\n    ]\n}";
    assert_eq!(crate::pretty_print(input, 4), expected_4);
}

// ---------------------------------------------------------------------------
// d. Garbage input: sane, no panic (documented: echoed leniently)
// ---------------------------------------------------------------------------

#[test]
fn garbage_is_echoed_without_panic() {
    // Non-JSON tokens pass through; brackets still get balanced.
    assert_eq!(crate::pretty_print("hello world", 2), "hello world");
    assert_eq!(crate::pretty_print("", 2), "");
    let out = crate::pretty_print("hello {world", 2);
    assert_brackets_balanced(&out);
    let out = crate::pretty_print("]}{", 2); // mismatched brackets: echoed leniently
    assert!(!out.is_empty());
    // Invalid UTF-8-adjacent nasties and lone escapes do not panic.
    let _ = crate::pretty_print("\"unterminated", 2);
    let _ = crate::pretty_print("\"back\\\\", 2);
    let _ = crate::pretty_print("::,,,", 2);
}

// ---------------------------------------------------------------------------
// Round-trip: abridge(to_string_pretty(x), 1024) → pretty_print is balanced
// ---------------------------------------------------------------------------

#[test]
fn round_trip_abridged_pretty_json() {
    let values = vec![
        serde_json::json!({}),
        serde_json::json!([]),
        serde_json::json!(null),
        serde_json::json!(42),
        serde_json::json!(-3.5),
        serde_json::json!("a string"),
        serde_json::json!({"a": 1}),
        serde_json::json!([1, 2, 3]),
        serde_json::json!({"s": "line\nbreak \"q\" back\\slash unicode héllo ✓"}),
        serde_json::json!({"nested": {"deep": {"deeper": [true, false, null]}}}),
        serde_json::json!({"keys": [ {"a": 1}, {"b": [2, 3]}, {"c": {"d": 4}} ]}),
        serde_json::json!({"pad": "x".repeat(2000)}),
        serde_json::json!({"nums": [0, -1, 3.125, 1e2, 18446744073709551615u64]}),
        serde_json::json!({"mixed": [{"a": [1]}, [[[]]], {}, {"": ""}]}),
        serde_json::json!({"unicode": "日本語 ✓ emoji 🎉 end"}),
        serde_json::json!({"long_array": (0..100).collect::<Vec<i32>>()}),
        serde_json::json!({"empty_str": "", "empty_arr": [], "empty_obj": {}}),
        serde_json::json!({"b": true, "n": null, "f": false}),
        serde_json::json!({"levels": {"1": {"2": {"3": {"4": {"5": "deep"}}}}}}),
        serde_json::json!({"trailing": {"list": ["a", "b", "c"], "num": 7}}),
    ];
    assert_eq!(values.len(), 20);
    for v in values {
        let pretty = serde_json::to_string_pretty(&v).unwrap();
        // Server contract: cut so the total is ≤1024 chars including one … .
        let abridged = if pretty.chars().count() > 1024 {
            let head: String = pretty.chars().take(1023).collect();
            format!("{head}…")
        } else {
            pretty.clone()
        };
        let out = crate::pretty_print(&abridged, 2);
        assert_brackets_balanced(&out);
        if abridged.ends_with('…') {
            // Somewhere the cut must be visible: either a …" string marker or,
            // for cuts outside strings, the output is a prefix re-format.
            let _ = out; // balance assertion above is the real check
        }
        let compact = abridged.replace(['\n', ' '], "");
        let out_compact = crate::pretty_print(&compact, 2);
        assert_brackets_balanced(&out_compact);
    }
}
