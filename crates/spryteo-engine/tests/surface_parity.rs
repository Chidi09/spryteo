//! Cross-surface parity (issue #19 acceptance criterion).
//!
//! Every native surface must produce byte-identical SVG and metadata for
//! the same input and options. Before the shared engine existed, the CLI,
//! napi addon, WASM binding and MCP server each ran their own copy of the
//! pipeline and had already drifted — semantic grouping was wired only
//! into the CLI, so `grouping: "semantic"` silently did nothing on three
//! of the four surfaces.
//!
//! These tests call the engine the way each adapter calls it, rather than
//! linking the adapters themselves: the napi and wasm crates are cdylibs
//! that cannot be depended on from a test, and the MCP server is a binary.
//! What is pinned here is that every adapter entry shape agrees.

use spryteo_core::{AlphaMode, ColorSpec, ConvertOptions, Grouping, Mode, Rgb, SpryteoError};
use spryteo_engine::{convert, convert_with, convert_with_timeout, EngineRequest};

fn fixture(size: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(size, size);
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let d = (dx * dx + dy * dy).sqrt();
            let px = if d < c * 0.35 {
                image::Rgba([20, 120, 220, 255])
            } else if d < c * 0.7 {
                image::Rgba([230, 80, 40, 255])
            } else {
                image::Rgba([250, 250, 250, 255])
            };
            img.put_pixel(x, y, px);
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode fixture");
    out.into_inner()
}

/// The option sets each surface exposes, exercised across all of them.
fn option_matrix() -> Vec<(&'static str, ConvertOptions)> {
    vec![
        ("defaults", ConvertOptions::default()),
        (
            "icon-4-colors",
            ConvertOptions {
                mode: Mode::Icon,
                colors: ColorSpec::N(4),
                ..Default::default()
            },
        ),
        (
            "semantic-grouping",
            ConvertOptions {
                grouping: Grouping::Semantic,
                ..Default::default()
            },
        ),
        (
            "flat-grouping",
            ConvertOptions {
                grouping: Grouping::Flat,
                ..Default::default()
            },
        ),
        (
            "matte-black",
            ConvertOptions {
                alpha_mode: AlphaMode::Matte(Rgb { r: 0, g: 0, b: 0 }),
                ..Default::default()
            },
        ),
        (
            "max-dim-32",
            ConvertOptions {
                max_trace_dimension: Some(32),
                ..Default::default()
            },
        ),
        (
            "stroke",
            ConvertOptions {
                stroke: true,
                ..Default::default()
            },
        ),
    ]
}

/// `convert` (the plain entry the MCP and Node adapters use via
/// `convert_with_timeout`) and `convert_with` (used by the CLI and WASM
/// adapters, which supply a token) must agree exactly.
#[test]
fn every_engine_entry_point_agrees() {
    let bytes = fixture(96);
    for (name, opts) in option_matrix() {
        let plain = convert(&bytes, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
        let via_request = convert_with(EngineRequest::new(&bytes, &opts))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let via_timeout =
            convert_with_timeout(&bytes, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));

        assert_eq!(plain.svg, via_request.svg, "{name}: svg differs");
        assert_eq!(plain.svg, via_timeout.svg, "{name}: svg differs");

        let meta_a = serde_json::to_string(&plain.meta).unwrap();
        let meta_b = serde_json::to_string(&via_request.meta).unwrap();
        let meta_c = serde_json::to_string(&via_timeout.meta).unwrap();
        assert_eq!(meta_a, meta_b, "{name}: metadata differs");
        assert_eq!(meta_a, meta_c, "{name}: metadata differs");
    }
}

/// An explicitly empty mask slice must behave exactly like supplying none.
#[test]
fn empty_mask_slice_matches_no_masks() {
    let bytes = fixture(96);
    let opts = ConvertOptions {
        grouping: Grouping::Semantic,
        ..Default::default()
    };

    let without = convert(&bytes, &opts).unwrap();
    let with_empty = convert_with(EngineRequest::new(&bytes, &opts).with_masks(&[])).unwrap();
    assert_eq!(without.svg, with_empty.svg);
}

/// Repeated conversion must be bit-stable — the parity guarantee is
/// meaningless if a single surface is not itself deterministic.
#[test]
fn conversion_is_stable_across_runs() {
    let bytes = fixture(96);
    for (name, opts) in option_matrix() {
        let first = convert(&bytes, &opts).unwrap().svg;
        for _ in 0..3 {
            assert_eq!(
                first,
                convert(&bytes, &opts).unwrap().svg,
                "{name} unstable"
            );
        }
    }
}

/// Semantic grouping must actually do something on every surface, not be a
/// silent alias for component grouping. This is the specific drift issue
/// #19 and #11 both call out.
#[test]
fn semantic_grouping_is_not_a_silent_no_op() {
    let bytes = fixture(96);

    let component = ConvertOptions {
        grouping: Grouping::Component,
        ..Default::default()
    };
    let semantic = ConvertOptions {
        grouping: Grouping::Semantic,
        ..Default::default()
    };

    let a = convert(&bytes, &component).unwrap().svg;
    let b = convert(&bytes, &semantic).unwrap().svg;

    assert_ne!(
        a, b,
        "semantic grouping produced identical output to component grouping, \
         which is what the pre-engine surfaces did when they ignored the option"
    );
}

/// Option validation is centralised, so an invalid value must be rejected
/// identically no matter which entry point a surface uses.
#[test]
fn invalid_options_are_rejected_by_every_entry_point() {
    let bytes = fixture(32);
    let opts = ConvertOptions {
        colors: ColorSpec::N(0),
        ..Default::default()
    };

    for (label, result) in [
        ("convert", convert(&bytes, &opts)),
        (
            "convert_with",
            convert_with(EngineRequest::new(&bytes, &opts)),
        ),
        ("convert_with_timeout", convert_with_timeout(&bytes, &opts)),
    ] {
        match result {
            Err(SpryteoError::InvalidInput(m)) => {
                assert!(m.contains("colors"), "{label}: unexpected message {m}")
            }
            other => panic!("{label}: expected InvalidInput, got {other:?}"),
        }
    }
}
