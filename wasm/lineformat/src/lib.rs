//! Shared parsing of rollout wire lines: `<ascii unix epoch ms>\0<json>\n`.
//!
//! One implementation, two runtimes: this crate compiles as an `rlib` for the
//! server and as a `cdylib`+wasm for the browser (see `make wasm-lineformat`).
//! The server never JSON-parses when streaming history; the browser uses the
//! same split as its high-watermark seam. Truncation of oversized payloads is
//! char-boundary safe so a `\0` or multi-byte character is never split.

use std::borrow::Cow;

/// Why a line failed the strict wire-line format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineError {
    /// Empty line — no prefix at all.
    Empty,
    /// The line does not start with an ASCII digit (spaces, sign, letters…).
    NotDigit,
    /// More than 16 digits before the separator.
    PrefixTooLong,
    /// Digits are present but not immediately followed by `\0`.
    MissingNul,
}

impl core::fmt::Display for LineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            LineError::Empty => "empty line",
            LineError::NotDigit => "line does not start with a digit",
            LineError::PrefixTooLong => "timestamp prefix longer than 16 digits",
            LineError::MissingNul => "timestamp prefix not immediately followed by NUL",
        };
        f.write_str(msg)
    }
}

/// A rollout wire line split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireFrame {
    pub ts: u64,
    pub r#type: String,
    pub text: String,
    pub truncated: bool,
}

/// Validate the `<ascii unix epoch ms>\0` prefix of a wire line and return the
/// timestamp. The prefix must be 1..=16 ASCII digits immediately followed by
/// `\0`; anything else (empty, spaces, sign, letters, missing separator) is a
/// `LineError`.
pub fn ts_prefix(line: &str) -> Result<u64, LineError> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return Err(LineError::Empty);
    }
    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits == 0 {
        return Err(LineError::NotDigit);
    }
    if digits > 16 {
        return Err(LineError::PrefixTooLong);
    }
    if digits == bytes.len() || bytes[digits] != b'\0' {
        return Err(LineError::MissingNul);
    }
    line[..digits]
        .parse::<u64>()
        .map_err(|_| LineError::PrefixTooLong)
}

/// Cheaply extract the `_type` string value from a JSON payload by scanning
/// for the literal `"_type":"` prefix — no JSON parse. The scan stops at the
/// first unescaped closing quote of the value. Returns `""` when absent.
pub fn extract_type(json: &str) -> &str {
    const TAG: &str = "\"_type\":\"";
    let Some(start) = json.find(TAG) else {
        return "";
    };
    let rest = &json[start + TAG.len()..];
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return &rest[..i];
        }
    }
    // Unterminated value (crash-truncated line): return the remainder leniently.
    rest
}

/// Lenient metadata extracted from a (possibly truncated) `tool_call` event
/// JSON payload. `*_pretty_head` fields carry the raw (possibly cut) string
/// contents of the payload fields; the payload fields serialize LAST so the
/// metadata is complete whenever the payload was egress-truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallMeta {
    pub tool: String,
    pub duration_ms: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub ts: u64,
    pub args_pretty_head: String,
    pub result_pretty_head: String,
}

/// Scan for the raw contents of a JSON string field `"key":"..."` — no JSON
/// parse. The value runs up to the first unescaped closing quote, or to
/// end-of-text when the payload was truncated mid-value (best effort). The
/// closing quote itself is never included. Returns `None` when the
/// `"key":"` literal is not present.
fn scan_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let mut out = String::new();
    let mut escaped = false;
    for c in rest.chars() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            out.push(c);
            escaped = true;
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Scan for a `u64` JSON field `"key":<digits>` — no JSON parse. Returns
/// `None` when the `"key":` literal is absent or no ASCII digits follow it
/// (a truncated digit run yields the digits visible so far).
fn scan_u64_field(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let digits = rest
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    rest[..digits].parse::<u64>().ok()
}

