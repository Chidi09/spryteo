use super::*;

const KAPPA: f64 = 0.552_284_749_830_793_6;

fn m(x: f64, y: f64) -> PathElement {
    PathElement::MoveTo(x, y)
}
fn l(x: f64, y: f64) -> PathElement {
    PathElement::LineTo(x, y)
}
fn c(x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> PathElement {
    PathElement::CurveTo(x1, y1, x2, y2, x3, y3)
}
fn z() -> PathElement {
    PathElement::ClosePath
}

/// An axis-aligned square, counter-clockwise in SVG's y-down space.
fn square(x: f64, y: f64, side: f64) -> Vec<PathElement> {
    vec![
        m(x, y),
        l(x + side, y),
        l(x + side, y + side),
        l(x, y + side),
        z(),
    ]
}

/// A circle built from the usual four-cubic approximation.
fn circle(cx: f64, cy: f64, r: f64) -> Vec<PathElement> {
    let k = KAPPA * r;
    vec![
        m(cx + r, cy),
        c(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
        c(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
        c(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
        c(cx + k, cy - r, cx + r, cy - k, cx + r, cy),
        z(),
    ]
}

fn close(a: f64, b: f64, eps: f64, what: &str) {
    assert!(
        (a - b).abs() < eps,
        "{what}: expected {b}, got {a} (delta {})",
        (a - b).abs()
    );
}

// ── Baseline: shapes with known closed forms ──────────────────────────

#[test]
fn square_area_and_centroid_are_exact() {
    let g = path_geometry(&square(0.0, 0.0, 10.0));
    close(g.area, 100.0, 1e-9, "area");
    close(g.centroid.0, 5.0, 1e-9, "centroid x");
    close(g.centroid.1, 5.0, 1e-9, "centroid y");
    close(g.bbox.x_min, 0.0, 1e-9, "x_min");
    close(g.bbox.x_max, 10.0, 1e-9, "x_max");
}

/// The four-cubic circle approximation is within ~0.03% of a true circle,
/// so matching π·r² this closely shows the integral is exact for cubics —
/// a chord polygon through the same endpoints would give 2r², a 36% error.
#[test]
fn circle_area_matches_pi_r_squared() {
    let r = 10.0;
    let g = path_geometry(&circle(0.0, 0.0, r));
    let exact = std::f64::consts::PI * r * r;

    close(g.area, exact, exact * 0.001, "circle area");
    close(g.centroid.0, 0.0, 1e-6, "centroid x");
    close(g.centroid.1, 0.0, 1e-6, "centroid y");

    // What the old endpoint-polygon method would have produced.
    let chord_square = 2.0 * r * r;
    assert!(
        (g.area - chord_square).abs() > exact * 0.3,
        "area should not match the endpoint chord polygon"
    );
}

#[test]
fn circle_bbox_is_tight() {
    let g = path_geometry(&circle(5.0, 5.0, 10.0));
    for (got, want, what) in [
        (g.bbox.x_min, -5.0, "x_min"),
        (g.bbox.x_max, 15.0, "x_max"),
        (g.bbox.y_min, -5.0, "y_min"),
        (g.bbox.y_max, 15.0, "y_max"),
    ] {
        close(got, want, 1e-6, what);
    }
}

// ── Curve extrema (#8: "large-control-point curves") ──────────────────

/// A curve whose control points sit far outside its endpoints bulges past
/// them. The bbox must cover the bulge — but must not reach the control
/// points, which the curve never touches.
#[test]
fn bbox_covers_the_bulge_but_not_the_control_points() {
    // Endpoints on y = 0; controls pulled to y = 100.
    let path = vec![m(0.0, 0.0), c(0.0, 100.0, 100.0, 100.0, 100.0, 0.0)];
    let g = path_geometry(&path);

    close(g.bbox.y_min, 0.0, 1e-9, "y_min stays at the endpoints");
    assert!(
        g.bbox.y_max > 1.0,
        "endpoint-only bbox would report y_max = 0; got {}",
        g.bbox.y_max
    );
    assert!(
        g.bbox.y_max < 100.0,
        "the curve never reaches its control points; got {}",
        g.bbox.y_max
    );
    // For this symmetric cubic the peak is at t = 0.5, height 3/4 · 100.
    close(g.bbox.y_max, 75.0, 1e-9, "peak height");
}

/// Extrema outside `(0, 1)` must be ignored — a monotone curve's box is
/// its endpoints.
#[test]
fn monotone_curve_bbox_is_its_endpoints() {
    let path = vec![m(0.0, 0.0), c(1.0, 1.0, 2.0, 2.0, 3.0, 3.0)];
    let g = path_geometry(&path);
    close(g.bbox.x_min, 0.0, 1e-9, "x_min");
    close(g.bbox.x_max, 3.0, 1e-9, "x_max");
    close(g.bbox.y_min, 0.0, 1e-9, "y_min");
    close(g.bbox.y_max, 3.0, 1e-9, "y_max");
}

/// A cubic that degenerates to a straight line must not produce spurious
/// extrema from the near-zero leading coefficient.
#[test]
fn degenerate_cubic_is_handled() {
    let path = vec![m(0.0, 0.0), c(0.0, 0.0, 10.0, 0.0, 10.0, 0.0), z()];
    let g = path_geometry(&path);
    close(g.bbox.x_max, 10.0, 1e-9, "x_max");
    close(g.area, 0.0, 1e-9, "a flat curve encloses nothing");
}

// ── Same endpoints, different curves (#8) ─────────────────────────────

/// The regression the issue calls out: two shapes with identical endpoints
/// but different curvature had identical geometry, because only endpoints
/// were measured.
#[test]
fn curves_with_the_same_endpoints_have_different_areas() {
    let ends = (m(0.0, 0.0), l(0.0, 0.0));
    let shallow = vec![
        ends.0.clone(),
        c(30.0, 10.0, 70.0, 10.0, 100.0, 0.0),
        l(100.0, 0.0),
        z(),
    ];
    let deep = vec![
        ends.0.clone(),
        c(30.0, 90.0, 70.0, 90.0, 100.0, 0.0),
        l(100.0, 0.0),
        z(),
    ];
    let _ = ends.1;

    let a = path_geometry(&shallow);
    let b = path_geometry(&deep);

    assert!(a.area > 0.0, "shallow arc encloses area");
    assert!(
        b.area > a.area * 3.0,
        "a much deeper arc must enclose much more area: {} vs {}",
        b.area,
        a.area
    );
    assert!(
        b.bbox.y_max > a.bbox.y_max * 3.0,
        "and reach much further: {} vs {}",
        b.bbox.y_max,
        a.bbox.y_max
    );
}

/// A curved shape whose endpoints are nearly collinear has near-zero
/// *polygon* area but real enclosed area. It used to be deleted as
/// degenerate.
#[test]
fn a_bulging_shape_with_collinear_endpoints_is_not_degenerate() {
    // Every endpoint is on y = 0; the ink is entirely in the curves.
    let path = vec![
        m(0.0, 0.0),
        c(0.0, 40.0, 100.0, 40.0, 100.0, 0.0),
        c(100.0, -40.0, 0.0, -40.0, 0.0, 0.0),
        z(),
    ];
    let g = path_geometry(&path);
    assert!(
        g.area > 1000.0,
        "a lens shape encloses real area; got {}",
        g.area
    );
}

// ── Subpaths and holes (#8) ───────────────────────────────────────────

#[test]
fn a_hole_is_subtracted() {
    let mut path = square(0.0, 0.0, 10.0);
    path.extend(square(3.0, 3.0, 4.0));

    let g = path_geometry(&path);
    close(g.area, 100.0 - 16.0, 1e-9, "outer minus hole");
}

/// Before subpaths were separated, two rings were joined end-to-end and
/// measured as one polygon, which is neither ring.
#[test]
fn two_disjoint_rings_add_up() {
    let mut path = square(0.0, 0.0, 10.0);
    path.extend(square(50.0, 50.0, 10.0));

    let g = path_geometry(&path);
    close(g.area, 200.0, 1e-9, "two separate squares");
    close(g.centroid.0, 30.0, 1e-9, "centroid between them");
    close(g.centroid.1, 30.0, 1e-9, "centroid between them");
    close(g.bbox.x_max, 60.0, 1e-9, "bbox spans both");
}

/// An off-centre hole must pull the centroid away from it.
#[test]
fn an_asymmetric_hole_shifts_the_centroid() {
    let solid = path_geometry(&square(0.0, 0.0, 100.0));
    close(solid.centroid.0, 50.0, 1e-9, "solid centroid");

    let mut path = square(0.0, 0.0, 100.0);
    path.extend(square(60.0, 40.0, 30.0)); // hole to the right
    let holed = path_geometry(&path);

    close(holed.area, 10_000.0 - 900.0, 1e-9, "area");
    assert!(
        holed.centroid.0 < 49.0,
        "a hole on the right must pull the centroid left; got {}",
        holed.centroid.0
    );
    // The hole is vertically centred, so y should barely move.
    close(holed.centroid.1, 50.0, 1.0, "centroid y");
}

/// Depth 2 is solid again: a ring inside a hole is an island.
#[test]
fn a_nested_island_is_added_back() {
    let mut path = square(0.0, 0.0, 100.0); // depth 0: solid
    path.extend(square(20.0, 20.0, 60.0)); // depth 1: hole
    path.extend(square(40.0, 40.0, 20.0)); // depth 2: island

    let g = path_geometry(&path);
    close(
        g.area,
        10_000.0 - 3_600.0 + 400.0,
        1e-9,
        "solid − hole + island",
    );
}

#[test]
fn three_levels_of_nesting_alternate() {
    let mut path = square(0.0, 0.0, 100.0);
    path.extend(square(10.0, 10.0, 80.0));
    path.extend(square(20.0, 20.0, 60.0));
    path.extend(square(30.0, 30.0, 40.0));

    let g = path_geometry(&path);
    let expected = 10_000.0 - 6_400.0 + 3_600.0 - 1_600.0;
    close(g.area, expected, 1e-9, "alternating nesting");
}

/// Winding direction must not change the result: `evenodd` does not care,
/// and the tracer does not promise a consistent orientation.
#[test]
fn hole_subtraction_ignores_winding_direction() {
    let outer = square(0.0, 0.0, 10.0);

    // Same inner square, traced the other way round.
    let reversed = vec![m(3.0, 3.0), l(3.0, 7.0), l(7.0, 7.0), l(7.0, 3.0), z()];

    let mut a = outer.clone();
    a.extend(square(3.0, 3.0, 4.0));
    let mut b = outer;
    b.extend(reversed);

    close(path_geometry(&a).area, path_geometry(&b).area, 1e-9, "area");
}

// ── Open paths (#8: "open stroke paths") ──────────────────────────────

/// SVG implicitly closes an open subpath when filling it, so the reported
/// area is that of the implicitly-closed contour — here, half the 10×10
/// square. The bbox and centroid stay exact either way, which is what a
/// stroke transform needs.
#[test]
fn an_open_path_is_measured_as_implicitly_closed() {
    let path = vec![m(0.0, 0.0), l(10.0, 0.0), l(10.0, 10.0)];
    let g = path_geometry(&path);

    close(g.area, 50.0, 1e-9, "triangle closed back to the start");
    close(g.bbox.x_max, 10.0, 1e-9, "x_max");
    close(g.bbox.y_max, 10.0, 1e-9, "y_max");
    assert!(g.centroid.0.is_finite() && g.centroid.1.is_finite());
}

/// A stroke that doubles back encloses nothing; it must not acquire a
/// phantom area, and its centroid must fall back to the bbox midpoint.
#[test]
fn a_degenerate_open_stroke_encloses_nothing() {
    let path = vec![m(0.0, 0.0), l(10.0, 0.0), l(0.0, 0.0)];
    let g = path_geometry(&path);

    close(g.area, 0.0, 1e-9, "an out-and-back stroke encloses nothing");
    close(g.bbox.x_max, 10.0, 1e-9, "but still has an extent");
    close(
        g.centroid.0,
        5.0,
        1e-9,
        "centroid falls back to the midpoint",
    );
}

/// An open curved stroke's box must still account for its bulge.
#[test]
fn an_open_curve_bbox_includes_its_extrema() {
    let path = vec![m(0.0, 0.0), c(0.0, 60.0, 100.0, 60.0, 100.0, 0.0)];
    let g = path_geometry(&path);
    close(g.bbox.y_max, 45.0, 1e-9, "3/4 of the control height");
    close(g.bbox.y_min, 0.0, 1e-9, "endpoints stay on the baseline");
    assert!(g.area > 0.0, "the implicit closure encloses the bulge");
}

// ── Degenerate input ──────────────────────────────────────────────────

#[test]
fn empty_path_is_all_zero() {
    let g = path_geometry(&[]);
    close(g.area, 0.0, 1e-12, "area");
    close(g.bbox.x_min, 0.0, 1e-12, "x_min");
    assert!(g.polygon.is_empty());
}

#[test]
fn a_lone_moveto_is_empty() {
    let g = path_geometry(&[m(5.0, 5.0)]);
    close(g.area, 0.0, 1e-12, "area");
}

#[test]
fn a_single_point_subpath_has_zero_area() {
    let g = path_geometry(&[m(5.0, 5.0), l(5.0, 5.0), z()]);
    close(g.area, 0.0, 1e-12, "area");
    close(g.centroid.0, 5.0, 1e-9, "centroid falls back to the extent");
}

#[test]
fn geometry_is_translation_equivariant() {
    let a = path_geometry(&circle(0.0, 0.0, 7.0));
    let b = path_geometry(&circle(1000.0, -500.0, 7.0));

    close(a.area, b.area, 1e-6, "area is translation invariant");
    close(b.centroid.0 - 1000.0, a.centroid.0, 1e-6, "centroid x");
    close(b.centroid.1 + 500.0, a.centroid.1, 1e-6, "centroid y");
}

#[test]
fn geometry_is_deterministic() {
    let path = {
        let mut p = circle(0.0, 0.0, 10.0);
        p.extend(square(-2.0, -2.0, 4.0));
        p
    };
    let first = path_geometry(&path);
    for _ in 0..3 {
        assert_eq!(first, path_geometry(&path));
    }
}

#[test]
fn a_square_perimeter_is_four_sides() {
    let square = vec![
        PathElement::MoveTo(0.0, 0.0),
        PathElement::LineTo(10.0, 0.0),
        PathElement::LineTo(10.0, 10.0),
        PathElement::LineTo(0.0, 10.0),
        PathElement::ClosePath,
    ];
    assert!((path_length(&square) - 40.0).abs() < 1e-9);
}

#[test]
fn an_open_path_is_not_measured_as_closed() {
    // The same three sides, once left open and once closed. Area treats
    // both the same; length must not.
    let open = vec![
        PathElement::MoveTo(0.0, 0.0),
        PathElement::LineTo(10.0, 0.0),
        PathElement::LineTo(10.0, 10.0),
        PathElement::LineTo(0.0, 10.0),
    ];
    let mut closed = open.clone();
    closed.push(PathElement::ClosePath);

    assert!((path_length(&open) - 30.0).abs() < 1e-9);
    assert!((path_length(&closed) - 40.0).abs() < 1e-9);
}

#[test]
fn circle_circumference_matches_two_pi_r() {
    // Four cubics approximating a unit circle, the standard kappa control
    // offset. Quadrature should land on 2*pi to well under a thousandth.
    const K: f64 = 0.552_284_749_831;
    let c = vec![
        PathElement::MoveTo(1.0, 0.0),
        PathElement::CurveTo(1.0, K, K, 1.0, 0.0, 1.0),
        PathElement::CurveTo(-K, 1.0, -1.0, K, -1.0, 0.0),
        PathElement::CurveTo(-1.0, -K, -K, -1.0, 0.0, -1.0),
        PathElement::CurveTo(K, -1.0, 1.0, -K, 1.0, 0.0),
        PathElement::ClosePath,
    ];
    let expected = 2.0 * std::f64::consts::PI;
    assert!(
        (path_length(&c) - expected).abs() < 1e-3,
        "got {}, want {expected}",
        path_length(&c)
    );
}

#[test]
fn length_of_a_straight_cubic_is_its_span() {
    // Control points on the line: the curve is a straight run of length 9.
    let line = vec![
        PathElement::MoveTo(1.0, 0.0),
        PathElement::CurveTo(4.0, 0.0, 7.0, 0.0, 10.0, 0.0),
    ];
    assert!((path_length(&line) - 9.0).abs() < 1e-9);
}

#[test]
fn subpaths_each_contribute_their_own_length() {
    let two = vec![
        PathElement::MoveTo(0.0, 0.0),
        PathElement::LineTo(3.0, 0.0),
        PathElement::MoveTo(10.0, 0.0),
        PathElement::LineTo(10.0, 4.0),
        PathElement::ClosePath,
    ];
    // 3, then 4 out and 4 back.
    assert!((path_length(&two) - 11.0).abs() < 1e-9);
}
