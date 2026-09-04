use super::*;
use spryteo_core::Rgb;

fn opts() -> ConvertOptions {
    ConvertOptions::default()
}

fn err_msg(o: &ConvertOptions) -> String {
    match validate(o) {
        Err(SpryteoError::InvalidInput(m)) => m,
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn defaults_are_valid() {
    assert!(validate(&opts()).is_ok());
}

#[test]
fn zero_and_one_colors_are_rejected() {
    let mut o = opts();
    o.colors = ColorSpec::N(0);
    assert!(err_msg(&o).contains("at least 2"));
    o.colors = ColorSpec::N(1);
    assert!(err_msg(&o).contains("at least 2"));
}

#[test]
fn color_count_above_the_ceiling_is_rejected() {
    let mut o = opts();
    o.colors = ColorSpec::N(MAX_COLORS + 1);
    assert!(err_msg(&o).contains("at most 64"));
}

#[test]
fn color_count_at_the_bounds_is_accepted() {
    let mut o = opts();
    o.colors = ColorSpec::N(2);
    assert!(validate(&o).is_ok());
    o.colors = ColorSpec::N(MAX_COLORS);
    assert!(validate(&o).is_ok());
}

#[test]
fn empty_palette_is_rejected() {
    let mut o = opts();
    o.colors = ColorSpec::Palette(vec![]);
    assert!(err_msg(&o).contains("at least one colour"));
}

#[test]
fn oversized_palette_is_rejected() {
    let mut o = opts();
    o.colors = ColorSpec::Palette(vec![Rgb { r: 0, g: 0, b: 0 }; MAX_COLORS as usize + 1]);
    assert!(err_msg(&o).contains("at most 64"));
}

#[test]
fn single_colour_palette_is_allowed() {
    // Unlike ColorSpec::N, a one-colour palette is a legitimate request:
    // "render everything in this brand colour".
    let mut o = opts();
    o.colors = ColorSpec::Palette(vec![Rgb { r: 1, g: 2, b: 3 }]);
    assert!(validate(&o).is_ok());
}

#[test]
fn negative_and_nonfinite_tolerance_are_rejected() {
    let mut o = opts();
    o.tolerance = -0.1;
    assert!(err_msg(&o).contains("tolerance"));
    o.tolerance = f32::NAN;
    assert!(err_msg(&o).contains("tolerance"));
    o.tolerance = f32::INFINITY;
    assert!(err_msg(&o).contains("tolerance"));
}

#[test]
fn zero_tolerance_is_allowed() {
    let mut o = opts();
    o.tolerance = 0.0;
    assert!(validate(&o).is_ok(), "0 means 'fit as tightly as possible'");
}

#[test]
fn smoothness_outside_potrace_range_is_rejected() {
    let mut o = opts();
    o.smoothness = -0.01;
    assert!(err_msg(&o).contains("smoothness"));
    o.smoothness = 1.35;
    assert!(err_msg(&o).contains("smoothness"));
    o.smoothness = f32::NAN;
    assert!(err_msg(&o).contains("smoothness"));
}

#[test]
fn smoothness_at_the_bounds_is_accepted() {
    let mut o = opts();
    o.smoothness = 0.0;
    assert!(validate(&o).is_ok());
    o.smoothness = 1.34;
    assert!(validate(&o).is_ok());
}

#[test]
fn absurd_precision_is_rejected() {
    let mut o = opts();
    o.precision = 11;
    assert!(err_msg(&o).contains("precision"));
}

#[test]
fn tiny_max_trace_dimension_is_rejected() {
    let mut o = opts();
    o.max_trace_dimension = Some(0);
    assert!(err_msg(&o).contains("max_trace_dimension"));
    o.max_trace_dimension = Some(MIN_TRACE_DIMENSION - 1);
    assert!(err_msg(&o).contains("max_trace_dimension"));
}

#[test]
fn max_trace_dimension_at_the_floor_is_accepted() {
    let mut o = opts();
    o.max_trace_dimension = Some(MIN_TRACE_DIMENSION);
    assert!(validate(&o).is_ok());
}

#[test]
fn zero_limits_are_rejected() {
    let mut o = opts();
    o.max_pixels = 0;
    assert!(err_msg(&o).contains("max_pixels"));

    let mut o = opts();
    o.max_input_bytes = 0;
    assert!(err_msg(&o).contains("max_input_bytes"));
}

#[test]
fn validation_is_deterministic_for_multiple_violations() {
    // Field order is fixed, so the same bad options always name the same
    // field first — surfaces can rely on the message.
    let mut o = opts();
    o.colors = ColorSpec::N(0);
    o.precision = 200;
    o.tolerance = -5.0;
    for _ in 0..5 {
        assert!(err_msg(&o).contains("colors"));
    }
}
