//! Generated JTD wire-event validators, one implementation, two runtimes.
//!
//! The validator functions under [`generated`] are machine-written by
//! `jtd-codegen --target rust` (mise-pinned `github:simbo1905/jtd-wasm`
//! release-0.3.0) from `schemas/*.jdt.json`; see the `wasm-validators`
//! Makefile target. Do not edit them by hand. The same code is exercised as
//! pure Rust unit tests (`cargo test`, see `tests.rs`, ported from
//! `tests/wire_protocol.rs`) and shipped to the browser as a cdylib+wasm
//! (`make wasm-validators`), replacing the generated `.mjs` validators in
//! `web/src/wire.mjs` behind `web/src/wasm-validators.mjs`.

pub mod generated;

/// WASM exports for the browser (`make wasm-validators`): one `validate_*`
/// per schema stem, with the SAME contract as the generated `.mjs`
/// validators — the argument is a parsed JSON value, the return is a JS
/// array of `{ instancePath, schemaPath }` error objects, and an empty
/// array means valid.
#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::JsValue;

    type Validate = fn(&serde_json::Value) -> Vec<(String, String)>;

    /// Convert the JS instance to a JSON value, run the generated validator,
    /// and lift the `(instancePath, schemaPath)` tuples into JS error
    /// objects. A value that cannot be represented as JSON (e.g. contains
    /// `undefined`) is reported as a single conversion error so the caller
    /// treats the frame as malformed and drops it.
    fn validate_js(validate: Validate, instance: &JsValue) -> JsValue {
        let errors = match serde_wasm_bindgen::from_value::<serde_json::Value>(instance.clone()) {
            Ok(value) => validate(&value),
            Err(_) => vec![(String::new(), String::from("/wasm/conversion"))],
        };
        let arr = js_sys::Array::new();
        for (instance_path, schema_path) in errors {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("instancePath"),
                &JsValue::from_str(&instance_path),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("schemaPath"),
                &JsValue::from_str(&schema_path),
            );
            arr.push(&obj.into());
        }
        arr.into()
    }

    macro_rules! wasm_validators {
        ($($name:ident => $stem:ident),+ $(,)?) => {
            $(
                #[wasm_bindgen]
                pub fn $name(instance: &JsValue) -> JsValue {
                    validate_js(crate::generated::$stem::validate, instance)
                }
            )+
        };
    }

    wasm_validators! {
        validate_ack => ack,
        validate_assistant => assistant,
        validate_console_entry => console_entry,
        validate_error => error,
        validate_pong => pong,
        validate_prompt => prompt,
        validate_provider_models => provider_models,
        validate_ready => ready,
        validate_rename => rename,
        validate_session_meta => session_meta,
        validate_session_rename => session_rename,
        validate_tool_call => tool_call,
    }
}

#[cfg(test)]
mod tests;
