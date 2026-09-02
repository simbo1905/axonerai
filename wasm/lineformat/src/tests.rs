//! Adversarial test matrix for the shared wire-line format.

use super::{LineError, WireFrame, extract_type, split_wire_line, truncate_payload, ts_prefix};
use std::borrow::Cow;

fn ts(line: &str) -> Result<u64, LineError> {
    ts_prefix(line)
}

const VALID: &str = "1717238400000\0{\"x\":1}";

// ---------------------------------------------------------------------------
// ts_prefix — the strict `<digits>\0` contract
// ---------------------------------------------------------------------------

#[test]
fn ts_prefix_empty_line_is_rejected() {
    assert_eq!(ts(""), Err(LineError::Empty));
}

#[test]
fn ts_prefix_letters_before_nul_is_rejected() {
    assert_eq!(ts("abc\0{}"), Err(LineError::NotDigit));
}

#[test]
fn ts_prefix_space_before_digits_is_rejected() {
    assert_eq!(ts(" 12\0{}"), Err(LineError::NotDigit));
}

#[test]
fn ts_prefix_sign_is_rejected() {
    assert_eq!(ts("+1\0{}"), Err(LineError::NotDigit));
    assert_eq!(ts("-1\0{}"), Err(LineError::NotDigit));
}

#[test]
fn ts_prefix_space_after_digits_is_rejected() {
    // digits must be IMMEDIATELY followed by \0 — `12 {}` has none
    assert_eq!(ts("12 {}"), Err(LineError::MissingNul));
}

#[test]
fn ts_prefix_digits_only_without_nul_is_rejected() {
    assert_eq!(ts("12"), Err(LineError::MissingNul));
    assert_eq!(ts("1717238400000"), Err(LineError::MissingNul));
}

#[test]
fn ts_prefix_digit_only_17_chars_is_rejected() {
    assert_eq!(ts("12345678901234567\0{}"), Err(LineError::PrefixTooLong));
}

#[test]
fn ts_prefix_16_digits_is_accepted() {
    assert_eq!(ts("1234567890123456\0{}"), Ok(1_234_567_890_123_456));
}

#[test]
fn ts_prefix_1_digit_is_accepted() {
    assert_eq!(ts("7\0{}"), Ok(7));
}

#[test]
fn ts_prefix_valid_epoch() {
    assert_eq!(ts(VALID), Ok(1_717_238_400_000));
}

#[test]
fn ts_prefix_digits_then_nul_with_no_payload_is_accepted() {
    // A crash mid-write can leave `12\0` as the final line: the PREFIX is
    // valid; the (empty) payload is handled leniently downstream.
    assert_eq!(ts("12\0"), Ok(12));
}

#[test]
fn ts_prefix_newline_first_is_rejected() {
    assert_eq!(ts("\n12\0{}"), Err(LineError::NotDigit));
}

// ---------------------------------------------------------------------------
// extract_type — literal `"_type":"` scan, no JSON parse
// ---------------------------------------------------------------------------

#[test]
fn extract_type_reads_value() {
    let json = r#"{"_type":"assistant","text":"hi"}"#;
    assert_eq!(extract_type(json), "assistant");
}

