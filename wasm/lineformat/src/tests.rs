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
