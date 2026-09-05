//! Contract tests for the public JSON options schema (issue #2).

use super::*;

fn parse(json: &str) -> ConvertOptions {
    options_from_json(json).unwrap_or_else(|e| panic!("failed to parse {json}: {e}"))
}

fn err(json: &str) -> String {
    match options_from_json(json) {
        Err(SpryteoError::InvalidInput(m)) => m,
        other => panic!("expected InvalidInput for {json}, got {other:?}"),
    }
}

// ── the exact examples in the README / docs / MCP tool description ───────

#[test]
fn readme_node_example_parses() {
    // README: JSON.stringify({ mode: 'icon', colors: 8 })
    let o = parse(r#"{"mode":"icon","colors":8}"#);
    assert_eq!(o.mode, Mode::Icon);
    assert_eq!(o.colors, ColorSpec::N(8));
}

#[test]
fn docs_example_with_css_preset_parses() {
    // The documented example that previously failed on `mode` and `colors`
    // and silently dropped `css` (the Rust field is named emit_css).
    let o = parse(r#"{"mode":"icon","colors":8,"css":"draw"}"#);
    assert_eq!(o.mode, Mode::Icon);
    assert_eq!(o.colors, ColorSpec::N(8));
    assert_eq!(o.emit_css, Some(Preset::Draw));
}

#[test]
fn mcp_tool_description_example_parses() {
    let o = parse(r#"{"stroke": true, "mode": "photo"}"#);
    assert!(o.stroke);
    assert_eq!(o.mode, Mode::Photo);
}

// ── enum spelling ────────────────────────────────────────────────────────

#[test]
fn every_mode_spelling_round_trips() {
    for (json, expected) in [
        ("auto", Mode::Auto),
        ("icon", Mode::Icon),
        ("pixel-art", Mode::PixelArt),
        ("line-art", Mode::LineArt),
        ("photo", Mode::Photo),
    ] {
        let o = parse(&format!(r#"{{"mode":"{json}"}}"#));
        assert_eq!(o.mode, expected, "mode {json}");
        let back = serde_json::to_value(&o).unwrap();
        assert_eq!(back["mode"], json, "mode {json} must serialize back");
    }
}

#[test]
fn underscore_mode_spellings_are_accepted_as_aliases() {
    assert_eq!(parse(r#"{"mode":"pixel_art"}"#).mode, Mode::PixelArt);
    assert_eq!(parse(r#"{"mode":"pixelart"}"#).mode, Mode::PixelArt);
    assert_eq!(parse(r#"{"mode":"line_art"}"#).mode, Mode::LineArt);
}

#[test]
fn rust_variant_names_are_rejected() {
    // "PixelArt" was the *only* accepted spelling before this change.
    assert!(err(r#"{"mode":"PixelArt"}"#).contains("options JSON"));
}

#[test]
fn all_simple_enums_use_lowercase_strings() {
    let o = parse(
        r#"{"layering":"cutout","gradients":"off","grouping":"flat",
            "idStyle":"sequential","transformOrigin":"baked",
            "background":"rect","output":"svg-pretty","css":"pop"}"#,
    );
    assert_eq!(o.layering, Layering::Cutout);
    assert_eq!(o.gradients, Tri::Off);
    assert_eq!(o.grouping, Grouping::Flat);
    assert_eq!(o.id_style, IdStyle::Sequential);
    assert_eq!(o.transform_origin, TOrigin::Baked);
    assert_eq!(o.background, Background::Rect);
    assert_eq!(o.output, OutputFormat::SvgPretty);
    assert_eq!(o.emit_css, Some(Preset::Pop));
}

#[test]
fn pretty_is_accepted_as_an_output_alias() {
    assert_eq!(
        parse(r#"{"output":"pretty"}"#).output,
        OutputFormat::SvgPretty
    );
}

// ── colors: auto / integer / palette (issues #2 and #21) ─────────────────

#[test]
fn colors_auto_string() {
    assert_eq!(parse(r#"{"colors":"auto"}"#).colors, ColorSpec::Auto);
}

#[test]
fn colors_integer() {
    assert_eq!(parse(r#"{"colors":2}"#).colors, ColorSpec::N(2));
    assert_eq!(parse(r#"{"colors":64}"#).colors, ColorSpec::N(64));
}

#[test]
fn colors_hex_palette() {
    let o = parse(r##"{"colors":["#ff0000","#00ff00","#0000ff"]}"##);
    assert_eq!(
        o.colors,
        ColorSpec::Palette(vec![
            Rgb { r: 255, g: 0, b: 0 },
            Rgb { r: 0, g: 255, b: 0 },
            Rgb { r: 0, g: 0, b: 255 },
        ])
    );
}

#[test]
fn palette_accepts_short_hex_and_missing_hash() {
    let o = parse(r##"{"colors":["#f00","0000ff"]}"##);
    assert_eq!(
        o.colors,
        ColorSpec::Palette(vec![Rgb { r: 255, g: 0, b: 0 }, Rgb { r: 0, g: 0, b: 255 },])
    );
}

#[test]
fn palette_accepts_rgb_objects() {
    let o = parse(r#"{"colors":[{"r":1,"g":2,"b":3}]}"#);
    assert_eq!(o.colors, ColorSpec::Palette(vec![Rgb { r: 1, g: 2, b: 3 }]));
}

#[test]
fn palette_order_is_preserved() {
    let o = parse(r##"{"colors":["#030201","#010203"]}"##);
    match o.colors {
        ColorSpec::Palette(p) => {
            assert_eq!(p[0], Rgb { r: 3, g: 2, b: 1 });
            assert_eq!(p[1], Rgb { r: 1, g: 2, b: 3 });
        }
        other => panic!("expected palette, got {other:?}"),
    }
}

#[test]
fn palette_duplicates_are_preserved_not_deduplicated() {
    // Deduplicating would silently change the index a brand colour maps to.
    let o = parse(r##"{"colors":["#ff0000","#ff0000"]}"##);
    match o.colors {
        ColorSpec::Palette(p) => assert_eq!(p.len(), 2),
        other => panic!("expected palette, got {other:?}"),
    }
}

#[test]
fn invalid_hex_colors_are_rejected() {
    assert!(err(r##"{"colors":["#gg0000"]}"##).contains("invalid hex colour"));
    assert!(err(r##"{"colors":["#ff00"]}"##).contains("invalid hex colour"));
    assert!(err(r#"{"colors":["nonsense"]}"#).contains("invalid hex colour"));
}

#[test]
fn nonsense_colors_string_is_rejected_with_a_useful_message() {
    let m = err(r#"{"colors":"eight"}"#);
    assert!(m.contains("colors"), "unhelpful message: {m}");
}

#[test]
fn negative_colors_is_rejected() {
    assert!(err(r#"{"colors":-1}"#).contains("negative"));
}

#[test]
fn colors_round_trips_through_serialization() {
    for json in [r#""auto""#, "8", r##"["#ff0000"]"##] {
        let o = parse(&format!(r#"{{"colors":{json}}}"#));
        let back = serde_json::to_value(&o).unwrap();
        let reparsed = parse(&format!(r#"{{"colors":{}}}"#, back["colors"]));
        assert_eq!(o.colors, reparsed.colors, "colors {json}");
    }
}

// ── alphaMode ────────────────────────────────────────────────────────────

#[test]
fn alpha_mode_string_forms() {
    assert_eq!(parse(r#"{"alphaMode":"keep"}"#).alpha_mode, AlphaMode::Keep);
    assert_eq!(
        parse(r#"{"alphaMode":"matte:#ff00ff"}"#).alpha_mode,
        AlphaMode::Matte(Rgb {
            r: 255,
            g: 0,
            b: 255
        })
    );
    assert_eq!(
        parse(r#"{"alphaMode":"threshold:128"}"#).alpha_mode,
        AlphaMode::Threshold(128)
    );
}

#[test]
fn alpha_mode_object_forms() {
    assert_eq!(
        parse(r##"{"alphaMode":{"matte":"#00ff00"}}"##).alpha_mode,
        AlphaMode::Matte(Rgb { r: 0, g: 255, b: 0 })
    );
    assert_eq!(
        parse(r#"{"alphaMode":{"threshold":200}}"#).alpha_mode,
        AlphaMode::Threshold(200)
    );
}

#[test]
fn alpha_mode_round_trips() {
    for json in [r#""keep""#, r#""matte:#ff00ff""#, r#""threshold:128""#] {
        let o = parse(&format!(r#"{{"alphaMode":{json}}}"#));
        let back = serde_json::to_value(&o).unwrap();
        assert_eq!(back["alphaMode"].to_string(), json, "alphaMode {json}");
    }
}

#[test]
fn invalid_alpha_mode_is_rejected() {
    assert!(err(r#"{"alphaMode":"nonsense"}"#).contains("alphaMode"));
    assert!(err(r#"{"alphaMode":"matte:zzz"}"#).contains("invalid hex colour"));
    assert!(err(r#"{"alphaMode":"threshold:999"}"#).contains("0-255"));
    assert!(err(r#"{"alphaMode":{"bogus":1}}"#).contains("unknown alphaMode key"));
}

// ── unknown fields are an error, not silently dropped ────────────────────

#[test]
fn unknown_keys_are_rejected() {
    let m = err(r#"{"mode":"icon","nonsenseKey":true}"#);
    assert!(
        m.contains("nonsenseKey"),
        "error must name the offending key: {m}"
    );
}

#[test]
fn the_old_internal_key_name_still_works_as_an_alias() {
    // emit_css was the accidental public name; keep it working so existing
    // callers do not break, but `css` is the documented key.
    assert_eq!(parse(r#"{"emit_css":"fade"}"#).emit_css, Some(Preset::Fade));
    assert_eq!(parse(r#"{"emitCss":"fade"}"#).emit_css, Some(Preset::Fade));
}

#[test]
fn snake_case_keys_are_accepted_as_aliases() {
    let o = parse(
        r#"{"id_style":"none","transform_origin":"baked","alpha_mode":"keep",
            "max_trace_dimension":64,"current_color":true,"max_pixels":100,
            "max_input_bytes":200,"timeout_ms":300}"#,
    );
    assert_eq!(o.id_style, IdStyle::None);
    assert_eq!(o.transform_origin, TOrigin::Baked);
    assert_eq!(o.max_trace_dimension, Some(64));
    assert!(o.current_color);
    assert_eq!(o.max_pixels, 100);
    assert_eq!(o.max_input_bytes, 200);
    assert_eq!(o.timeout_ms, Some(300));
}

#[test]
fn camel_case_keys_are_the_documented_form() {
    let o = parse(
        r#"{"idStyle":"none","transformOrigin":"baked","alphaMode":"keep",
            "maxTraceDimension":64,"currentColor":true,"maxPixels":100,
            "maxInputBytes":200,"timeoutMs":300}"#,
    );
    assert_eq!(o.id_style, IdStyle::None);
    assert_eq!(o.max_trace_dimension, Some(64));
    assert!(o.current_color);
    assert_eq!(o.timeout_ms, Some(300));
}

// ── merge semantics ──────────────────────────────────────────────────────

#[test]
fn empty_input_yields_defaults() {
    for json in ["", "  ", "{}"] {
        let o = parse(json);
        let d = ConvertOptions::default();
        assert_eq!(o.mode, d.mode);
        assert_eq!(o.colors, d.colors);
        assert_eq!(o.precision, d.precision);
    }
}

#[test]
fn omitted_fields_keep_their_defaults() {
    let o = parse(r#"{"mode":"photo"}"#);
    let d = ConvertOptions::default();
    assert_eq!(o.mode, Mode::Photo);
    assert_eq!(o.tolerance, d.tolerance);
    assert_eq!(o.turdsize, d.turdsize);
    assert_eq!(o.max_pixels, d.max_pixels);
}

#[test]
fn a_patch_applies_over_a_custom_base() {
    let base = ConvertOptions {
        turdsize: 99,
        mode: Mode::Photo,
        ..Default::default()
    };
    let o = options_from_json_with_base(r#"{"mode":"icon"}"#, base).unwrap();
    assert_eq!(o.mode, Mode::Icon, "patch wins");
    assert_eq!(o.turdsize, 99, "unpatched base field survives");
}

#[test]
fn non_object_json_is_rejected_clearly() {
    assert!(err("[1,2,3]").contains("expected a JSON object"));
    assert!(err("42").contains("expected a JSON object"));
    assert!(err(r#""a string""#).contains("expected a JSON object"));
}

#[test]
fn malformed_json_is_rejected() {
    assert!(err("{not valid json").contains("Failed to parse options JSON"));
}

#[test]
fn full_options_round_trip_through_json() {
    // Serializing the defaults must produce something the parser accepts,
    // which is what keeps the two encodings from drifting apart.
    let d = ConvertOptions::default();
    let json = serde_json::to_string(&d).unwrap();
    let back = parse(&json);
    assert_eq!(back.mode, d.mode);
    assert_eq!(back.colors, d.colors);
    assert_eq!(back.alpha_mode, d.alpha_mode);
    assert_eq!(back.output, d.output);
    assert_eq!(back.emit_css, d.emit_css);
}

#[test]
fn serialized_defaults_use_the_public_key_names() {
    let json = serde_json::to_value(ConvertOptions::default()).unwrap();
    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("css"), "preset key must be `css`");
    assert!(!obj.contains_key("emit_css"));
    assert!(obj.contains_key("alphaMode"));
    assert!(obj.contains_key("maxTraceDimension"));
    assert!(obj.contains_key("idStyle"));
}

// ── hex helpers ──────────────────────────────────────────────────────────

#[test]
fn hex_helpers_round_trip() {
    for s in ["#000000", "#ffffff", "#1a2b3c"] {
        let c = parse_hex_color(s).unwrap();
        assert_eq!(format_hex_color(&c), s);
    }
}

#[test]
fn short_hex_expands_by_digit_doubling() {
    assert_eq!(
        parse_hex_color("#abc").unwrap(),
        Rgb {
            r: 0xaa,
            g: 0xbb,
            b: 0xcc
        }
    );
}

#[test]
fn parse_alpha_mode_matches_the_json_spellings() {
    // The CLI flag parser and the JSON deserializer share this function, so
    // `--alpha-mode X` and `{"alphaMode":"X"}` cannot diverge.
    assert_eq!(parse_alpha_mode("keep").unwrap(), AlphaMode::Keep);
    assert_eq!(
        parse_alpha_mode("matte:#ff00ff").unwrap(),
        AlphaMode::Matte(Rgb {
            r: 255,
            g: 0,
            b: 255
        })
    );
    assert_eq!(
        parse_alpha_mode("threshold:255").unwrap(),
        AlphaMode::Threshold(255)
    );
    assert!(parse_alpha_mode("matte").is_err());
}