#[test]
fn extract_type_missing_returns_empty() {
    assert_eq!(extract_type(r#"{"x":1}"#), "");
    assert_eq!(extract_type(""), "");
}

#[test]
fn extract_type_stops_at_first_unescaped_quote() {
    let json = "{\"_type\":\"say \\\"hi\\\" now\",\"x\":1}";
    assert_eq!(extract_type(json), "say \\\"hi\\\" now");
}

#[test]
fn extract_type_ignores_lookalike_keys() {
    // A key named `_type2` must not match — the scan needs `"_type":"`.
    let json = r#"{"_type2":"decoy","_type":"real"}"#;
    assert_eq!(extract_type(json), "real");
}

// ---------------------------------------------------------------------------
// truncate_payload — char-boundary-safe cut
// ---------------------------------------------------------------------------

#[test]
fn truncate_payload_short_is_borrowed_untouched() {
    let (text, truncated) = truncate_payload("{\"x\":1}", 1024);
    assert_eq!(text, Cow::Borrowed("{\"x\":1}"));
    assert!(!truncated);
}

#[test]
fn truncate_payload_exact_length_is_not_truncated() {
    let payload = "abcdefgh";
    let (text, truncated) = truncate_payload(payload, 8);
    assert_eq!(text, payload);
    assert!(!truncated);
}

#[test]
fn truncate_payload_cuts_ascii() {
    let (text, truncated) = truncate_payload("abcdefgh", 4);
    assert_eq!(text, "abcd");
    assert!(truncated);
}

#[test]
fn truncate_payload_never_splits_emoji_codepoint() {
    // '😀' is 4 bytes; payload = 'a' + '😀' + 'b' = 6 bytes. Cutting at 3
    // would land mid-codepoint — the cut must fall back to a boundary (1).
    let payload = "a\u{1F600}b";
    assert_eq!(payload.len(), 6);
    let (text, truncated) = truncate_payload(payload, 3);
    assert!(truncated);
    assert_eq!(text, "a");
    // The result is valid UTF-8 by construction (a &str slice).
}

#[test]
fn truncate_payload_cut_at_exact_boundary_keeps_full_codepoint() {
    let payload = "a\u{1F600}b"; // boundaries at 0, 1, 5, 6
    let (text, truncated) = truncate_payload(payload, 5);
    assert!(truncated);
    assert_eq!(text, "a\u{1F600}");
}

#[test]
fn truncate_payload_zero_max_gives_empty() {
    let (text, truncated) = truncate_payload("{\"x\":1}", 0);
    assert_eq!(text, "");
    assert!(truncated);
}

// ---------------------------------------------------------------------------
// split_wire_line — the ONE function server and browser semantic-share
// ---------------------------------------------------------------------------

#[test]
fn split_valid_line() {
    let frame = split_wire_line(VALID, 1024).unwrap();
    assert_eq!(
        frame,
        WireFrame {
            ts: 1_717_238_400_000,
            r#type: String::new(),
            text: "{\"x\":1}".to_string(),
            truncated: false,
        }
    );
}

#[test]
fn split_extracts_type() {
    let line = "1717238400001\0{\"_type\":\"assistant\",\"text\":\"hi\"}";
    let frame = split_wire_line(line, 1024).unwrap();
    assert_eq!(frame.ts, 1_717_238_400_001);
    assert_eq!(frame.r#type, "assistant");
    assert_eq!(frame.text, line.split_once('\0').unwrap().1);
    assert!(!frame.truncated);
}

#[test]
fn split_truncates_oversized_payload_and_flags() {
    let line = "1717238400002\0{\"_type\":\"tool_trace\",\"result_json\":\"XXXX\"}";
    let frame = split_wire_line(line, 10).unwrap();
    assert!(frame.truncated);
    assert_eq!(frame.text.len(), 10);
    assert!(line.starts_with("1717238400002\0"));
}

#[test]
fn split_rejects_every_bad_prefix_shape() {
    for bad in ["", "abc\0{}", " 12\0{}", "+1\0{}", "12 {}", "12", "12345678901234567\0{}"] {
        assert!(split_wire_line(bad, 1024).is_err(), "must reject {bad:?}");
    }
}

#[test]
fn split_empty_payload_is_accepted_with_empty_text() {
    let frame = split_wire_line("12\0", 1024).unwrap();
    assert_eq!(frame.ts, 12);
    assert_eq!(frame.r#type, "");
    assert_eq!(frame.text, "");
    assert!(!frame.truncated);
}

#[test]
fn split_truncation_does_not_split_codepoint() {
    let line = "1717238400003\0{\"text\":\"a\u{1F600}b\"}";
    // Payload is 17 bytes; the emoji spans bytes 10..14, so a cut at 12 must
    // fall back to the boundary at 10.
    let frame = split_wire_line(line, 12).unwrap();
    assert!(frame.truncated);
    assert_eq!(frame.text.len(), 10);
    // Every emitted frame must be a valid &str slice of the payload prefix.
    let payload = line.split_once('\0').unwrap().1;
    assert!(payload.starts_with(frame.text.as_str()));
}

// ---------------------------------------------------------------------------
// extract_tool_call_meta — lenient scan over a (truncated) tool_call payload
// ---------------------------------------------------------------------------

use super::extract_tool_call_meta;

/// A realistic `tool_call` serialization in the exact byte order the server
/// writes (metadata first, payload LAST — see `src/wire.rs`
/// `tool_call_serializes_metadata_before_payload`). Built by concatenating
/// known parts so expected field positions are known by construction.
struct Fixture {
    full: String,
    args_val: String,
    result_val: String,
    /// Byte offset of the first digit of the `ts` value (the LAST mandatory
    /// metadata field).
    ts_digits_start: usize,
    /// Byte offset just past the last `ts` digit (metadata fully visible).
    ts_digits_end: usize,
    args_val_start: usize,
    args_val_end: usize,
    result_val_start: usize,
    result_val_end: usize,
}

fn fixture() -> Fixture {
    // Payload contents full of escaped quotes — adversarial for the string
    // scanner. Pure ASCII so byte offsets == char offsets.
    let args_val =
        "{\"query\":\"axonerai rollout wasm\",\"max_results\":5,\"deep\":{\"n\":1}}"
            .replace('"', "\\\"");
    let result_val =
        "{\"results\":[{\"title\":\"page one\",\"url\":\"https://example.test/a\"},{\"title\":\"page two\"}],\"took_ms\":42}"
            .replace('"', "\\\"");
    let head = format!(
        "{{\"_type\":\"tool_call\",\"id\":null,\"session_id\":\"01890a5d-ac96-774b-bcce-b302099a8057\",\"tool\":\"WebSearch\",\"bytes_up\":120,\"bytes_down\":4567,\"duration_ms\":8123,\"ts\":{}",
        1717238400123u64
    );
    let args_open = "\"args_pretty\":\"";
    let args_close = "\",";
    let result_open = "\"result_pretty\":\"";
    let result_close = "\"}";
    let args_val_start = head.len() + args_open.len();
    let args_val_end = args_val_start + args_val.len();
    let result_val_start = args_val_end + args_close.len() + result_open.len();
    let result_val_end = result_val_start + result_val.len();
    let full = format!(
        "{head}{args_open}{args_val}{args_close}{result_open}{result_val}{result_close}"
    );
    let ts_digits_start = full.find("1717238400123").expect("ts digits in fixture");
    Fixture {
        ts_digits_end: ts_digits_start + "1717238400123".len(),
        ts_digits_start,
        args_val_start,
        args_val_end,
        result_val_start,
        result_val_end,
        args_val,
        result_val,
        full,
    }
}

/// Independently derived expected head for a payload string whose raw
/// contents start at `start` and end (last content char) at `end - 1`.
fn expected_head(val: &str, start: usize, end: usize, cut: usize) -> String {
    if cut <= start {
        String::new()
    } else {
        val[..(cut - start).min(val.len())].to_string()
    }
}

/// A head must never end with an UNESCAPED quote: the only way a `"` can be
/// the last visible char of a cut value is as an escaped content quote (`\"`).
/// An unescaped one would be the field's closing quote, which the scanner
/// must consume — never emit.
fn assert_no_tail_quote(head: &str, cut: usize) {
    if head.ends_with('"') {
        assert!(
            head.ends_with("\\\""),
            "head at cut {cut} ends with an unterminated tail quote: {head:?}"
        );
    }
}

#[test]
fn extract_tool_call_meta_reads_full_payload() {
    let f = fixture();
    let meta = extract_tool_call_meta(&f.full).expect("full payload must parse");
    assert_eq!(meta.tool, "WebSearch");
    assert_eq!(meta.duration_ms, 8123);
    assert_eq!(meta.bytes_up, 120);
    assert_eq!(meta.bytes_down, 4567);
    assert_eq!(meta.ts, 1_717_238_400_123);
    assert_eq!(meta.args_pretty_head, f.args_val, "args head is the raw contents");
    assert_eq!(
        meta.result_pretty_head, f.result_val,
        "result head is the raw contents"
    );
}

#[test]
fn extract_tool_call_meta_truncated_at_every_64_bytes() {
    let f = fixture();
    let len = f.full.len();
    let mut cut = 64usize;
    while cut < len {
        let text = &f.full[..cut];
        let meta = extract_tool_call_meta(text);
        if cut <= f.ts_digits_start {
            // The last mandatory metadata field (ts) has no digit visible
            // yet — metadata is incomplete, so the scan must refuse.
            assert!(
                meta.is_none(),
                "cut {cut} has incomplete metadata, expected None"
            );
        } else {
            let expected_ts_digits = (cut - f.ts_digits_start).min(13);
            let meta = meta.expect("cut past ts start must extract (leniently)");
            assert_eq!(meta.tool, "WebSearch", "tool at cut {cut}");
            assert_eq!(meta.bytes_up, 120, "bytes_up at cut {cut}");
            assert_eq!(meta.bytes_down, 4567, "bytes_down at cut {cut}");
            assert_eq!(meta.duration_ms, 8123, "duration_ms at cut {cut}");
            if expected_ts_digits == 13 {
                assert_eq!(meta.ts, 1_717_238_400_123, "ts at cut {cut}");
            } else {
                let expected_ts: u64 = f.full[f.ts_digits_start..f.ts_digits_start + expected_ts_digits]
                    .parse()
                    .expect("digit prefix parses");
                assert_eq!(meta.ts, expected_ts, "ts digit prefix at cut {cut}");
            }
            assert!(
                f.args_val.starts_with(&meta.args_pretty_head),
                "args head must be a prefix of the raw contents (cut {cut})"
            );
            assert!(
                f.result_val.starts_with(&meta.result_pretty_head),
                "result head must be a prefix of the raw contents (cut {cut})"
            );
            assert_eq!(
                meta.args_pretty_head,
                expected_head(&f.args_val, f.args_val_start, f.args_val_end, cut),
                "args head at cut {cut}"
            );
            assert_eq!(
                meta.result_pretty_head,
                expected_head(&f.result_val, f.result_val_start, f.result_val_end, cut),
                "result head at cut {cut}"
            );
            assert_no_tail_quote(&meta.args_pretty_head, cut);
            assert_no_tail_quote(&meta.result_pretty_head, cut);
        }
        cut += 64;
    }
}

#[test]
fn extract_tool_call_meta_dense_sweep_over_metadata_region() {
    // Step 1 over every byte of the metadata region plus the start of the
    // payload: Some/None transitions and lenient digit prefixes are exact.
    let f = fixture();
    let sweep_end = f.args_val_start + 8;
    for cut in 0..=sweep_end {
        let text = &f.full[..cut];
        let meta = extract_tool_call_meta(text);
        if cut <= f.ts_digits_start {
            assert!(meta.is_none(), "cut {cut} incomplete metadata, expected None");
            continue;
        }
        let expected_ts_digits = (cut - f.ts_digits_start).min(13);
        let meta = meta.unwrap_or_else(|| panic!("cut {cut} must extract"));
        assert_eq!(meta.tool, "WebSearch", "tool at cut {cut}");
        assert_eq!(meta.duration_ms, 8123, "duration_ms at cut {cut}");
        if expected_ts_digits == 13 {
            assert_eq!(meta.ts, 1_717_238_400_123, "ts at cut {cut}");
        } else {
            let expected_ts: u64 = f.full[f.ts_digits_start..f.ts_digits_start + expected_ts_digits]
                .parse()
                .unwrap();
            assert_eq!(meta.ts, expected_ts, "ts digit prefix at cut {cut}");
        }
        assert_eq!(
            meta.args_pretty_head,
            expected_head(&f.args_val, f.args_val_start, f.args_val_end, cut),
            "args head at cut {cut}"
        );
        assert_no_tail_quote(&meta.args_pretty_head, cut);
    }
}

#[test]
fn extract_tool_call_meta_rejects_missing_metadata() {
    // Not a tool_call at all.
    assert!(extract_tool_call_meta(r#"{"_type":"assistant","text":"hi"}"#).is_none());
    assert!(extract_tool_call_meta("").is_none());
    // tool_call tag but missing each mandatory metadata field in turn.
    assert!(extract_tool_call_meta(r#"{"_type":"tool_call"}"#).is_none());
    assert!(
        extract_tool_call_meta(
            r#"{"_type":"tool_call","duration_ms":1,"bytes_up":2,"bytes_down":3,"ts":4}"#
        )
        .is_none(),
        "missing tool must be None"
    );
    assert!(
        extract_tool_call_meta(
            r#"{"_type":"tool_call","tool":"t","bytes_up":2,"bytes_down":3,"ts":4}"#
        )
        .is_none(),
        "missing duration_ms must be None"
    );
    assert!(
        extract_tool_call_meta(
            r#"{"_type":"tool_call","tool":"t","duration_ms":1,"bytes_up":2,"bytes_down":3}"#
        )
        .is_none(),
        "missing ts must be None"
    );
}

#[test]
fn extract_tool_call_meta_lenient_on_type_tag_cut() {
    // The `_type` value itself cut mid-token: the tag scan cannot confirm
    // "tool_call", so extraction refuses.
    let f = fixture();
    let meta = extract_tool_call_meta(&f.full[..20]);
    assert!(meta.is_none(), "cut _type tag must refuse");
}

#[test]
fn extract_tool_call_meta_full_input_without_payloads_still_extracts() {
    // Metadata-only tool_call (payloads empty): metadata complete, heads empty.
    let json = r#"{"_type":"tool_call","id":null,"session_id":"s","tool":"Calc","bytes_up":0,"bytes_down":0,"duration_ms":5,"ts":7,"args_pretty":"","result_pretty":""}"#;
    let meta = extract_tool_call_meta(json).expect("metadata-only payload parses");
    assert_eq!(meta.tool, "Calc");
    assert_eq!(meta.duration_ms, 5);
    assert_eq!(meta.ts, 7);
    assert_eq!(meta.args_pretty_head, "");
    assert_eq!(meta.result_pretty_head, "");
}
