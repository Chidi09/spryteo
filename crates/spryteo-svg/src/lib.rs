//! SceneGraph to SVG emission, optimizer, metadata sidecar.

use spryteo_core::ir::{
    Bbox, ConvertResult, CurveOrigin, CurveSet, Fill, Group, GroupMeta, Meta, Node, NodeMeta,
    PathElement, Primitive, Rgb, SceneGraph, Shape, Stats, Stroke, Transform,
};
use spryteo_core::options::{ConvertOptions, Grouping, IdStyle, OutputFormat, Preset, TOrigin};
use spryteo_geom::{dedupe_ids, stable_id};
use std::fmt::Write;

/// Shapes whose final fitted geometry has area at or below this threshold
/// are degenerate (single-point/collinear slivers that survived raster-level
/// turdsize filtering but collapsed during curve fitting) and are dropped
/// here rather than emitted as zero-area paths. This is deliberately a tiny
/// epsilon, not a user-tunable size filter -- --turdsize is still the real
/// size control; this only catches true degenerate geometry that
/// numerically rounds to ~zero area, not small-but-real shapes.
const DEGENERATE_AREA_EPSILON: f64 = 1e-6;

/// Helper to extract a representative color from a `Fill`.
///
/// For solid fills, this is the solid color itself.
/// For gradient fills, it uses the color of the first gradient stop as a sensible representative color
/// for metadata consumers, keeping the metadata sidecar clean and lightweight.
fn get_representative_color(fill: &Option<Fill>) -> Option<Rgb> {
    match fill {
        Some(Fill::Solid(rgb)) => Some(*rgb),
        Some(Fill::LinearGradient { stops, .. }) | Some(Fill::RadialGradient { stops, .. }) => {
            stops.first().map(|stop| stop.color)
        }
        None => None,
    }
}

/// Computes the bounding box, centroid, and area of a Shape.
///
/// Uses closed-form formulas for primitives and polygon formulas for paths.
fn get_shape_geom(shape: &Shape, arcs: bool) -> (Bbox, (f64, f64), f64) {
    match shape {
        Shape::Primitive(prim) => match prim {
            Primitive::Circle { cx, cy, r } => {
                let bbox = Bbox {
                    x_min: cx - r,
                    x_max: cx + r,
                    y_min: cy - r,
                    y_max: cy + r,
                };
                let centroid = (*cx, *cy);
                let area = std::f64::consts::PI * r * r;
                (bbox, centroid, area)
            }
            Primitive::Ellipse {
                cx,
                cy,
                rx,
                ry,
                rotation,
            } => {
                let theta = *rotation;
                let w_x = (rx * rx * theta.cos().powi(2) + ry * ry * theta.sin().powi(2)).sqrt();
                let w_y = (rx * rx * theta.sin().powi(2) + ry * ry * theta.cos().powi(2)).sqrt();
                let bbox = Bbox {
                    x_min: cx - w_x,
                    x_max: cx + w_x,
                    y_min: cy - w_y,
                    y_max: cy + w_y,
                };
                let centroid = (*cx, *cy);
                let area = std::f64::consts::PI * rx * ry;
                (bbox, centroid, area)
            }
            Primitive::Rect {
                x,
                y,
                width,
                height,
                ..
            } => {
                let bbox = Bbox {
                    x_min: *x,
                    x_max: x + width,
                    y_min: *y,
                    y_max: y + height,
                };
                let centroid = (x + width / 2.0, y + height / 2.0);
                let area = width * height;
                (bbox, centroid, area)
            }
            Primitive::Arc {
                cx,
                cy,
                rx,
                ry,
                rotation,
                ..
            } => {
                if arcs {
                    let theta = *rotation;
                    let w_x =
                        (rx * rx * theta.cos().powi(2) + ry * ry * theta.sin().powi(2)).sqrt();
                    let w_y =
                        (rx * rx * theta.sin().powi(2) + ry * ry * theta.cos().powi(2)).sqrt();
                    let bbox = Bbox {
                        x_min: cx - w_x,
                        x_max: cx + w_x,
                        y_min: cy - w_y,
                        y_max: cy + w_y,
                    };
                    let centroid = (*cx, *cy);
                    let area = std::f64::consts::PI * rx * ry;
                    (bbox, centroid, area)
                } else {
                    // Fallback to empty if it occurs directly
                    let bbox = Bbox {
                        x_min: 0.0,
                        x_max: 0.0,
                        y_min: 0.0,
                        y_max: 0.0,
                    };
                    (bbox, (0.0, 0.0), 0.0)
                }
            }
        },
        Shape::Path(segments) => {
            // Exact: curve extrema for the box, Green's-theorem integrals
            // for area and centroid, subpaths measured separately with
            // holes subtracted (#8).
            let g = spryteo_geom::path_geometry(segments);
            (g.bbox, g.centroid, g.area)
        }
    }
}

/// The coarse polygon used for containment tests between shapes.
///
/// Primitives are sampled from their closed form; paths reuse the
/// flattened outline `path_geometry` already produced, so a curved shape
/// is tested by its real outline rather than by its endpoint hull (#8).
fn containment_polygon(shape: &Shape) -> Vec<(f64, f64)> {
    match shape {
        Shape::Path(segments) => spryteo_geom::path_geometry(segments).polygon,
        Shape::Primitive(prim) => {
            const N: usize = 32;
            // A polygon through N points *on* an ellipse inscribes it: every
            // edge cuts inside the true outline by up to 1 - cos(pi/N) of the
            // radius. That is enough to report a shape as outside the ring it
            // genuinely sits within — traced anti-aliasing rings differ by
            // well under a percent of radius — so the sample radius is scaled
            // out to make the polygon circumscribe instead. Containment then
            // errs towards including a borderline shape rather than
            // fabricating an overlap that the geometry does not have.
            let circumscribe = 1.0 / (std::f64::consts::PI / N as f64).cos();
            let sample = |cx: f64, cy: f64, rx: f64, ry: f64, rot: f64| {
                let (rx, ry) = (rx * circumscribe, ry * circumscribe);
                (0..N)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / N as f64;
                        let (x, y) = (rx * t.cos(), ry * t.sin());
                        (
                            cx + x * rot.cos() - y * rot.sin(),
                            cy + x * rot.sin() + y * rot.cos(),
                        )
                    })
                    .collect()
            };
            match *prim {
                Primitive::Circle { cx, cy, r } => sample(cx, cy, r, r, 0.0),
                Primitive::Ellipse {
                    cx,
                    cy,
                    rx,
                    ry,
                    rotation,
                } => sample(cx, cy, rx, ry, rotation),
                Primitive::Arc {
                    cx,
                    cy,
                    rx,
                    ry,
                    rotation,
                    ..
                } => sample(cx, cy, rx, ry, rotation),
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => vec![
                    (x, y),
                    (x + width, y),
                    (x + width, y + height),
                    (x, y + height),
                ],
            }
        }
    }
}

/// Re-expresses a shape's coordinates relative to a new origin (cx, cy).
fn shift_shape(shape: &mut Shape, cx: f64, cy: f64) {
    match shape {
        Shape::Primitive(prim) => match prim {
            Primitive::Circle {
                cx: pcx, cy: pcy, ..
            } => {
                *pcx -= cx;
                *pcy -= cy;
            }
            Primitive::Ellipse {
                cx: pcx, cy: pcy, ..
            } => {
                *pcx -= cx;
                *pcy -= cy;
            }
            Primitive::Rect { x, y, .. } => {
                *x -= cx;
                *y -= cy;
            }
            Primitive::Arc {
                cx: pcx, cy: pcy, ..
            } => {
                *pcx -= cx;
                *pcy -= cy;
            }
        },
        Shape::Path(segments) => {
            for seg in segments {
                match seg {
                    PathElement::MoveTo(x, y) => {
                        *x -= cx;
                        *y -= cy;
                    }
                    PathElement::LineTo(x, y) => {
                        *x -= cx;
                        *y -= cy;
                    }
                    PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                        *x1 -= cx;
                        *y1 -= cy;
                        *x2 -= cx;
                        *y2 -= cy;
                        *x3 -= cx;
                        *y3 -= cy;
                    }
                    PathElement::ClosePath => {}
                }
            }
        }
    }
}

/// Formats a float coordinate rounded to the given precision, handling negative zero.
fn format_coord(v: f64, precision: usize) -> String {
    let factor = 10_f64.powi(precision as i32);
    let mut rounded = (v * factor).round() / factor;
    if rounded == -0.0 {
        rounded = 0.0;
    }
    format!("{:.1$}", rounded, precision)
}

