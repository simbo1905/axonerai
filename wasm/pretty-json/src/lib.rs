//! Lenient pretty-printer for well-formed or abridged (truncated) JSON.
//!
//! Compiled to WASM for the browser (see `make wasm-pretty`). Well-formed JSON
//! is formatted byte-identically to `serde_json::to_string_pretty` (indent 2).
//! Abridged JSON — text truncated mid-token with a single trailing `…` — is
//! scanned leniently: dangling commas are dropped, an unterminated string is
//! closed with a visible `…` marker, and all open brackets are closed in
//! reverse order. Never panics on any input.

use wasm_bindgen::prelude::*;

/// Pretty-print `input`, which may be well-formed JSON or JSON truncated
/// mid-token (optionally ending in a single `…`).
///
/// For well-formed input with `indent == 2` the output is byte-identical to
/// `serde_json::to_string_pretty`. For abridged input the open brackets are
/// closed in reverse order; an unterminated string is closed as `…"` so the
/// reader sees where the cut happened. Garbage input is echoed leniently
/// (non-JSON scalar tokens pass through; brackets are balanced). Never panics.
#[wasm_bindgen]
pub fn pretty_print(input: &str, indent: usize) -> String {
    // Guard against absurd indent widths (no panic, bounded memory).
    let width = if indent == 0 { 2 } else { indent.min(16) };
    let pad = " ".repeat(width);
    // The server appends exactly one `…` at the very end of an abridged
    // payload; strip it before scanning. An ellipsis inside string content
    // (not the last char) is preserved.
    let s = input.strip_suffix('…').unwrap_or(input);
    scan(s, &pad)
}

struct Frame {
    close: char,
    has_content: bool,
}

/// Write the newline + indentation owed after an open bracket or comma, right
/// before the next piece of content. Deferred so empty containers (`{}`, `[]`)
/// render compactly like serde_json does.
fn flush_pending(out: &mut String, depth: usize, pad: &str, pending: &mut bool) {
    if *pending {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(pad);
        }
        *pending = false;
    }
}

fn scan(s: &str, pad: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2 + 16);
    let mut stack: Vec<Frame> = Vec::new();
    let mut chars = s.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    let mut pending = false; // newline+indent owed before next content
    let mut prev_primitive = false; // last emission was a primitive token

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                if let Some(f) = stack.last_mut() {
                    f.has_content = true;
                }
            }
            continue;
        }
        match c {
            '"' => {
                flush_pending(&mut out, stack.len(), pad, &mut pending);
                in_string = true;
                out.push('"');
                prev_primitive = false;
            }
            '{' | '[' => {
                // The pending newline (from a comma or parent open) belongs to
                // this bracket itself, at the pre-push depth.
                flush_pending(&mut out, stack.len(), pad, &mut pending);
                out.push(c);
                stack.push(Frame {
                    close: if c == '{' { '}' } else { ']' },
                    has_content: false,
                });
                pending = true;
                prev_primitive = false;
            }
            '}' | ']' => {
                // Lenient: only treat as a close when it matches the innermost
                // open bracket; otherwise echo it (garbage tolerance).
                if stack.last().is_some_and(|f| f.close == c) {
                    let f = stack.pop().expect("checked non-empty above");
                    if f.has_content {
                        out.push('\n');
                        for _ in 0..stack.len() {
                            out.push_str(pad);
                        }
                    }
                    out.push(f.close);
                    if let Some(p) = stack.last_mut() {
                        p.has_content = true;
                    }
                } else {
                    flush_pending(&mut out, stack.len(), pad, &mut pending);
                    out.push(c);
                }
                pending = false;
                prev_primitive = false;
            }
            ',' => {
                out.push(',');
                if !stack.is_empty() {
                    pending = true;
                }
                prev_primitive = false;
            }
            ':' => {
                out.push_str(": ");
                prev_primitive = false;
            }
            c if c.is_whitespace() => {
                // Inside valid JSON, whitespace between primitive tokens never
                // occurs (a comma intervenes); preserve the run for garbage
                // echo only when a primitive token actually follows it.
                if prev_primitive {
                    let mut run = String::new();
                    run.push(c);
                    let mut next = None;
                    while let Some(&n) = chars.peek() {
                        if n.is_whitespace() {
                            run.push(n);
                            chars.next();
                        } else {
                            next = Some(n);
                            break;
                        }
                    }
                    if next
                        .is_some_and(|n| !matches!(n, ',' | ':' | '{' | '}' | '[' | ']' | '"'))
                    {
                        out.push_str(&run);
                    }
                }
            }
            c => {
                // Primitive token: number, true/false/null, or garbage run.
                flush_pending(&mut out, stack.len(), pad, &mut pending);
                let mut tok = String::new();
                tok.push(c);
                while let Some(&nc) = chars.peek() {
                    if matches!(nc, ',' | ':' | '{' | '}' | '[' | ']' | '"')
                        || nc.is_whitespace()
                    {
                        break;
                    }
                    tok.push(nc);
                    chars.next();
                }
                out.push_str(&render_token(&tok));
                if let Some(f) = stack.last_mut() {
                    f.has_content = true;
                }
                prev_primitive = true;
            }
        }
    }

    // EOF inside a string: show the cut with a marker, then close the string
    // so brackets stay balanced outside string literals. The frame does hold
    // content, so it closes on its own line.
    if in_string {
        out.push('…');
        out.push('"');
        if let Some(f) = stack.last_mut() {
            f.has_content = true;
        }
    }

    // Close all remaining open brackets in reverse order, dropping dangling
    // trailing commas at each close.
    while let Some(f) = stack.pop() {
        strip_dangling_commas(&mut out);
        if f.has_content {
            out.push('\n');
            for _ in 0..stack.len() {
                out.push_str(pad);
            }
        }
        out.push(f.close);
    }

    out
}

fn strip_dangling_commas(out: &mut String) {
    loop {
        while out.ends_with(|c: char| c.is_whitespace()) {
            out.pop();
        }
        if out.ends_with(',') {
            out.pop();
        } else {
            break;
        }
    }
}

/// Render a scanned primitive token. Tokens that form a valid JSON number are
/// normalized the way serde_json re-encodes them (integer vs f64 shortest
/// round-trip) so well-formed output matches `serde_json::to_string_pretty`
/// byte-for-byte. Anything else (partial number like `42.`, partial keyword
/// like `tru`, garbage) is echoed as-is.
fn render_token(tok: &str) -> String {
    if let Ok(i) = tok.parse::<i64>() {
        return i.to_string();
    }
    if tok.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(u) = tok.parse::<u64>() {
            return u.to_string();
        }
    }
    if is_json_number(tok) {
        if let Ok(f) = tok.parse::<f64>() {
            return format_f64(f);
        }
    }
    tok.to_string()
}

/// Strict JSON number grammar: `-? (0 | [1-9][0-9]*)? (. [0-9]+)? ([eE][+-]?[0-9]+)?`
/// with at least one digit somewhere.
fn is_json_number(tok: &str) -> bool {
    let b = tok.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    let int_start = i;
    if i < b.len() && b[i] == b'0' {
        i += 1;
    } else {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == int_start {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

/// Format an f64 the way serde_json (ryu) does for the value ranges this
/// printer handles: positional notation with a `.0` suffix for integral
/// values, shortest round-trip otherwise.
fn format_f64(f: f64) -> String {
    if f.is_finite() && f == f.trunc() && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

#[cfg(test)]
mod tests;
