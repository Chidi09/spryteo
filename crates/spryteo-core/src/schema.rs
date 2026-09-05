//! The metadata sidecar's published contract (#22).
//!
//! The sidecar is the machine-facing half of a conversion, so its shape is
//! an interface other people write code against. These generators emit that
//! interface in the two forms consumers actually consume — JSON Schema for
//! validators and codegen, TypeScript for the Node and WASM bindings — from
//! the Rust types themselves, so a field cannot be added, renamed or made
//! optional without the published contract moving with it.
//!
//! The checked-in copies under `schema/` are compared against fresh output
//! by a test; see `schema_tests.rs` for how to regenerate them.

use crate::ir::{ConvertResult, Meta};
use serde_json::Value;
use std::fmt::Write as _;

/// The JSON Schema for [`Meta`], as pretty-printed JSON.
pub fn meta_json_schema() -> String {
    let schema = schemars::schema_for!(Meta);
    let mut out = serde_json::to_string_pretty(&schema).expect("a schema serialises");
    out.push('\n');
    out
}

/// TypeScript declarations for [`Meta`] and everything it references.
///
/// Generated from the same JSON Schema rather than written by hand, so the
/// two cannot drift apart. The translation covers only the constructs these
/// types actually use — objects, arrays, fixed-length tuples, nullable
/// references, string enums and internally tagged unions — and says so
/// loudly if it meets anything else, because silently emitting `any` would
/// turn a contract into a suggestion.
pub fn meta_typescript() -> String {
    let schema = serde_json::to_value(schemars::schema_for!(Meta)).expect("a schema serialises");

    let mut out = String::new();
    out.push_str("// Generated from the Rust `Meta` type by spryteo-core.\n");
    out.push_str("// Do not edit by hand: run `cargo test -p spryteo-core` to check it,\n");
    out.push_str("// and `UPDATE_SCHEMA=1 cargo test -p spryteo-core` to regenerate.\n");

    let empty = serde_json::Map::new();
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    // Named types first, in a stable order, then the root.
    let mut names: Vec<&String> = defs.keys().collect();
    names.sort();
    for name in names {
        write_declaration(&mut out, name, &defs[name]);
    }
    write_declaration(&mut out, "Meta", &schema);

    // The shape every surface actually returns, so a binding can type its
    // result rather than leaving callers with `any`.
    let result =
        serde_json::to_value(schemars::schema_for!(ConvertResult)).expect("a schema serialises");
    write_declaration(&mut out, "ConvertResult", &result);
    out
}

fn write_declaration(out: &mut String, name: &str, schema: &Value) {
    out.push('\n');
    write_doc(out, schema, "");

    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        let rendered: Vec<String> = variants.iter().map(render_variant).collect();
        // A union of string literals stays on one line; object variants get
        // one line each, which is how a discriminated union reads best.
        let inline = rendered.iter().all(|v| !v.contains('\n'));
        if inline {
            let _ = writeln!(out, "export type {name} = {};", rendered.join(" | "));
        } else {
            let _ = writeln!(out, "export type {name} =");
            for (i, variant) in rendered.iter().enumerate() {
                let end = if i + 1 == rendered.len() { ";" } else { "" };
                let _ = writeln!(out, "  | {variant}{end}");
            }
        }
        return;
    }

    let _ = writeln!(out, "export interface {name} {{");
    write_properties(out, schema, "  ");
    out.push_str("}\n");
}

/// One arm of a `oneOf`: either a string-literal union or an object shape.
fn render_variant(variant: &Value) -> String {
    if let Some(consts) = variant.get("enum").and_then(Value::as_array) {
        return consts
            .iter()
            .map(|c| format!("{:?}", c.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    if let Some(c) = variant.get("const").and_then(Value::as_str) {
        return format!("{c:?}");
    }
    let mut body = String::new();
    write_properties(&mut body, variant, "      ");
    format!("{{\n{body}    }}")
}

fn write_properties(out: &mut String, schema: &Value, indent: &str) {
    let empty = serde_json::Map::new();
    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let mut names: Vec<&String> = props.keys().collect();
    names.sort();
    for name in names {
        let prop = &props[name];
        write_doc(out, prop, indent);
        // A field with a serde default is optional to *write* but always
        // present when we write it; `?` is the honest declaration for a
        // consumer that may also be reading an older sidecar.
        let optional = if required.contains(&name.as_str()) {
            ""
        } else {
            "?"
        };
        let _ = writeln!(out, "{indent}{name}{optional}: {};", type_of(prop));
    }
}

fn write_doc(out: &mut String, schema: &Value, indent: &str) {
    let Some(doc) = schema.get("description").and_then(Value::as_str) else {
        return;
    };
    let _ = writeln!(out, "{indent}/**");
    for line in doc.lines() {
        if line.is_empty() {
            let _ = writeln!(out, "{indent} *");
        } else {
            let _ = writeln!(out, "{indent} * {line}");
        }
    }
    let _ = writeln!(out, "{indent} */");
}

fn type_of(schema: &Value) -> String {
    // A `const` is the discriminator of a tagged union, and must survive as
    // a literal type — degrade it to `string` and `switch (paint.kind)`
    // stops narrowing, which is the one thing the union exists to do.
    if let Some(c) = schema.get("const").and_then(Value::as_str) {
        return format!("{c:?}");
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .map(|v| format!("{:?}", v.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return reference
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string();
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        // `Option<T>` arrives as `T | null`.
        let parts: Vec<String> = any_of.iter().map(type_of).collect();
        return parts.join(" | ");
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => "string".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("integer") | Some("number") => "number".to_string(),
        Some("null") => "null".to_string(),
        Some("array") => {
            if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
                let parts: Vec<String> = prefix.iter().map(type_of).collect();
                return format!("[{}]", parts.join(", "));
            }
            match schema.get("items") {
                Some(items) => format!("{}[]", type_of(items)),
                None => "unknown[]".to_string(),
            }
        }
        other => panic!(
            "the TypeScript generator does not handle {other:?}; teach it \
             this construct rather than letting the contract go untyped"
        ),
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