/// Formats path segments into an SVG path data string using absolute commands.
fn format_path_data(segments: &[PathElement], precision: usize) -> String {
    let mut d = String::new();
    for (idx, seg) in segments.iter().enumerate() {
        if idx > 0 {
            d.push(' ');
        }
        match seg {
            PathElement::MoveTo(x, y) => {
                write!(
                    d,
                    "M {} {}",
                    format_coord(*x, precision),
                    format_coord(*y, precision)
                )
                .unwrap();
            }
            PathElement::LineTo(x, y) => {
                write!(
                    d,
                    "L {} {}",
                    format_coord(*x, precision),
                    format_coord(*y, precision)
                )
                .unwrap();
            }
            PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                write!(
                    d,
                    "C {} {} {} {} {} {}",
                    format_coord(*x1, precision),
                    format_coord(*y1, precision),
                    format_coord(*x2, precision),
                    format_coord(*y2, precision),
                    format_coord(*x3, precision),
                    format_coord(*y3, precision)
                )
                .unwrap();
            }
            PathElement::ClosePath => {
                d.push('Z');
            }
        }
    }
    d
}

/// Escapes HTML special characters in string values to prevent code injection.
fn escape_html(s: &str) -> String {
    let mut escaped = String::new();
    for c in s.chars() {
        match c {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// A gradient that needs a `<defs>` entry, with its final element id already
/// resolved. Fill paints and stroke paints share the defs block but must not
/// share ids — one node can carry both.
struct GradientRef<'a> {
    /// Full element id, already namespaced (`grad-…` or `strokegrad-…`).
    id: String,
    fill: &'a Fill,
    /// The owning node's translate, subtracted from emitted coordinates.
    offset: (f64, f64),
}

/// Collects gradients (both fill and stroke) along with the translate baked
/// into the owning node.
///
/// Gradient coordinates are produced in image space, but a node may be emitted in a
/// centroid-relative local space with a compensating `transform="translate(..)"`. Since
/// `gradientUnits="userSpaceOnUse"` resolves against the element's *pre-transform* space,
/// the translate must be subtracted from the gradient geometry or the ramp lands off the
/// shape and every pixel clamps to a single stop.
fn collect_gradients<'a>(group: &'a Group, grads: &mut Vec<GradientRef<'a>>) {
    for node in &group.nodes {
        if let Some(ref fill) = node.fill {
            match fill {
                Fill::LinearGradient { .. } | Fill::RadialGradient { .. } => {
                    grads.push(GradientRef {
                        id: format!("grad-{}", node.id),
                        fill,
                        offset: (node.transform.translate_x, node.transform.translate_y),
                    });
                }
                _ => {}
            }
        }
        // An empty node id means no `id` attribute is emitted, so a
        // `url(#strokegrad-)` referrer would dangle and the stroke would render
        // as nothing at all. The guard here must stay in lockstep with the one
        // at the `stroke=` write in `serialize_stroke_svg`.
        if let Some(paint) = node.stroke.as_ref().and_then(|s| s.paint.as_ref()) {
            match paint {
                Fill::LinearGradient { .. } | Fill::RadialGradient { .. }
                    if !node.id.is_empty() =>
                {
                    grads.push(GradientRef {
                        id: format!("strokegrad-{}", node.id),
                        fill: paint,
                        offset: (node.transform.translate_x, node.transform.translate_y),
                    });
                }
                _ => {}
            }
        }
    }
    for child in &group.groups {
        collect_gradients(child, grads);
    }
}

fn serialize_defs(grads: &[GradientRef], opts: &ConvertOptions) -> String {
    if grads.is_empty() {
        return String::new();
    }
    let precision = opts.precision as usize;
    let pretty = matches!(opts.output, OutputFormat::SvgPretty | OutputFormat::Jsx);
    let is_jsx = matches!(opts.output, OutputFormat::Jsx);
    let stop_color_attr = if is_jsx { "stopColor" } else { "stop-color" };
    let mut out = String::new();

    let indent_defs = if pretty { "  " } else { "" };
    let indent_grad = if pretty { "    " } else { "" };
    let indent_stop = if pretty { "      " } else { "" };
    let newline = if pretty { "\n" } else { "" };

    write!(out, "{}<defs>{}", indent_defs, newline).unwrap();
    for g in grads {
        let grad_id = &g.id;
        let (tx, ty) = g.offset;
        match g.fill {
            Fill::LinearGradient {
                x1,
                y1,
                x2,
                y2,
                stops,
            } => {
                write!(
                    out,
                    "{}<linearGradient id=\"{}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{}",
                    indent_grad,
                    escape_html(grad_id),
                    format_coord(*x1 - tx, precision),
                    format_coord(*y1 - ty, precision),
                    format_coord(*x2 - tx, precision),
                    format_coord(*y2 - ty, precision),
                    newline
                )
                .unwrap();
                for stop in stops {
                    let offset_pct = (stop.offset * 100.0).round() as i32;
                    write!(
                        out,
                        "{}<stop offset=\"{}%\" {}=\"#{:02x}{:02x}{:02x}\" />{}",
                        indent_stop,
                        offset_pct,
                        stop_color_attr,
                        stop.color.r,
                        stop.color.g,
                        stop.color.b,
                        newline
                    )
                    .unwrap();
                }
                write!(out, "{}</linearGradient>{}", indent_grad, newline).unwrap();
            }
            Fill::RadialGradient { cx, cy, r, stops } => {
                write!(
                    out,
                    "{}<radialGradient id=\"{}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\">{}",
                    indent_grad,
                    escape_html(grad_id),
                    format_coord(*cx - tx, precision),
                    format_coord(*cy - ty, precision),
                    format_coord(*r, precision),
                    newline
                )
                .unwrap();
                for stop in stops {
                    let offset_pct = (stop.offset * 100.0).round() as i32;
                    write!(
                        out,
                        "{}<stop offset=\"{}%\" {}=\"#{:02x}{:02x}{:02x}\" />{}",
                        indent_stop,
                        offset_pct,
                        stop_color_attr,
                        stop.color.r,
                        stop.color.g,
                        stop.color.b,
                        newline
                    )
                    .unwrap();
                }
                write!(out, "{}</radialGradient>{}", indent_grad, newline).unwrap();
            }
            Fill::Solid(_) => {}
        }
    }
    write!(out, "{}</defs>{}", indent_defs, newline).unwrap();
    out
}

fn emit_css_draw_style_block(out: &mut String, nodes_with_ids: &[&Node], pretty: bool) {
    if nodes_with_ids.is_empty() {
        return;
    }
    if pretty {
        writeln!(out, "  <style>").unwrap();
        writeln!(out, "    @keyframes sc-draw {{").unwrap();
        writeln!(out, "      to {{").unwrap();
        writeln!(out, "        stroke-dashoffset: 0;").unwrap();
        writeln!(out, "      }}").unwrap();
        writeln!(out, "    }}").unwrap();
        for (i, node) in nodes_with_ids.iter().enumerate() {
            let delay = i * 100;
            writeln!(out, "    #{} {{", node.id).unwrap();
            writeln!(out, "      stroke-dasharray: 100;").unwrap();
            writeln!(out, "      stroke-dashoffset: 100;").unwrap();
            writeln!(out, "      animation: sc-draw 1s ease forwards;").unwrap();
            writeln!(out, "      animation-delay: {}ms;", delay).unwrap();
            writeln!(out, "    }}").unwrap();
        }
        writeln!(out, "  </style>").unwrap();
    } else {
        write!(
            out,
            "<style>@keyframes sc-draw{{to{{stroke-dashoffset:0;}}}}"
        )
        .unwrap();
        for (i, node) in nodes_with_ids.iter().enumerate() {
            let delay = i * 100;
            write!(
                out,
                "#{} {{stroke-dasharray:100;stroke-dashoffset:100;animation:sc-draw 1s ease forwards;animation-delay:{}ms;}}",
                node.id,
                delay
            )
            .unwrap();
        }
        write!(out, "</style>").unwrap();
    }
}

fn analyze_scene_fills(scene: &SceneGraph) -> (Option<Rgb>, bool) {
    let mut flat_fills = Vec::new();
    let mut has_gradient = false;

    fn traverse_group(group: &Group, flat_fills: &mut Vec<Rgb>, has_gradient: &mut bool) {
        for node in &group.nodes {
            match &node.fill {
                Some(Fill::Solid(rgb)) => {
                    if !flat_fills.contains(rgb) {
                        flat_fills.push(*rgb);
                    }
                }
                Some(Fill::LinearGradient { .. }) | Some(Fill::RadialGradient { .. }) => {
                    *has_gradient = true;
                }
                None => {}
            }
        }
        for child in &group.groups {
            traverse_group(child, flat_fills, has_gradient);
        }
    }

    for group in &scene.groups {
        traverse_group(group, &mut flat_fills, &mut has_gradient);
    }

    if has_gradient {
        (None, false)
    } else if flat_fills.len() == 1 {
        (Some(flat_fills[0]), true)
    } else {
        (None, false)
    }
}

/// Recursively serializes a group and its children (nodes then nested groups)
/// into the output buffer, respecting indentation depth for pretty mode.
#[allow(clippy::too_many_arguments)]
fn serialize_group<'a>(
    out: &mut String,
    group: &'a Group,
    depth: usize,
    precision: usize,
    pretty: bool,
    fill_rule_attr: &str,
    current_color_applied: bool,
    nodes_with_ids: &mut Vec<&'a Node>,
    opts: &ConvertOptions,
) {
    let g_indent = if pretty {
        "  ".repeat(depth + 1)
    } else {
        String::new()
    };
    let mut g_attrs = String::new();
    if !matches!(opts.id_style, IdStyle::None) && !group.id.is_empty() {
        write!(g_attrs, " id=\"{}\"", escape_html(&group.id)).unwrap();
    }
    if pretty {
        writeln!(out, "{}<g{}>", g_indent, g_attrs).unwrap();
    } else {
        write!(out, "<g{}>", g_attrs).unwrap();
    }

    for node in &group.nodes {
        let indent = if pretty {
            "  ".repeat(depth + 2)
        } else {
            String::new()
        };

        let mut node_attrs = String::new();
        if !matches!(opts.id_style, IdStyle::None) && !node.id.is_empty() {
            write!(node_attrs, " id=\"{}\"", escape_html(&node.id)).unwrap();
            nodes_with_ids.push(node);
        }

        match &node.fill {
            Some(Fill::Solid(rgb)) => {
                if current_color_applied {
                    write!(node_attrs, " fill=\"currentColor\"").unwrap();
                } else {
                    write!(
                        node_attrs,
                        " fill=\"#{:02x}{:02x}{:02x}\"",
                        rgb.r, rgb.g, rgb.b
                    )
                    .unwrap();
                }
            }
            Some(Fill::LinearGradient { .. }) | Some(Fill::RadialGradient { .. }) => {
                let grad_id = format!("grad-{}", node.id);
                write!(node_attrs, " fill=\"url(#{})\"", escape_html(&grad_id)).unwrap();
            }
            None => {
                write!(node_attrs, " fill=\"none\"").unwrap();
            }
        }

        if matches!(opts.emit_css, Some(Preset::Draw)) {
            match &node.fill {
                Some(Fill::Solid(rgb)) => {
                    if current_color_applied {
                        write!(node_attrs, " stroke=\"currentColor\"").unwrap();
                    } else {
                        write!(
                            node_attrs,
                            " stroke=\"#{:02x}{:02x}{:02x}\"",
                            rgb.r, rgb.g, rgb.b
                        )
                        .unwrap();
                    }
                }
                Some(Fill::LinearGradient { stops, .. })
                | Some(Fill::RadialGradient { stops, .. }) => {
                    if let Some(stop) = stops.first() {
                        write!(
                            node_attrs,
                            " stroke=\"#{:02x}{:02x}{:02x}\"",
                            stop.color.r, stop.color.g, stop.color.b
                        )
                        .unwrap();
                    }
                }
                None => {}
            }
            write!(
                node_attrs,
                " stroke-width=\"{}\" pathLength=\"100\"",
                format_coord(1.0, precision)
            )
            .unwrap();
        }

        let tx = node.transform.translate_x;
        let ty = node.transform.translate_y;
        if tx != 0.0 || ty != 0.0 {
            write!(
                node_attrs,
                " transform=\"translate({}, {})\"",
                format_coord(tx, precision),
                format_coord(ty, precision)
            )
            .unwrap();
        }

        match &node.shape {
            Shape::Primitive(prim) => match prim {
                Primitive::Circle { cx, cy, r } => {
                    if pretty {
                        writeln!(
                            out,
                            "{}<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{} />",
                            indent,
                            format_coord(*cx, precision),
                            format_coord(*cy, precision),
                            format_coord(*r, precision),
                            node_attrs
                        )
                        .unwrap();
                    } else {
                        write!(
                            out,
                            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
                            format_coord(*cx, precision),
                            format_coord(*cy, precision),
                            format_coord(*r, precision),
                            node_attrs
                        )
                        .unwrap();
                    }
                }
                Primitive::Ellipse {
                    cx,
                    cy,
                    rx,
                    ry,
                    rotation: _,
                } => {
                    let rot_attr = String::new();
                    if pretty {
                        writeln!(
                            out,
                            "{}<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{} />",
                            indent,
                            format_coord(*cx, precision),
                            format_coord(*cy, precision),
                            format_coord(*rx, precision),
                            format_coord(*ry, precision),
                            rot_attr,
                            node_attrs
                        )
                        .unwrap();
                    } else {
                        write!(
                            out,
                            "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{}/>",
                            format_coord(*cx, precision),
                            format_coord(*cy, precision),
                            format_coord(*rx, precision),
                            format_coord(*ry, precision),
                            rot_attr,
                            node_attrs
                        )
                        .unwrap();
                    }
                }
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    rx,
                    ry,
                } => {
                    let mut rx_ry_attrs = String::new();
                    if let Some(val_rx) = rx {
                        write!(rx_ry_attrs, " rx=\"{}\"", format_coord(*val_rx, precision))
                            .unwrap();
                    }
                    if let Some(val_ry) = ry {
                        write!(rx_ry_attrs, " ry=\"{}\"", format_coord(*val_ry, precision))
                            .unwrap();
                    }
                    if pretty {
                        writeln!(
                            out,
                            "{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} />",
                            indent,
                            format_coord(*x, precision),
                            format_coord(*y, precision),
                            format_coord(*width, precision),
                            format_coord(*height, precision),
                            rx_ry_attrs,
                            node_attrs
                        )
                        .unwrap();
                    } else {
                        write!(
                            out,
                            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{}/>",
                            format_coord(*x, precision),
                            format_coord(*y, precision),
                            format_coord(*width, precision),
                            format_coord(*height, precision),
                            rx_ry_attrs,
                            node_attrs
                        )
                        .unwrap();
                    }
                }
                Primitive::Arc {
                    cx,
                    cy,
                    rx,
                    ry,
                    start_angle,
                    end_angle,
                    rotation,
                } => {
                    let cx = *cx;
                    let cy = *cy;
                    let rx = *rx;
                    let ry = *ry;
                    let start_angle = *start_angle;
                    let end_angle = *end_angle;
                    let rot = *rotation;

                    let point_on_ellipse = |angle: f64| -> (f64, f64) {
                        let x_local = rx * angle.cos();
                        let y_local = ry * angle.sin();
                        let x_rot = x_local * rot.cos() - y_local * rot.sin();
                        let y_rot = x_local * rot.sin() + y_local * rot.cos();
                        (cx + x_rot, cy + y_rot)
                    };

                    let (x1, y1) = point_on_ellipse(start_angle);
                    let (x2, y2) = point_on_ellipse(end_angle);

                    let angle_diff = (end_angle - start_angle).abs();
                    let large_arc_flag = if angle_diff > std::f64::consts::PI {
                        1
                    } else {
                        0
                    };
                    let sweep_flag = if end_angle > start_angle { 1 } else { 0 };

                    let d_str = format!(
                        "M {} {} A {} {} {} {} {} {} {}",
                        format_coord(x1, precision),
                        format_coord(y1, precision),
                        format_coord(rx, precision),
                        format_coord(ry, precision),
                        format_coord(rot.to_degrees(), precision),
                        large_arc_flag,
                        sweep_flag,
                        format_coord(x2, precision),
                        format_coord(y2, precision)
                    );

                    if pretty {
                        writeln!(
                            out,
                            "{}<path d=\"{}\" {}=\"evenodd\"{} />",
                            indent, d_str, fill_rule_attr, node_attrs
                        )
                        .unwrap();
                    } else {
                        write!(
                            out,
                            "<path d=\"{}\" {}=\"evenodd\"{}/>",
                            d_str, fill_rule_attr, node_attrs
                        )
                        .unwrap();
                    }
                }
            },
            Shape::Path(segments) => {
                let d_str = format_path_data(segments, precision);
                if pretty {
                    writeln!(
                        out,
                        "{}<path d=\"{}\" {}=\"evenodd\"{} />",
                        indent, d_str, fill_rule_attr, node_attrs
                    )
                    .unwrap();
                } else {
                    write!(
                        out,
                        "<path d=\"{}\" {}=\"evenodd\"{}/>",
                        d_str, fill_rule_attr, node_attrs
                    )
                    .unwrap();
                }
            }
        }
    }

    for child in &group.groups {
        serialize_group(
            out,
            child,
            depth + 1,
            precision,
            pretty,
            fill_rule_attr,
            current_color_applied,
            nodes_with_ids,
            opts,
        );
    }

    if pretty {
        writeln!(out, "{}</g>", g_indent).unwrap();
    } else {
        write!(out, "</g>").unwrap();
    }
}

