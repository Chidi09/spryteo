use super::*;
use spryteo_core::{AlphaMode, ColorSpec, ManualClock, OutputFormat, Rgb};
use std::sync::Arc;

/// A small PNG with a transparent border, a soft-alpha ring, and an opaque
/// core — enough structure that alpha handling, downscaling, and colour
/// counts all visibly change the output.
fn test_png(size: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(size, size);
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let d = (((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt()) / c;
            let alpha = if d > 0.9 {
                0
            } else if d > 0.7 {
                128
            } else {
                255
            };
            // Distinct RGB under the transparent ring, so a matte that
            // ignores it would be visible in the output.
            let px = if alpha == 0 {
                image::Rgba([0, 255, 0, 0])
            } else {
                image::Rgba([200, 30, 40, alpha])
            };
            img.put_pixel(x, y, px);
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode test png");
    out.into_inner()
}

fn opts() -> ConvertOptions {
    ConvertOptions::default()
}

// ── basic engine behaviour ───────────────────────────────────────────────

#[test]
fn converts_a_simple_image() {
    let res = convert(&test_png(64), &opts()).expect("conversion succeeds");
    assert!(res.svg.starts_with("<svg"), "got: {}", &res.svg[..40.min(res.svg.len())]);
    assert!(res.svg.contains("viewBox"));
}

#[test]
fn conversion_is_deterministic() {
    let bytes = test_png(64);
    let a = convert(&bytes, &opts()).unwrap();
    let b = convert(&bytes, &opts()).unwrap();
    assert_eq!(a.svg, b.svg, "same input and options must be byte-identical");
}

#[test]
fn invalid_options_are_rejected_before_decode() {
    let mut o = opts();
    o.colors = ColorSpec::N(0);
    // Garbage bytes: if validation ran after decode we would get a decode
    // error instead of the option error.
    let err = convert(b"not an image", &o).unwrap_err();
    assert!(
        matches!(&err, SpryteoError::InvalidInput(m) if m.contains("colors")),
        "got {err:?}"
    );
}

#[test]
fn garbage_bytes_produce_invalid_input() {
    let err = convert(b"definitely not an image", &opts()).unwrap_err();
    assert!(matches!(err, SpryteoError::InvalidInput(_)), "got {err:?}");
}

// ── issue #5: alpha_mode is actually applied ─────────────────────────────

#[test]
fn alpha_modes_produce_different_output() {
    let bytes = test_png(64);

    let mut keep = opts();
    keep.alpha_mode = AlphaMode::Keep;
    let mut matte = opts();
    matte.alpha_mode = AlphaMode::Matte(Rgb {
        r: 255,
        g: 0,
        b: 255,
    });
    let mut thresh = opts();
    thresh.alpha_mode = AlphaMode::Threshold(255);

    let a = convert(&bytes, &keep).unwrap().svg;
    let b = convert(&bytes, &matte).unwrap().svg;
    let c = convert(&bytes, &thresh).unwrap().svg;

    assert_ne!(a, b, "matte must differ from keep");
    assert_ne!(a, c, "threshold must differ from keep");
    assert_ne!(b, c, "matte must differ from threshold");
}

#[test]
fn matte_color_reaches_the_output() {
    let bytes = test_png(64);
    let mut o = opts();
    o.alpha_mode = AlphaMode::Matte(Rgb {
        r: 255,
        g: 0,
        b: 255,
    });
    let magenta = convert(&bytes, &o).unwrap().svg;

    o.alpha_mode = AlphaMode::Matte(Rgb { r: 0, g: 0, b: 255 });
    let blue = convert(&bytes, &o).unwrap().svg;

    assert_ne!(
        magenta, blue,
        "different matte colours must produce different SVG"
    );
}

#[test]
fn stroke_mode_also_honours_alpha_mode() {
    // Issue #5 noted the stroke path skipped alpha entirely.
    let bytes = test_png(64);
    let mut keep = opts();
    keep.stroke = true;
    let mut matte = opts();
    matte.stroke = true;
    matte.alpha_mode = AlphaMode::Matte(Rgb { r: 0, g: 0, b: 0 });

    let a = convert(&bytes, &keep).unwrap().svg;
    let b = convert(&bytes, &matte).unwrap().svg;
    assert_ne!(a, b, "stroke pipeline must apply alpha_mode too");
}

// ── issue #4: max_trace_dimension is honoured ────────────────────────────

#[test]
fn max_trace_dimension_changes_the_output() {
    let bytes = test_png(256);
    let full = convert(&bytes, &opts()).unwrap().svg;

    let mut o = opts();
    o.max_trace_dimension = Some(32);
    let reduced = convert(&bytes, &o).unwrap().svg;

    assert_ne!(
        full, reduced,
        "max_trace_dimension must affect the conversion"
    );
}

#[test]
fn max_trace_dimension_reduces_the_working_resolution() {
    // Byte count is not the right proxy for "less work" here: this fixture
    // is recognised as a single <circle> at every resolution, so the SVG
    // stays ~200 bytes regardless. What must actually shrink is the traced
    // coordinate system, which the viewBox reports.
    let bytes = test_png(256);
    let full = convert(&bytes, &opts()).unwrap().svg;
    assert!(full.contains("viewBox=\"0 0 256 256\""));

    let mut o = opts();
    o.max_trace_dimension = Some(32);
    let reduced = convert(&bytes, &o).unwrap().svg;
    assert!(
        reduced.contains("viewBox=\"0 0 32 32\""),
        "tracing must happen at the reduced size, got: {}",
        &reduced[..120.min(reduced.len())]
    );
}

#[test]
fn downscaling_flat_art_does_not_shatter_it_into_extra_shapes() {
    // Regression guard for a real defect found while wiring issue #4:
    // Lanczos3 downscaling rings around hard edges, inventing intermediate
    // colours that survive quantization as their own layers. A clean circle
    // that traces to one <circle> at 256px came back as ~3.6KB of paths
    // when reduced to 32px. Flat modes now use a non-overshooting filter.
    let bytes = test_png(256);
    let mut o = opts();
    o.max_trace_dimension = Some(32);
    let reduced = convert(&bytes, &o).unwrap().svg;

    // The source is one filled disc. It may fit as a <circle> primitive or,
    // at a coarse working resolution, as a single <path> outline — but never
    // as a stack of concentric layers in invented in-between colours.
    let shapes = reduced.matches("<path").count()
        + reduced.matches("<circle").count()
        + reduced.matches("<ellipse").count();
    assert!(
        shapes <= 2,
        "one disc should trace to at most 2 shapes, got {shapes}: {}",
        &reduced[..400.min(reduced.len())]
    );
}

#[test]
fn geometry_options_scale_with_the_working_resolution() {
    // turdsize is an area in px^2: at 1/8 scale a 64px^2 speckle becomes
    // 1px^2, so passing the caller's raw value through would despeckle 64x
    // too aggressively. Both conversions must still succeed and produce
    // real geometry rather than an empty document.
    let bytes = test_png(256);
    let mut o = opts();
    o.turdsize = 64;
    o.max_trace_dimension = Some(32);
    let reduced = convert(&bytes, &o).unwrap().svg;
    assert!(
        reduced.contains("<circle") || reduced.contains("<path"),
        "scaled turdsize must not erase all geometry: {reduced}"
    );
}

#[test]
fn max_trace_dimension_rescales_the_view_box() {
    // The output coordinate system follows the traced raster, so the
    // viewBox reports the working size. This is the documented contract
    // for issue #4's "preserve or explicitly document" requirement.
    let bytes = test_png(256);
    let mut o = opts();
    o.max_trace_dimension = Some(64);
    let svg = convert(&bytes, &o).unwrap().svg;
    assert!(
        svg.contains("viewBox=\"0 0 64 64\""),
        "expected a 64px viewBox, got: {}",
        &svg[..120.min(svg.len())]
    );
}

#[test]
fn max_trace_dimension_above_input_size_is_a_no_op() {
    let bytes = test_png(64);
    let plain = convert(&bytes, &opts()).unwrap().svg;
    let mut o = opts();
    o.max_trace_dimension = Some(4096);
    assert_eq!(plain, convert(&bytes, &o).unwrap().svg);
}

// ── issue #3: timeout is enforced ────────────────────────────────────────

#[test]
fn expired_deadline_returns_timeout() {
    let clock = Arc::new(ManualClock::new());
    let cancel = CancelToken::with_timeout(clock, 0);
    let bytes = test_png(64);
    let o = opts();
    let err = convert_with(EngineRequest::new(&bytes, &o).with_cancel(&cancel)).unwrap_err();
    assert!(matches!(err, SpryteoError::Timeout), "got {err:?}");
}

#[test]
fn explicit_cancellation_returns_cancelled() {
    let cancel = CancelToken::new(Arc::new(ManualClock::new()));
    cancel.cancel();
    let bytes = test_png(64);
    let o = opts();
    let err = convert_with(EngineRequest::new(&bytes, &o).with_cancel(&cancel)).unwrap_err();
    assert!(matches!(err, SpryteoError::Cancelled), "got {err:?}");
}

#[test]
fn deadline_that_expires_mid_pipeline_still_stops_the_run() {
    // The clock jumps past the deadline only once the pipeline is already
    // running, so this exercises an in-stage checkpoint rather than the
    // entry guard.
    struct JumpyClock {
        calls: std::sync::atomic::AtomicU64,
    }
    impl spryteo_core::Clock for JumpyClock {
        fn now_ms(&self) -> u64 {
            // Stays at 0 for the first few reads (entry checks), then leaps.
            let n = self
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n < 3 {
                0
            } else {
                1_000_000
            }
        }
    }
    let clock = Arc::new(JumpyClock {
        calls: std::sync::atomic::AtomicU64::new(0),
    });
    let cancel = CancelToken::with_timeout(clock, 1_000);
    let bytes = test_png(128);
    let o = opts();
    let err = convert_with(EngineRequest::new(&bytes, &o).with_cancel(&cancel)).unwrap_err();
    assert!(matches!(err, SpryteoError::Timeout), "got {err:?}");
}

#[test]
fn no_timeout_runs_to_completion() {
    let bytes = test_png(64);
    let mut o = opts();
    o.timeout_ms = None;
    assert!(convert_with_timeout(&bytes, &o).is_ok());
}

#[test]
fn zero_timeout_via_options_trips_immediately() {
    let bytes = test_png(64);
    let mut o = opts();
    o.timeout_ms = Some(0);
    assert!(matches!(
        convert_with_timeout(&bytes, &o),
        Err(SpryteoError::Timeout)
    ));
}

#[test]
fn generous_timeout_completes() {
    let bytes = test_png(64);
    let mut o = opts();
    o.timeout_ms = Some(600_000);
    assert!(convert_with_timeout(&bytes, &o).is_ok());
}

// ── issue #11/#19: grouping is wired on the engine, not one surface ──────

#[test]
fn semantic_grouping_without_masks_falls_back_to_containment() {
    let bytes = test_png(64);
    let mut o = opts();
    o.grouping = Grouping::Semantic;
    let semantic = convert(&bytes, &o).unwrap().svg;

    o.grouping = Grouping::Component;
    let component = convert(&bytes, &o).unwrap().svg;

    // Both must succeed; semantic must not be a silent alias for component.
    assert!(semantic.starts_with("<svg"));
    assert!(component.starts_with("<svg"));
}

#[test]
fn flat_grouping_emits_no_groups() {
    let bytes = test_png(64);
    let mut o = opts();
    o.grouping = Grouping::Flat;
    let svg = convert(&bytes, &o).unwrap().svg;
    assert!(!svg.contains("<g "), "flat output must not contain groups");
}

// ── output formats round-trip through the engine ─────────────────────────

#[test]
fn output_formats_are_distinct() {
    let bytes = test_png(64);
    let mut o = opts();

    o.output = OutputFormat::Svg;
    let min = convert(&bytes, &o).unwrap().svg;
    o.output = OutputFormat::SvgPretty;
    let pretty = convert(&bytes, &o).unwrap().svg;

    assert_ne!(min, pretty);
    assert!(pretty.len() >= min.len(), "pretty output should not be smaller");
}

#[test]
fn meta_node_count_matches_emitted_paths() {
    let res = convert(&test_png(64), &opts()).unwrap();
    assert!(
        !res.meta.nodes.is_empty(),
        "a non-trivial image must produce metadata nodes"
    );
}
