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
}

#[cfg(test)]
mod tests;