/// Serializes the scene graph into an SVG string according to conversion options.
fn serialize_svg(
    scene: &SceneGraph,
    width: u32,
    height: u32,
    opts: &ConvertOptions,
    background_rect_color: Option<Rgb>,
    current_color_applied: bool,
) -> String {
    let precision = opts.precision as usize;
    let pretty = matches!(opts.output, OutputFormat::SvgPretty | OutputFormat::Jsx);
    let is_jsx = matches!(opts.output, OutputFormat::Jsx);

    let mut out = String::new();
    let fill_rule_attr = if is_jsx { "fillRule" } else { "fill-rule" };

    if pretty {
        writeln!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">",
            width, height
        )
        .unwrap();
    } else {
        write!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">",
            width, height
        )
        .unwrap();
    }

    let mut grads = Vec::new();
    for group in &scene.groups {
        collect_gradients(group, &mut grads);
    }
    let defs_str = serialize_defs(&grads, opts);
    out.push_str(&defs_str);

    if let Some(color) = background_rect_color {
        if pretty {
            writeln!(
                out,
                "  <rect width=\"{}\" height=\"{}\" fill=\"#{:02x}{:02x}{:02x}\"/>",
                width, height, color.r, color.g, color.b
            )
            .unwrap();
        } else {
            write!(
                out,
                "<rect width=\"{}\" height=\"{}\" fill=\"#{:02x}{:02x}{:02x}\"/>",
                width, height, color.r, color.g, color.b
            )
            .unwrap();
        }
    }

    let mut nodes_with_ids = Vec::new();

    if matches!(opts.grouping, Grouping::Flat) {
        for group in &scene.groups {
            for node in &group.nodes {
                let indent = if pretty { "  " } else { "" };

                let mut node_attrs = String::new();
                if !matches!(opts.id_style, IdStyle::None) && !node.id.is_empty() {
                    write!(node_attrs, " id=\"{}\"", escape_html(&node.id)).unwrap();
                    nodes_with_ids.push(node);
                }

                match &node.fill {
                    Some(Fill::Solid(rgb)) => {
                        if current_color_applied {
                            write!(node_attrs, " fill=\"currentColor\"").unwrap();
                        } else {
                            write!(
                                node_attrs,
                                " fill=\"#{:02x}{:02x}{:02x}\"",
                                rgb.r, rgb.g, rgb.b
                            )
                            .unwrap();
                        }
                    }
                    Some(Fill::LinearGradient { .. }) | Some(Fill::RadialGradient { .. }) => {
                        let grad_id = format!("grad-{}", node.id);
                        write!(node_attrs, " fill=\"url(#{})\"", escape_html(&grad_id)).unwrap();
                    }
                    None => {
                        write!(node_attrs, " fill=\"none\"").unwrap();
                    }
                }

                if matches!(opts.emit_css, Some(Preset::Draw)) {
                    match &node.fill {
                        Some(Fill::Solid(rgb)) => {
                            if current_color_applied {
                                write!(node_attrs, " stroke=\"currentColor\"").unwrap();
                            } else {
                                write!(
                                    node_attrs,
                                    " stroke=\"#{:02x}{:02x}{:02x}\"",
                                    rgb.r, rgb.g, rgb.b
                                )
                                .unwrap();
                            }
                        }
                        Some(Fill::LinearGradient { stops, .. })
                        | Some(Fill::RadialGradient { stops, .. }) => {
                            if let Some(stop) = stops.first() {
                                write!(
                                    node_attrs,
                                    " stroke=\"#{:02x}{:02x}{:02x}\"",
                                    stop.color.r, stop.color.g, stop.color.b
                                )
                                .unwrap();
                            }
                        }
                        None => {}
                    }
                    write!(
                        node_attrs,
                        " stroke-width=\"{}\" pathLength=\"100\"",
                        format_coord(1.0, precision)
                    )
                    .unwrap();
                }

                let tx = node.transform.translate_x;
                let ty = node.transform.translate_y;
                if tx != 0.0 || ty != 0.0 {
                    write!(
                        node_attrs,
                        " transform=\"translate({}, {})\"",
                        format_coord(tx, precision),
                        format_coord(ty, precision)
                    )
                    .unwrap();
                }

                match &node.shape {
                    Shape::Primitive(prim) => match prim {
                        Primitive::Circle { cx, cy, r } => {
                            if pretty {
                                writeln!(
                                    out,
                                    "{}<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{} />",
                                    indent,
                                    format_coord(*cx, precision),
                                    format_coord(*cy, precision),
                                    format_coord(*r, precision),
                                    node_attrs
                                )
                                .unwrap();
                            } else {
                                write!(
                                    out,
                                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
                                    format_coord(*cx, precision),
                                    format_coord(*cy, precision),
                                    format_coord(*r, precision),
                                    node_attrs
                                )
                                .unwrap();
                            }
                        }
                        Primitive::Ellipse {
                            cx,
                            cy,
                            rx,
                            ry,
                            rotation: _,
                        } => {
                            let rot_attr = String::new();
                            if pretty {
                                writeln!(
                                    out,
                                    "{}<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{} />",
                                    indent,
                                    format_coord(*cx, precision),
                                    format_coord(*cy, precision),
                                    format_coord(*rx, precision),
                                    format_coord(*ry, precision),
                                    rot_attr,
                                    node_attrs
                                )
                                .unwrap();
                            } else {
                                write!(
                                    out,
                                    "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{}/>",
                                    format_coord(*cx, precision),
                                    format_coord(*cy, precision),
                                    format_coord(*rx, precision),
                                    format_coord(*ry, precision),
                                    rot_attr,
                                    node_attrs
                                )
                                .unwrap();
                            }
                        }
                        Primitive::Rect {
                            x,
                            y,
                            width,
                            height,
                            rx,
                            ry,
                        } => {
                            let mut rx_ry_attrs = String::new();
                            if let Some(val_rx) = rx {
                                write!(rx_ry_attrs, " rx=\"{}\"", format_coord(*val_rx, precision))
                                    .unwrap();
                            }
                            if let Some(val_ry) = ry {
                                write!(rx_ry_attrs, " ry=\"{}\"", format_coord(*val_ry, precision))
                                    .unwrap();
                            }
                            if pretty {
                                writeln!(
                                    out,
                                    "{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} />",
                                    indent,
                                    format_coord(*x, precision),
                                    format_coord(*y, precision),
                                    format_coord(*width, precision),
                                    format_coord(*height, precision),
                                    rx_ry_attrs,
                                    node_attrs
                                )
                                .unwrap();
                            } else {
                                write!(
                                    out,
                                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{}/>",
                                    format_coord(*x, precision),
                                    format_coord(*y, precision),
                                    format_coord(*width, precision),
                                    format_coord(*height, precision),
                                    rx_ry_attrs,
                                    node_attrs
                                )
                                .unwrap();
                            }
                        }
                        Primitive::Arc {
                            cx,
                            cy,
                            rx,
                            ry,
                            start_angle,
                            end_angle,
                            rotation,
                        } => {
                            let cx = *cx;
                            let cy = *cy;
                            let rx = *rx;
                            let ry = *ry;
                            let start_angle = *start_angle;
                            let end_angle = *end_angle;
                            let rot = *rotation;

                            let point_on_ellipse = |angle: f64| -> (f64, f64) {
                                let x_local = rx * angle.cos();
                                let y_local = ry * angle.sin();
                                let x_rot = x_local * rot.cos() - y_local * rot.sin();
                                let y_rot = x_local * rot.sin() + y_local * rot.cos();
                                (cx + x_rot, cy + y_rot)
                            };

                            let (x1, y1) = point_on_ellipse(start_angle);
                            let (x2, y2) = point_on_ellipse(end_angle);

                            let angle_diff = (end_angle - start_angle).abs();
                            let large_arc_flag = if angle_diff > std::f64::consts::PI {
                                1
                            } else {
                                0
                            };
                            let sweep_flag = if end_angle > start_angle { 1 } else { 0 };

                            let d_str = format!(
                                "M {} {} A {} {} {} {} {} {} {}",
                                format_coord(x1, precision),
                                format_coord(y1, precision),
                                format_coord(rx, precision),
                                format_coord(ry, precision),
                                format_coord(rot.to_degrees(), precision),
                                large_arc_flag,
                                sweep_flag,
                                format_coord(x2, precision),
                                format_coord(y2, precision)
                            );

                            if pretty {
                                writeln!(
                                    out,
                                    "{}<path d=\"{}\" {}=\"evenodd\"{} />",
                                    indent, d_str, fill_rule_attr, node_attrs
                                )
                                .unwrap();
                            } else {
                                write!(
                                    out,
                                    "<path d=\"{}\" {}=\"evenodd\"{}/>",
                                    d_str, fill_rule_attr, node_attrs
                                )
                                .unwrap();
                            }
                        }
                    },
                    Shape::Path(segments) => {
                        let d_str = format_path_data(segments, precision);
                        if pretty {
                            writeln!(
                                out,
                                "{}<path d=\"{}\" {}=\"evenodd\"{} />",
                                indent, d_str, fill_rule_attr, node_attrs
                            )
                            .unwrap();
                        } else {
                            write!(
                                out,
                                "<path d=\"{}\" {}=\"evenodd\"{}/>",
                                d_str, fill_rule_attr, node_attrs
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    } else {
        for group in &scene.groups {
            serialize_group(
                &mut out,
                group,
                0,
                precision,
                pretty,
                fill_rule_attr,
                current_color_applied,
                &mut nodes_with_ids,
                opts,
            );
        }
    }

    if let Some(Preset::Draw) = opts.emit_css {
        emit_css_draw_style_block(&mut out, &nodes_with_ids, pretty);
    }

    if pretty {
        writeln!(out, "</svg>").unwrap();
    } else {
        write!(out, "</svg>").unwrap();
    }

    out
}

/// Builds a `SceneGraph` from a `CurveSet`.
///
/// ## Parameters
/// - `curves`: The input `CurveSet` containing curves to group and convert.
/// - `id_style`: The ID generation style (Hash, Sequential, or None).
/// - `transform_origin`: Controls whether shape coordinates are relative to centroid or absolute.
/// - `fills`: An additional slice of RGB fill colors, one per curve in `curves.curves`.
///   Must be ordered the same way. If missing, the fill is set to `None`.
/// - `arcs`: If false, and a `Primitive::Arc` appears, it will fall back to its raw path representation.
///
/// ## Grouping Simplification
/// Per the Phase 1 roadmap, "connected component" grouping is simplified: we produce
/// one group (`<g>`) per curve. This is because the flattened `CurveSet` does not carry
/// the parent/child nesting relationships needed to group nested holes/contours.
/// Helper function to perform a point-in-polygon test (ray casting).
fn point_in_polygon(point: (f64, f64), polygon: &[(f64, f64)]) -> bool {
    let (px, py) = point;
    let mut inside = false;
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (ix, iy) = polygon[i];
        let (jx, jy) = polygon[j];
        if ((iy > py) != (jy > py)) && (px < (jx - ix) * (py - iy) / (jy - iy) + ix) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn bbox_contains(b: &Bbox, a: &Bbox) -> bool {
    a.x_min >= b.x_min && a.x_max <= b.x_max && a.y_min >= b.y_min && a.y_max <= b.y_max
}

/// True when shape A is strictly inside shape B: A's area must be strictly
/// smaller (the 0.98 factor rejects the near-identical boundaries that
/// stacked layering produces in adjacent layers, which would otherwise
/// report mutual containment) and every sampled boundary vertex of A must
/// fall inside B's outer polygon. Purely geometric — paint-order ties are
/// resolved by keeping the existing (deterministic) shape order instead.
fn is_shape_inside(
    poly_a: &[(f64, f64)],
    bbox_a: &Bbox,
    area_a: f64,
    poly_b: &[(f64, f64)],
    bbox_b: &Bbox,
    area_b: f64,
) -> bool {
    // Strictly smaller by a clear margin. Traced anti-aliasing can leave
    // several rings with the same area to within floating-point noise, and
    // those are coincident outlines rather than nested ones: which contained
    // which would be arbitrary, so neither does.
    if area_a <= 0.0 || area_b <= 0.0 || area_a >= area_b * 0.98 {
        return false;
    }
    if !bbox_contains(bbox_b, bbox_a) {
        return false;
    }
    if poly_a.is_empty() || poly_b.len() < 3 {
        return false;
    }
    poly_a.iter().all(|&p| point_in_polygon(p, poly_b))
}

/// Builds a `SceneGraph` from a `CurveSet`.
///
/// ## Parameters
/// - `curves`: The input `CurveSet` containing curves to group and convert.
/// - `id_style`: The ID generation style (Hash, Sequential, or None).
/// - `transform_origin`: Controls whether shape coordinates are relative to centroid or absolute.
/// - `fills`: An additional slice of RGB fill colors, one per curve in `curves.curves`.
///   Must be ordered the same way. If missing, the fill is set to `None`.
/// - `arcs`: If false, and a `Primitive::Arc` appears, it will fall back to its raw path representation.
///
/// ## Grouping Simplification
/// Per the Phase 1 roadmap, "connected component" grouping is simplified: we produce
/// one group (`<g>`) per curve. This is because the flattened `CurveSet` does not carry
/// the parent/child nesting relationships needed to group nested holes/contours.
pub fn build_scene_graph(
    curves: &CurveSet,
    id_style: &IdStyle,
    transform_origin: &TOrigin,
    fills: &[Fill],
    arcs: bool,
) -> SceneGraph {
    build_scene_graph_with_origins(
        curves,
        id_style,
        transform_origin,
        fills,
        arcs,
        &SceneGrouping::default(),
    )
}

/// The provenance and canvas context needed to build real `<g>` groups (#11).
///
/// Defaults to "no provenance", which reproduces the historical one-group-
/// per-shape scene.
#[derive(Debug, Clone, Default)]
pub struct SceneGrouping<'a> {
    /// One [`CurveOrigin`] per curve, in the same order as `curves.curves`.
    /// Empty disables component grouping.
    pub origins: &'a [CurveOrigin],
    /// Canvas size in pixels, used only to recognise a backdrop.
    pub canvas: Option<(u32, u32)>,
}

/// A component covering at least this fraction of the canvas is treated as a
/// backdrop and never adopts children.
///
/// A background layer's bbox contains everything else on the canvas, so plain
/// containment would nest the entire drawing inside it. That reads as "the
/// artwork is part of the backdrop", and worse, animating the backdrop would
/// drag the artwork with it. A backdrop is scenery: it stays a sibling.
const BACKDROP_COVERAGE: f64 = 0.9;

/// [`build_scene_graph`] with the trace-pipeline provenance needed to build
/// real groups (#11).
///
/// `origins` carries one [`CurveOrigin`] per curve in `curves.curves`, in the
/// same order — the engine builds it from the same `ContourSet` walk that
/// produces `fills`. With it, every top-level traced contour and the islands
/// inside its holes become one `<g>`, and those groups are then nested by
/// geometric containment, so a shape that sits inside another is emitted
/// inside its group.
///
/// Passing an empty slice keeps the older behaviour of one `<g>` per shape,
/// which is what the flat entry point above does.
///
/// Paint order is preserved in both cases: groups appear in the order their
/// first shape is painted, a group's own nodes precede its child groups, and
/// no shape ever moves relative to another.
pub fn build_scene_graph_with_origins(
    curves: &CurveSet,
    id_style: &IdStyle,
    transform_origin: &TOrigin,
    fills: &[Fill],
    arcs: bool,
    grouping: &SceneGrouping<'_>,
) -> SceneGraph {
    let mut groups = Vec::new();

    let mut ids = Vec::new();
    for (i, curve) in curves.curves.iter().enumerate() {
        let fill = fills.get(i).cloned();
        let rep_fill = get_representative_color(&fill);
        let id = match id_style {
            IdStyle::Hash => {
                // Full geometry, not just endpoints, and no paint-array
                // position — see `stable_id`'s contract (#7).
                stable_id(&curve.segments, curve.primitive.as_ref(), rep_fill)
            }
            IdStyle::Sequential => {
                format!("s-{}", i)
            }
            IdStyle::None => "".to_string(),
        };
        ids.push(id);
    }

    if !matches!(id_style, IdStyle::None) {
        dedupe_ids(&mut ids);
    }

    let n = curves.curves.len();
    let mut polys = Vec::with_capacity(n);
    let mut bboxes = Vec::with_capacity(n);
    let mut areas = Vec::with_capacity(n);

    for curve in &curves.curves {
        let is_arc_fallback = match &curve.primitive {
            Some(Primitive::Arc { .. }) => !arcs,
            _ => false,
        };

        let shape = if is_arc_fallback {
            Shape::Path(curve.segments.clone())
        } else {
            match &curve.primitive {
                Some(prim) => Shape::Primitive(prim.clone()),
                None => Shape::Path(curve.segments.clone()),
            }
        };

        // Containment is judged on the shape as emitted — a promoted
        // circle by its circle, a path by its real flattened outline —
        // not by the raw endpoint hull (#8).
        let poly = containment_polygon(&shape);
        let (bbox, _, area) = get_shape_geom(&shape, arcs);

        polys.push(poly);
        bboxes.push(bbox);
        areas.push(area);
    }

    // Containment depth: how many shapes strictly contain this one. Painting
    // in ascending depth order guarantees contained detail (a letter inside a
    // badge inside a background) is never covered by its container — a
    // correctness property no per-layer total order can provide.
    let mut depths = vec![0usize; n];
    for i in 0..n {
        let mut count = 0;
        for j in 0..n {
            if i != j
                && is_shape_inside(
                    &polys[i], &bboxes[i], areas[i], &polys[j], &bboxes[j], areas[j],
                )
            {
                count += 1;
            }
        }
        depths[i] = count;
    }

    // Stable: shapes at equal depth keep their existing quantization order,
    // preserving current behavior (and byte-identical output) for scenes
    // without nesting.
    //
    // This runs whether or not provenance is available. It is not merely a
    // nesting concern: shapes that *nearly* contain one another — the
    // concentric anti-aliasing rings around a traced disc, whose areas sit
    // within a couple of percent — fail the strict containment test used
    // for grouping, so only this sort keeps them painting back-to-front.
    // Grouping is layered on top of the order it produces, never instead
    // of it.
    let mut ordered_indices: Vec<usize> = (0..n).collect();
    ordered_indices.sort_by_key(|&i| depths[i]);

    for &i in &ordered_indices {
        let curve = &curves.curves[i];
        let fill = fills.get(i).cloned();
        let id = ids[i].clone();

        let is_arc_fallback = match &curve.primitive {
            Some(Primitive::Arc { .. }) => !arcs,
            _ => false,
        };

        let mut shape = if is_arc_fallback {
            Shape::Path(curve.segments.clone())
        } else {
            match &curve.primitive {
                Some(prim) => Shape::Primitive(prim.clone()),
                None => Shape::Path(curve.segments.clone()),
            }
        };

        // Degenerate shapes (single-point / collinear slivers) can survive
        // raster-level turdsize filtering and collapse to zero area during
        // curve fitting. Filter them out here by post-fit geometric area,
        // which is a stricter guarantee than raster pixel count.
        if areas[i] <= DEGENERATE_AREA_EPSILON {
            continue;
        }

        let (_bbox, centroid, _) = get_shape_geom(&shape, arcs);

        let transform = match transform_origin {
            TOrigin::Centroid => {
                shift_shape(&mut shape, centroid.0, centroid.1);
                Transform {
                    translate_x: centroid.0,
                    translate_y: centroid.1,
                }
            }
            TOrigin::Baked => Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
        };

        let node = Node {
            id: id.clone(),
            fill,
            stroke: None,
            transform,
            shape,
        };

        groups.push((i, node));
    }

    assemble_groups(groups, grouping, &polys, &bboxes, &areas)
}

/// Turn the painted shapes into the `<g>` tree.
///
/// `painted` holds `(curve index, node)` in final paint order. Without
/// provenance each shape becomes its own group, which is the historical
/// behaviour. With it, shapes are gathered into their traced components and
/// components are nested by containment.
///
/// ## Why this does not disturb the rendering
///
/// A group paints its own nodes before its child groups, so the flattened
/// emission order of the tree is its pre-order traversal — depth-first, while
/// the paint order arriving here is breadth-first (see the containment depth
/// sort in `build_scene_graph_with_origins`). Nesting therefore does move
/// shapes relative to one another, and it is worth being precise about why
/// that is safe.
///
/// Reordering two shapes is only visible when they overlap. Containment
/// nesting pulls a shape ahead only of shapes that do *not* contain it and
/// that it does not contain — and for clean traced geometry, regions are
/// laminar: any two either nest or are disjoint, so those shapes are disjoint
/// and the swap is invisible. The interesting case is the non-laminar one:
/// the concentric anti-aliasing rings around a traced disc genuinely overlap
/// while sitting within a couple of percent of one another's area, so strict
/// containment reports neither inside the other. Such shapes always land at
/// equal containment depth, which makes them siblings here — and sibling
/// order is taken straight from the paint order and never rearranged. The
/// pairs whose order actually matters are exactly the pairs this function
/// leaves alone.
fn assemble_groups(
    painted: Vec<(usize, Node)>,
    grouping: &SceneGrouping<'_>,
    polys: &[Vec<(f64, f64)>],
    bboxes: &[Bbox],
    areas: &[f64],
) -> SceneGraph {
    let origins = grouping.origins;
    let group_id_for = |node: &Node| {
        if node.id.is_empty() {
            String::new()
        } else {
            format!("g-{}", node.id)
        }
    };

    if origins.is_empty() {
        return SceneGraph {
            groups: painted
                .into_iter()
                .map(|(_, node)| Group {
                    id: group_id_for(&node),
                    nodes: vec![node],
                    groups: Vec::new(),
                })
                .collect(),
        };
    }

    // Bucket shapes into components, keeping first-appearance order so the
    // groups paint in the sequence their shapes did.
    let mut order: Vec<CurveOrigin> = Vec::new();
    let mut buckets: Vec<Vec<Node>> = Vec::new();
    let mut opener: Vec<usize> = Vec::new();
    for (i, node) in painted {
        let origin = origins.get(i).copied().unwrap_or(CurveOrigin {
            layer: usize::MAX,
            component: i,
        });
        match order.iter().position(|o| *o == origin) {
            Some(pos) => buckets[pos].push(node),
            None => {
                order.push(origin);
                buckets.push(vec![node]);
                // The component's first shape in paint order is also its
                // shallowest, so it is the contour the rest sit inside — a
                // sounder stand-in for the component than the largest-area
                // member, which can be a stray island.
                opener.push(i);
            }
        }
    }

    let n = buckets.len();

    // Scenery, not a parent: see `BACKDROP_COVERAGE`.
    let canvas_area = grouping
        .canvas
        .map(|(w, h)| f64::from(w) * f64::from(h))
        .filter(|a| *a > 0.0);
    let is_backdrop = |component: usize| match canvas_area {
        Some(canvas) => areas[opener[component]] >= canvas * BACKDROP_COVERAGE,
        None => false,
    };

    // Nest a component under the *last-painted* component that contains it,
    // not the smallest one. Both are containers, so either would read
    // correctly as structure — but a group paints before its children, and
    // the choice therefore fixes where the child lands in the output. A
    // shape contained by several others has to paint after all of them; only
    // the last of them puts it there. Choosing the smallest instead is what
    // slides a traced highlight back underneath the coincident rings that
    // were meant to sit behind it.
    //
    // Containment uses the real flattened outline rather than the bounding
    // box, so a shape merely sharing a box with another is not swallowed by
    // it. No cycle is possible: a container is strictly larger in area, so
    // parent links climb a strict order and must terminate.
    let mut parent: Vec<Option<usize>> = vec![None; n];
    for a in 0..n {
        let ra = opener[a];
        parent[a] = (0..n).rfind(|&b| {
            if b == a || is_backdrop(b) {
                return false;
            }
            let rb = opener[b];
            is_shape_inside(
                &polys[ra],
                &bboxes[ra],
                areas[ra],
                &polys[rb],
                &bboxes[rb],
                areas[rb],
            )
        });
    }

    // Children inherit first-appearance order from the bucket order.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, p) in parent.iter().enumerate() {
        if let Some(p) = *p {
            children[p].push(a);
        }
    }

    fn build(
        idx: usize,
        buckets: &mut [Option<Vec<Node>>],
        children: &[Vec<usize>],
        group_id_for: &dyn Fn(&Node) -> String,
    ) -> Group {
        let nodes = buckets[idx]
            .take()
            .expect("each component is built exactly once");
        let id = nodes.first().map(group_id_for).unwrap_or_default();
        Group {
            id,
            groups: children[idx]
                .iter()
                .map(|&c| build(c, buckets, children, group_id_for))
                .collect(),
            nodes,
        }
    }

    let mut owned: Vec<Option<Vec<Node>>> = buckets.into_iter().map(Some).collect();
    let groups = (0..n)
        .filter(|&a| parent[a].is_none())
        .map(|a| build(a, &mut owned, &children, &group_id_for))
        .collect();

    SceneGraph { groups }
}

/// Walks the `SceneGraph`, emits the SVG string, and builds the `Meta` sidecar.
pub fn emit_svg(
    scene: &SceneGraph,
    width: u32,
    height: u32,
    opts: &ConvertOptions,
    background_rect_color: Option<Rgb>,
) -> ConvertResult {
    let nodes_meta = build_node_metas(scene, opts.arcs);

    let mut path_count = 0;
    for group in &scene.groups {
        count_paths_recursive(group, opts.arcs, &mut path_count);
    }

    let (_target_color, current_color_applied) = if opts.current_color {
        analyze_scene_fills(scene)
    } else {
        (None, false)
    };

    let svg = serialize_svg(
        scene,
        width,
        height,
        opts,
        background_rect_color,
        current_color_applied,
    );
    let byte_count = svg.len();

    let stats = Stats {
        node_count: nodes_meta.len(),
        path_count,
        byte_count,
    };

    let meta = Meta {
        nodes: nodes_meta,
        groups: build_group_metas(scene),
        stats,
        current_color_applied,
    };

    ConvertResult { svg, meta }
}

/// Walks the scene depth-first in document order and builds `NodeMeta` for every node.
///
/// For each group, first its `nodes` are processed, then its `groups` are
/// recursed into.  Every node's `group` field is set to the id of its
/// *immediate* parent group (not always the top-level group).  The
/// `z_order` and `suggested_draw_order` are a running document-order index.
///
/// For a scene with no nested groups this produces the same `Vec<NodeMeta>`
/// as the inline loop that `emit_svg` previously used — same order, same
/// values (including `group` = top-level group id).
pub fn build_node_metas(scene: &SceneGraph, arcs: bool) -> Vec<NodeMeta> {
    fn walk_group(
        group: &Group,
        arcs: bool,
        nodes_meta: &mut Vec<NodeMeta>,
        node_index: &mut usize,
    ) {
        for node in &group.nodes {
            let (rel_bbox, rel_centroid, area) = get_shape_geom(&node.shape, arcs);
            let tx = node.transform.translate_x;
            let ty = node.transform.translate_y;

            let bbox = Bbox {
                x_min: rel_bbox.x_min + tx,
                x_max: rel_bbox.x_max + tx,
                y_min: rel_bbox.y_min + ty,
                y_max: rel_bbox.y_max + ty,
            };

            let centroid = (rel_centroid.0 + tx, rel_centroid.1 + ty);

            nodes_meta.push(NodeMeta {
                id: node.id.clone(),
                bbox,
                centroid,
                area,
                fill: get_representative_color(&node.fill),
                group: group.id.clone(),
                z_order: *node_index,
                suggested_draw_order: *node_index,
            });

            *node_index += 1;
        }
        for child in &group.groups {
            walk_group(child, arcs, nodes_meta, node_index);
        }
    }

    let mut nodes_meta = Vec::new();
    let mut node_index = 0;
    for group in &scene.groups {
        walk_group(group, arcs, &mut nodes_meta, &mut node_index);
    }
    nodes_meta
}

/// Mirror the scene's `<g>` tree into the metadata sidecar (#11).
///
/// Walks the same groups, in the same order, that `serialize_svg` emits, so
/// the tree in `Meta` and the tree in the markup can never disagree.
pub fn build_group_metas(scene: &SceneGraph) -> Vec<GroupMeta> {
    fn walk(group: &Group) -> GroupMeta {
        GroupMeta {
            id: group.id.clone(),
            nodes: group.nodes.iter().map(|n| n.id.clone()).collect(),
            groups: group.groups.iter().map(walk).collect(),
        }
    }
    scene.groups.iter().map(walk).collect()
}

fn count_paths_recursive(group: &Group, arcs: bool, count: &mut usize) {
    for node in &group.nodes {
        let is_path = match &node.shape {
            Shape::Path(_) => true,
            Shape::Primitive(Primitive::Arc { .. }) => !arcs,
            _ => false,
        };
        if is_path {
            *count += 1;
        }
    }
    for child in &group.groups {
        count_paths_recursive(child, arcs, count);
    }
}

/// Builds a `SceneGraph` from a `CurveSet` specifically for stroke-mode.
///
/// ## Grouping Simplification
/// Consistent with `build_scene_graph`, we produce one group (`<g>`) per curve.
/// This maintains consistency across the pipeline and provides animators with
/// a clean per-stroke hierarchy.
///
/// ## Centroid Shifting
/// Centroid-shifting is not performed for stroke mode because open curves do not
/// have a well-defined centroid/interior area like closed shapes. Coordinates are
/// baked directly (equivalent to `TOrigin::Baked`).
pub fn build_stroke_scene_graph(
    curves: &CurveSet,
    id_style: &IdStyle,
    widths: &[f64],
) -> SceneGraph {
    let mut groups = Vec::new();

    let mut ids = Vec::new();
    for (i, curve) in curves.curves.iter().enumerate() {
        let id = match id_style {
            IdStyle::Hash => {
                // Stroke mode emits the raw path, never a primitive, and
                // carries no fill (#7).
                stable_id(&curve.segments, None, None)
            }
            IdStyle::Sequential => {
                format!("s-{}", i)
            }
            IdStyle::None => "".to_string(),
        };
        ids.push(id);
    }

    if !matches!(id_style, IdStyle::None) {
        dedupe_ids(&mut ids);
    }

    for (i, curve) in curves.curves.iter().enumerate() {
        let id = ids[i].clone();
        let width = widths.get(i).copied().unwrap_or(1.0);

        let node = Node {
            id: id.clone(),
            fill: None,
            stroke: Some(Stroke {
                color: Rgb { r: 0, g: 0, b: 0 },
                width,
                paint: None,
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(curve.segments.clone()),
        };

        let group_id = if id.is_empty() {
            "".to_string()
        } else {
            format!("g-{}", id)
        };

        let group = Group {
            id: group_id,
            nodes: vec![node],
            groups: vec![],
        };

        groups.push(group);
    }

    SceneGraph { groups }
}

/// Serializes a stroke-mode scene graph into an SVG string.
///
/// Supports Svg, SvgPretty, and Jsx output formats.
/// If `opts.emit_css` is `Some(Preset::Draw)`, appends a `<style>` block
/// containing keyframe and staggered animation delay rules.
///
/// NOTE: `Preset::Fade` and `Preset::Pop` are currently deferred/out of scope
/// for this centerline-tracing dispatch and are treated as no-ops.
fn serialize_stroke_svg(
    scene: &SceneGraph,
    width: u32,
    height: u32,
    opts: &ConvertOptions,
) -> String {
    let precision = opts.precision as usize;
    let pretty = matches!(opts.output, OutputFormat::SvgPretty | OutputFormat::Jsx);
    let is_jsx = matches!(opts.output, OutputFormat::Jsx);
    let current_color_applied = opts.current_color && analyze_scene_fills(scene).1;

    let mut out = String::new();

    let stroke_width_attr = if is_jsx {
        "strokeWidth"
    } else {
        "stroke-width"
    };
    let stroke_linecap_attr = if is_jsx {
        "strokeLinecap"
    } else {
        "stroke-linecap"
    };
    let stroke_linejoin_attr = if is_jsx {
        "strokeLinejoin"
    } else {
        "stroke-linejoin"
    };

    if pretty {
        writeln!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">",
            width, height
        )
        .unwrap();
    } else {
        write!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">",
            width, height
        )
        .unwrap();
    }

    let mut grads = Vec::new();
    for group in &scene.groups {
        collect_gradients(group, &mut grads);
    }
    // A fill gradient on an id-less node cannot be referenced here: the fill
    // write below falls back to the first stop's solid colour instead of a
    // dangling url, so its "grad-" def would be an orphan. Drop it. (The fill
    // serializer keeps these — its url(#grad-) referrer matches the def.)
    grads.retain(|g| g.id != "grad-");
    let defs_str = serialize_defs(&grads, opts);
    out.push_str(&defs_str);

    let mut nodes_with_ids = Vec::new();

    for group in &scene.groups {
        let use_group = !matches!(opts.grouping, Grouping::Flat);

        if use_group {
            let indent = if pretty { "  " } else { "" };
            let mut g_attrs = String::new();
            if !matches!(opts.id_style, IdStyle::None) && !group.id.is_empty() {
                write!(g_attrs, " id=\"{}\"", escape_html(&group.id)).unwrap();
            }
            if pretty {
                writeln!(out, "{}<g{}>", indent, g_attrs).unwrap();
            } else {
                write!(out, "<g{}>", g_attrs).unwrap();
            }
        }

        for node in &group.nodes {
            let indent = if pretty {
                if use_group {
                    "    "
                } else {
                    "  "
                }
            } else {
                ""
            };

            let mut node_attrs = String::new();
            if !matches!(opts.id_style, IdStyle::None) && !node.id.is_empty() {
                write!(node_attrs, " id=\"{}\"", escape_html(&node.id)).unwrap();
                nodes_with_ids.push(node);
            }

            match &node.fill {
                Some(Fill::Solid(rgb)) => {
                    if current_color_applied {
                        write!(node_attrs, " fill=\"currentColor\"").unwrap();
                    } else {
                        write!(
                            node_attrs,
                            " fill=\"#{:02x}{:02x}{:02x}\"",
                            rgb.r, rgb.g, rgb.b
                        )
                        .unwrap();
                    }
                }
                Some(Fill::LinearGradient { stops, .. })
                | Some(Fill::RadialGradient { stops, .. }) => {
                    if !node.id.is_empty() {
                        let grad_id = format!("grad-{}", node.id);
                        write!(node_attrs, " fill=\"url(#{})\"", escape_html(&grad_id)).unwrap();
                    } else if let Some(stop) = stops.first() {
                        if current_color_applied {
                            write!(node_attrs, " fill=\"currentColor\"").unwrap();
                        } else {
                            write!(
                                node_attrs,
                                " fill=\"#{:02x}{:02x}{:02x}\"",
                                stop.color.r, stop.color.g, stop.color.b
                            )
                            .unwrap();
                        }
                    } else {
                        write!(node_attrs, " fill=\"none\"").unwrap();
                    }
                }
                None => {
                    write!(node_attrs, " fill=\"none\"").unwrap();
                }
            }

            if let Some(ref stroke) = node.stroke {
                let is_gradient_stroke = matches!(
                    stroke.paint,
                    Some(Fill::LinearGradient { .. } | Fill::RadialGradient { .. })
                );
                if !node.id.is_empty() && is_gradient_stroke {
                    write!(
                        node_attrs,
                        " stroke=\"url(#strokegrad-{})\"",
                        escape_html(&node.id)
                    )
                    .unwrap();
                } else {
                    write!(
                        node_attrs,
                        " stroke=\"#{:02x}{:02x}{:02x}\"",
                        stroke.color.r, stroke.color.g, stroke.color.b
                    )
                    .unwrap();
                }
                write!(
                    node_attrs,
                    " {}=\"{}\"",
                    stroke_width_attr,
                    format_coord(stroke.width, precision)
                )
                .unwrap();
            } else {
                write!(node_attrs, " stroke=\"#000000\"").unwrap();
                write!(
                    node_attrs,
                    " {}=\"{}\"",
                    stroke_width_attr,
                    format_coord(1.0, precision)
                )
                .unwrap();
            }

            write!(
                node_attrs,
                " {}=\"round\" {}=\"round\" pathLength=\"100\"",
                stroke_linecap_attr, stroke_linejoin_attr
            )
            .unwrap();

            let tx = node.transform.translate_x;
            let ty = node.transform.translate_y;
            if tx != 0.0 || ty != 0.0 {
                write!(
                    node_attrs,
                    " transform=\"translate({}, {})\"",
                    format_coord(tx, precision),
                    format_coord(ty, precision)
                )
                .unwrap();
            }

            match &node.shape {
                Shape::Primitive(prim) => match prim {
                    Primitive::Circle { cx, cy, r } => {
                        if pretty {
                            writeln!(
                                out,
                                "{}<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{} />",
                                indent,
                                format_coord(*cx, precision),
                                format_coord(*cy, precision),
                                format_coord(*r, precision),
                                node_attrs
                            )
                            .unwrap();
                        } else {
                            write!(
                                out,
                                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
                                format_coord(*cx, precision),
                                format_coord(*cy, precision),
                                format_coord(*r, precision),
                                node_attrs
                            )
                            .unwrap();
                        }
                    }
                    Primitive::Ellipse {
                        cx,
                        cy,
                        rx,
                        ry,
                        rotation: _,
                    } => {
                        let rot_attr = String::new();
                        if pretty {
                            writeln!(
                                out,
                                "{}<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{} />",
                                indent,
                                format_coord(*cx, precision),
                                format_coord(*cy, precision),
                                format_coord(*rx, precision),
                                format_coord(*ry, precision),
                                rot_attr,
                                node_attrs
                            )
                            .unwrap();
                        } else {
                            write!(
                                out,
                                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{}/>",
                                format_coord(*cx, precision),
                                format_coord(*cy, precision),
                                format_coord(*rx, precision),
                                format_coord(*ry, precision),
                                rot_attr,
                                node_attrs
                            )
                            .unwrap();
                        }
                    }
                    Primitive::Rect {
                        x,
                        y,
                        width,
                        height,
                        rx,
                        ry,
                    } => {
                        let mut rx_ry_attrs = String::new();
                        if let Some(val_rx) = rx {
                            write!(rx_ry_attrs, " rx=\"{}\"", format_coord(*val_rx, precision))
                                .unwrap();
                        }
                        if let Some(val_ry) = ry {
                            write!(rx_ry_attrs, " ry=\"{}\"", format_coord(*val_ry, precision))
                                .unwrap();
                        }
                        if pretty {
                            writeln!(
                                out,
                                "{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} />",
                                indent,
                                format_coord(*x, precision),
                                format_coord(*y, precision),
                                format_coord(*width, precision),
                                format_coord(*height, precision),
                                rx_ry_attrs,
                                node_attrs
                            )
                            .unwrap();
                        } else {
                            write!(
                                out,
                                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{}/>",
                                format_coord(*x, precision),
                                format_coord(*y, precision),
                                format_coord(*width, precision),
                                format_coord(*height, precision),
                                rx_ry_attrs,
                                node_attrs
                            )
                            .unwrap();
                        }
                    }
                    Primitive::Arc {
                        cx,
                        cy,
                        rx,
                        ry,
                        start_angle,
                        end_angle,
                        rotation,
                    } => {
                        let cx = *cx;
                        let cy = *cy;
                        let rx = *rx;
                        let ry = *ry;
                        let start_angle = *start_angle;
                        let end_angle = *end_angle;
                        let rot = *rotation;

                        let point_on_ellipse = |angle: f64| -> (f64, f64) {
                            let x_local = rx * angle.cos();
                            let y_local = ry * angle.sin();
                            let x_rot = x_local * rot.cos() - y_local * rot.sin();
                            let y_rot = x_local * rot.sin() + y_local * rot.cos();
                            (cx + x_rot, cy + y_rot)
                        };

                        let (x1, y1) = point_on_ellipse(start_angle);
                        let (x2, y2) = point_on_ellipse(end_angle);

                        let angle_diff = (end_angle - start_angle).abs();
                        let large_arc_flag = if angle_diff > std::f64::consts::PI {
                            1
                        } else {
                            0
                        };
                        let sweep_flag = if end_angle > start_angle { 1 } else { 0 };

                        let d_str = format!(
                            "M {} {} A {} {} {} {} {} {} {}",
                            format_coord(x1, precision),
                            format_coord(y1, precision),
                            format_coord(rx, precision),
                            format_coord(ry, precision),
                            format_coord(rot.to_degrees(), precision),
                            large_arc_flag,
                            sweep_flag,
                            format_coord(x2, precision),
                            format_coord(y2, precision)
                        );

                        if pretty {
                            writeln!(out, "{}<path d=\"{}\"{} />", indent, d_str, node_attrs)
                                .unwrap();
                        } else {
                            write!(out, "<path d=\"{}\"{}/>", d_str, node_attrs).unwrap();
                        }
                    }
                },
                Shape::Path(segments) => {
                    let d_str = format_path_data(segments, precision);
                    if pretty {
                        writeln!(out, "{}<path d=\"{}\"{} />", indent, d_str, node_attrs).unwrap();
                    } else {
                        write!(out, "<path d=\"{}\"{}/>", d_str, node_attrs).unwrap();
                    }
                }
            }
        }

        if use_group {
            let indent = if pretty { "  " } else { "" };
            if pretty {
                writeln!(out, "{}</g>", indent).unwrap();
            } else {
                write!(out, "</g>").unwrap();
            }
        }
    }

    if let Some(Preset::Draw) = opts.emit_css {
        emit_css_draw_style_block(&mut out, &nodes_with_ids, pretty);
    }

    if pretty {
        writeln!(out, "</svg>").unwrap();
    } else {
        write!(out, "</svg>").unwrap();
    }

    out
}

/// Walks the `SceneGraph`, emits the stroke SVG string, and builds the `Meta` sidecar.
pub fn emit_stroke_svg(
    scene: &SceneGraph,
    width: u32,
    height: u32,
    opts: &ConvertOptions,
) -> ConvertResult {
    let mut nodes_meta = Vec::new();
    let mut node_index = 0;
    let mut path_count = 0;

    for group in &scene.groups {
        for node in &group.nodes {
            let (rel_bbox, rel_centroid, area) = get_shape_geom(&node.shape, opts.arcs);

            let tx = node.transform.translate_x;
            let ty = node.transform.translate_y;

            let bbox = Bbox {
                x_min: rel_bbox.x_min + tx,
                x_max: rel_bbox.x_max + tx,
                y_min: rel_bbox.y_min + ty,
                y_max: rel_bbox.y_max + ty,
            };

            let centroid = (rel_centroid.0 + tx, rel_centroid.1 + ty);

            let is_path = match &node.shape {
                Shape::Path(_) => true,
                Shape::Primitive(Primitive::Arc { .. }) => !opts.arcs,
                _ => false,
            };
            if is_path {
                path_count += 1;
            }

            nodes_meta.push(NodeMeta {
                id: node.id.clone(),
                bbox,
                centroid,
                area,
                fill: get_representative_color(&node.fill),
                group: group.id.clone(),
                z_order: node_index,
                suggested_draw_order: node_index,
            });

            node_index += 1;
        }
    }

    let svg = serialize_stroke_svg(scene, width, height, opts);
    let byte_count = svg.len();

    let stats = Stats {
        node_count: node_index,
        path_count,
        byte_count,
    };

    let current_color_applied = opts.current_color && analyze_scene_fills(scene).1;

    let meta = Meta {
        nodes: nodes_meta,
        groups: build_group_metas(scene),
        stats,
        current_color_applied,
    };

    ConvertResult { svg, meta }
}
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