/// Leniently extract the metadata of a `tool_call` event from a (possibly
/// truncated) JSON payload — no JSON parse, a literal scan like
/// [`extract_type`]. The metadata fields (`tool`, `duration_ms`, `bytes_up`,
/// `bytes_down`, `ts`) serialize BEFORE the payload strings, so they are
/// complete on an egress-truncated line; `*_pretty_head` carry the raw
/// (possibly cut) payload string contents up to the cut. Missing metadata
/// fields (or a cut `"_type":"tool_call"` tag) → `None`.
pub fn extract_tool_call_meta(text: &str) -> Option<ToolCallMeta> {
    if extract_type(text) != "tool_call" {
        return None;
    }
    let tool = scan_string_field(text, "tool")?;
    let duration_ms = scan_u64_field(text, "duration_ms")?;
    let bytes_up = scan_u64_field(text, "bytes_up")?;
    let bytes_down = scan_u64_field(text, "bytes_down")?;
    let ts = scan_u64_field(text, "ts")?;
    let args_pretty_head = scan_string_field(text, "args_pretty").unwrap_or_default();
    let result_pretty_head = scan_string_field(text, "result_pretty").unwrap_or_default();
    Some(ToolCallMeta {
        tool,
        duration_ms,
        bytes_up,
        bytes_down,
        ts,
        args_pretty_head,
        result_pretty_head,
    })
}

/// Char-boundary-safe cut of a payload to at most `max_bytes` bytes. Returns
/// the (possibly cut) text and whether it was truncated. Never splits a
/// multi-byte UTF-8 codepoint.
pub fn truncate_payload(json: &str, max_bytes: usize) -> (Cow<'_, str>, bool) {
    if json.len() <= max_bytes {
        return (Cow::Borrowed(json), false);
    }
    let mut end = max_bytes.min(json.len());
    while end > 0 && !json.is_char_boundary(end) {
        end -= 1;
    }
    (Cow::Owned(json[..end].to_string()), true)
}

/// Split one wire line into a [`WireFrame`]. `max_bytes` bounds the payload
/// text; oversized payloads are truncated (char-boundary safe) and flagged.
pub fn split_wire_line(line: &str, max_bytes: usize) -> Result<WireFrame, LineError> {
    let ts = ts_prefix(line)?;
    let nul = line.find('\0').ok_or(LineError::MissingNul)?;
    let payload = &line[nul + 1..];
    let r#type = extract_type(payload);
    let (text, truncated) = truncate_payload(payload, max_bytes);
    Ok(WireFrame {
        ts,
        r#type: r#type.to_string(),
        text: text.into_owned(),
        truncated,
    })
}

/// WASM exports for the browser (`make wasm-lineformat`): the SAME logic the
/// server runs, shared via this one crate.
#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use super::{split_wire_line, ts_prefix, WireFrame};
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::JsValue;

    /// Parse one wire line into `{ ts, type, text, truncated }`. Throws a
    /// string describing the corruption when the ts prefix is invalid.
    #[wasm_bindgen]
    pub fn parse_line(line: &str, max: usize) -> Result<JsValue, JsValue> {
        let frame: WireFrame = split_wire_line(line, max).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let obj = js_sys::Object::new();
        let set = |key: &str, value: JsValue| -> Result<(), JsValue> {
            js_sys::Reflect::set(&obj, &JsValue::from_str(key), &value).map(|_| ())
        };
        set("ts", JsValue::from_f64(frame.ts as f64))?;
        set("type", JsValue::from_str(&frame.r#type))?;
        set("text", JsValue::from_str(&frame.text))?;
        set("truncated", JsValue::from_bool(frame.truncated))?;
        Ok(obj.into())
    }

    /// Cheap validity check: does the line start with `<digits>\0`?
    #[wasm_bindgen]
    pub fn is_valid(line: &str) -> bool {
        ts_prefix(line).is_ok()
    }

    /// Lenient metadata extraction from a (possibly truncated) `tool_call`
    /// payload: `{ tool, duration_ms, bytes_up, bytes_down, ts,
    /// args_pretty_head, result_pretty_head }`, or `null` when the metadata
    /// fields are missing.
    #[wasm_bindgen]
    pub fn extract_tool_call_meta(text: &str) -> JsValue {
        let meta = match super::extract_tool_call_meta(text) {
            Some(meta) => meta,
            None => return JsValue::NULL,
        };
        let obj = js_sys::Object::new();
        let set = |key: &str, value: JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(key), &value);
        };
        set("tool", JsValue::from_str(&meta.tool));
        set("duration_ms", JsValue::from_f64(meta.duration_ms as f64));
        set("bytes_up", JsValue::from_f64(meta.bytes_up as f64));
        set("bytes_down", JsValue::from_f64(meta.bytes_down as f64));
        set("ts", JsValue::from_f64(meta.ts as f64));
        set("args_pretty_head", JsValue::from_str(&meta.args_pretty_head));
        set("result_pretty_head", JsValue::from_str(&meta.result_pretty_head));
        obj.into()
    }
}

#[cfg(test)]
mod tests;
