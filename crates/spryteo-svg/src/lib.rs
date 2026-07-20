//! SceneGraph to SVG emission, optimizer, metadata sidecar.

use spryteo_core::ir::{
    Bbox, ConvertResult, CurveSet, Fill, Group, Meta, Node, NodeMeta, PathElement, Primitive, Rgb,
    SceneGraph, Shape, Stats, Stroke, Transform,
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

/// Extracts the endpoints from a sequence of path elements.
///
/// For a curved node, this flattens its path elements to a point sequence.
/// The endpoint of each segment is sufficient for hashing and geometry calculation.
fn extract_endpoints(segments: &[PathElement]) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for seg in segments {
        match seg {
            PathElement::MoveTo(x, y) => points.push((*x, *y)),
            PathElement::LineTo(x, y) => points.push((*x, *y)),
            PathElement::CurveTo(_, _, _, _, x3, y3) => points.push((*x3, *y3)),
            PathElement::ClosePath => {}
        }
    }
    points
}

/// Computes the axis-aligned bounding box, centroid, and area of a polygon defined by points.
fn get_polygon_geom(points: &[(f64, f64)]) -> (Bbox, (f64, f64), f64) {
    let n = points.len();
    if n == 0 {
        return (
            Bbox {
                x_min: 0.0,
                x_max: 0.0,
                y_min: 0.0,
                y_max: 0.0,
            },
            (0.0, 0.0),
            0.0,
        );
    }

    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    for &(x, y) in points {
        if x < x_min {
            x_min = x;
        }
        if x > x_max {
            x_max = x;
        }
        if y < y_min {
            y_min = y;
        }
        if y > y_max {
            y_max = y;
        }
    }

    let mut area = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let factor = p1.0 * p2.1 - p2.0 * p1.1;
        area += factor;
        cx += (p1.0 + p2.0) * factor;
        cy += (p1.1 + p2.1) * factor;
    }
    area *= 0.5;
    let unsigned_area = area.abs();

    let centroid = if area.abs() > 1e-9 {
        (cx / (6.0 * area), cy / (6.0 * area))
    } else {
        let mut sx = 0.0;
        let mut sy = 0.0;
        for &(x, y) in points {
            sx += x;
            sy += y;
        }
        (sx / n as f64, sy / n as f64)
    };

    (
        Bbox {
            x_min,
            x_max,
            y_min,
            y_max,
        },
        centroid,
        unsigned_area,
    )
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
            let points = extract_endpoints(segments);
            get_polygon_geom(&points)
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

fn collect_gradients<'a>(group: &'a Group, grads: &mut Vec<(String, &'a Fill)>) {
    for node in &group.nodes {
        if let Some(ref fill) = node.fill {
            match fill {
                Fill::LinearGradient { .. } | Fill::RadialGradient { .. } => {
                    grads.push((node.id.clone(), fill));
                }
                _ => {}
            }
        }
    }
    for child in &group.groups {
        collect_gradients(child, grads);
    }
}

fn serialize_defs(grads: &[(String, &Fill)], opts: &ConvertOptions) -> String {
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
    for (node_id, fill) in grads {
        let grad_id = format!("grad-{}", node_id);
        match fill {
            Fill::LinearGradient {
                x1,
                y1,
                x2,
                y2,
                stops,
            } => {
                write!(
                    out,
                    "{}<linearGradient id=\"{}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{}",
                    indent_grad,
                    escape_html(&grad_id),
                    format_coord(*x1, precision),
                    format_coord(*y1, precision),
                    format_coord(*x2, precision),
                    format_coord(*y2, precision),
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
                    "{}<radialGradient id=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\">{}",
                    indent_grad,
                    escape_html(&grad_id),
                    format_coord(*cx, precision),
                    format_coord(*cy, precision),
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
    let mut groups = Vec::new();

    let mut ids = Vec::new();
    for (i, curve) in curves.curves.iter().enumerate() {
        let fill = fills.get(i).cloned();
        let rep_fill = get_representative_color(&fill);
        let id = match id_style {
            IdStyle::Hash => {
                let points = extract_endpoints(&curve.segments);
                stable_id(&points, rep_fill, i)
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

        let poly = extract_endpoints(&curve.segments);
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
                let points = extract_endpoints(&curve.segments);
                stable_id(&points, None, i)
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

            write!(node_attrs, " fill=\"none\"").unwrap();

            if let Some(ref stroke) = node.stroke {
                write!(
                    node_attrs,
                    " stroke=\"#{:02x}{:02x}{:02x}\"",
                    stroke.color.r, stroke.color.g, stroke.color.b
                )
                .unwrap();
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

    let meta = Meta {
        nodes: nodes_meta,
        stats,
        current_color_applied: false,
    };

    ConvertResult { svg, meta }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spryteo_core::ir::{
        Curve, CurveSet, Fill, GradientStop, PathElement, Primitive, Rgb, Shape,
    };
    use spryteo_core::options::{ConvertOptions, IdStyle, OutputFormat, Preset, TOrigin};

    fn make_test_options() -> ConvertOptions {
        ConvertOptions::default()
    }

    fn assert_all_numbers_finite(svg: &str) {
        let mut s = String::new();
        for c in svg.chars() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                s.push(c);
            } else {
                s.push(' ');
            }
        }
        for token in s.split_whitespace() {
            if token == "-" || token == "+" || token == "." || token == "e" || token == "E" {
                continue;
            }
            if let Ok(val) = token.parse::<f64>() {
                assert!(
                    val.is_finite(),
                    "Found non-finite number: {} in token: {}",
                    val,
                    token
                );
            }
        }
    }

    #[test]
    fn test_two_curves_distinct_groups() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(30.0, 20.0),
                PathElement::LineTo(30.0, 30.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(&curves, &IdStyle::Hash, &TOrigin::Centroid, &fills, false);

        assert_eq!(scene.groups.len(), 2);
        assert_eq!(scene.groups[0].nodes.len(), 1);
        assert_eq!(scene.groups[1].nodes.len(), 1);
        assert_ne!(scene.groups[0].nodes[0].id, scene.groups[1].nodes[0].id);

        assert_ne!(scene.groups[0].nodes[0].transform.translate_x, 0.0);
        assert_ne!(scene.groups[0].nodes[0].transform.translate_y, 0.0);
    }

    #[test]
    fn test_emit_primitive_circle() {
        let curve = Curve {
            segments: vec![],
            primitive: Some(Primitive::Circle {
                cx: 50.0,
                cy: 50.0,
                r: 10.0,
            }),
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 0, g: 255, b: 0 })];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(res.svg.contains("<circle"));
        assert!(res.svg.contains("cx=\"50.00\""));
        assert!(res.svg.contains("cy=\"50.00\""));
        assert!(res.svg.contains("r=\"10.00\""));
        assert!(!res.svg.contains("<path"));
        assert_all_numbers_finite(&res.svg);
    }

    #[test]
    fn test_emit_path() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb {
            r: 128,
            g: 128,
            b: 128,
        })];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(res.svg.contains("<path"));
        assert!(res
            .svg
            .contains("d=\"M 0.00 0.00 L 10.00 0.00 L 10.00 10.00 Z\""));
        assert_all_numbers_finite(&res.svg);
    }

    #[test]
    fn test_id_styles() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];

        let scene_hash = build_scene_graph(&curves, &IdStyle::Hash, &TOrigin::Baked, &fills, false);
        let mut opts = make_test_options();
        opts.id_style = IdStyle::Hash;
        let res_hash = emit_svg(&scene_hash, 100, 100, &opts, None);
        assert!(res_hash.svg.contains("id=\"s-"));

        let scene_none = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);
        opts.id_style = IdStyle::None;
        let res_none = emit_svg(&scene_none, 100, 100, &opts, None);
        assert!(!res_none.svg.contains("id="));

        let scene_seq = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );
        opts.id_style = IdStyle::Sequential;
        let res_seq = emit_svg(&scene_seq, 100, 100, &opts, None);
        assert!(res_seq.svg.contains("id=\"s-0\""));
    }

    #[test]
    fn test_precision() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(12.3456, 78.91011),
                PathElement::LineTo(0.0001, -0.0),
                PathElement::LineTo(5.0, 5.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb {
            r: 255,
            g: 255,
            b: 255,
        })];

        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);

        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;
        opts.precision = 1;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(res.svg.contains("12.3"));
        assert!(res.svg.contains("78.9"));
        assert!(res.svg.contains("0.0"));
        assert!(!res.svg.contains("12.35"));
        assert_all_numbers_finite(&res.svg);
    }

    #[test]
    fn test_pretty_vs_minified() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 0, g: 0, b: 0 })];

        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);

        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;

        opts.output = OutputFormat::Svg;
        let res_min = emit_svg(&scene, 100, 100, &opts, None);

        opts.output = OutputFormat::SvgPretty;
        let res_pretty = emit_svg(&scene, 100, 100, &opts, None);

        assert!(res_pretty.svg.contains('\n'));
        assert!(res_min.svg.len() < res_pretty.svg.len());
        assert_all_numbers_finite(&res_min.svg);
        assert_all_numbers_finite(&res_pretty.svg);
    }

    #[test]
    fn test_viewbox() {
        let curves = CurveSet { curves: vec![] };
        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &[], false);

        let opts = make_test_options();
        let res = emit_svg(&scene, 412, 927, &opts, None);
        assert!(res.svg.contains("viewBox=\"0 0 412 927\""));
    }

    #[test]
    fn test_sanitize_injection() {
        let node = Node {
            id: "<script>alert('hack')</script>".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 1.0,
                cy: 1.0,
                r: 1.0,
            }),
        };
        let group = Group {
            id: "<script>alert('group')</script>".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Hash;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(!res.svg.contains("<script>"));
        assert!(res.svg.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_determinism() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(1.23, 4.56),
                PathElement::LineTo(7.89, 0.12),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb {
            r: 255,
            g: 100,
            b: 50,
        })];

        let scene = build_scene_graph(&curves, &IdStyle::Hash, &TOrigin::Centroid, &fills, false);

        let opts = make_test_options();
        let res1 = emit_svg(&scene, 500, 500, &opts, None);
        let res2 = emit_svg(&scene, 500, 500, &opts, None);

        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_stroke_emit_open_path() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let scene = build_stroke_scene_graph(&curves, &IdStyle::Sequential, &[5.432]);
        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;
        opts.precision = 2;

        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = res.svg;

        assert!(svg.contains("<path"), "SVG should contain path element");
        assert!(svg.contains("fill=\"none\""), "fill should be none");
        assert!(
            svg.contains("stroke=\"#000000\""),
            "stroke color should be #000000"
        );
        assert!(
            svg.contains("stroke-width=\"5.43\""),
            "stroke-width should match and be rounded to precision"
        );
        assert!(
            svg.contains("pathLength=\"100\""),
            "pathLength should be 100"
        );

        // Assert open path (d attribute does not end with Z or z)
        assert!(
            svg.contains("d=\"M 10.00 10.00 L 20.00 30.00\""),
            "d attribute should not end with Z/z"
        );
        assert!(!svg.contains('Z'), "should not contain Z");
        assert!(!svg.contains('z'), "should not contain z");
    }

    #[test]
    fn test_stroke_emit_css_draw() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(40.0, 40.0),
                PathElement::LineTo(50.0, 60.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let scene = build_stroke_scene_graph(&curves, &IdStyle::Sequential, &[3.0, 4.0]);

        let mut opts_with_css = make_test_options();
        opts_with_css.id_style = IdStyle::Sequential;
        opts_with_css.emit_css = Some(Preset::Draw);
        opts_with_css.output = OutputFormat::SvgPretty;

        let res_with = emit_stroke_svg(&scene, 100, 100, &opts_with_css);
        assert!(res_with.svg.contains("<style"), "should contain style tag");
        assert!(
            res_with.svg.contains("@keyframes"),
            "should contain keyframes"
        );
        assert!(
            res_with.svg.contains("stroke-dasharray"),
            "should contain stroke-dasharray"
        );

        // Staggered delays check
        assert!(
            res_with.svg.contains("animation-delay: 0ms;"),
            "should contain delay for first node"
        );
        assert!(
            res_with.svg.contains("animation-delay: 100ms;"),
            "should contain delay for second node"
        );

        let mut opts_no_css = make_test_options();
        opts_no_css.id_style = IdStyle::Sequential;
        opts_no_css.emit_css = None;

        let res_without = emit_stroke_svg(&scene, 100, 100, &opts_no_css);
        assert!(
            !res_without.svg.contains("<style"),
            "should not contain style tag"
        );
        assert!(
            !res_without.svg.contains("@keyframes"),
            "should not contain keyframes"
        );
        assert!(
            !res_without.svg.contains("stroke-dasharray"),
            "should not contain stroke-dasharray in CSS"
        );
    }

    #[test]
    fn test_stroke_viewbox_and_id_styles() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let scene_hash = build_stroke_scene_graph(&curves, &IdStyle::Hash, &[2.0]);
        let mut opts = make_test_options();
        opts.id_style = IdStyle::Hash;
        let res_hash = emit_stroke_svg(&scene_hash, 150, 250, &opts);
        assert!(
            res_hash.svg.contains("viewBox=\"0 0 150 250\""),
            "should preserve viewBox dimensions"
        );
        assert!(
            res_hash.svg.contains("id=\"s-"),
            "should contain hash-based ID"
        );

        let scene_none = build_stroke_scene_graph(&curves, &IdStyle::None, &[2.0]);
        opts.id_style = IdStyle::None;
        let res_none = emit_stroke_svg(&scene_none, 150, 250, &opts);
        assert!(!res_none.svg.contains("id="), "should omit ID attribute");
    }

    #[test]
    fn test_stroke_determinism() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(40.0, 40.0),
                PathElement::LineTo(50.0, 60.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let scene = build_stroke_scene_graph(&curves, &IdStyle::Hash, &[3.0, 4.0]);
        let opts = make_test_options();

        let res1 = emit_stroke_svg(&scene, 100, 100, &opts);
        let res2 = emit_stroke_svg(&scene, 100, 100, &opts);

        assert_eq!(res1.svg, res2.svg, "outputs should be byte-identical");
        assert_eq!(res1.meta.stats.byte_count, res2.meta.stats.byte_count);
    }

    #[test]
    fn test_gradient_emission_linear_radial_determinism() {
        // 1. Linear gradient test
        let curve_linear = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let fills_linear = vec![Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        }];
        let curves_linear = CurveSet {
            curves: vec![curve_linear],
        };
        let scene_linear = build_scene_graph(
            &curves_linear,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_linear,
            false,
        );
        let opts = make_test_options();
        let res_linear = emit_svg(&scene_linear, 100, 100, &opts, None);

        assert!(
            res_linear.svg.contains("<defs>"),
            "linear: should contain defs block"
        );
        assert!(res_linear.svg.contains("<linearGradient id=\"grad-s-0\" x1=\"0.00\" y1=\"0.00\" x2=\"10.00\" y2=\"10.00\">"), "linear: should contain linearGradient with correct coordinates");
        assert!(
            res_linear
                .svg
                .contains("<stop offset=\"0%\" stop-color=\"#ff0000\" />"),
            "linear: stop 1"
        );
        assert!(
            res_linear
                .svg
                .contains("<stop offset=\"100%\" stop-color=\"#0000ff\" />"),
            "linear: stop 2"
        );
        assert!(
            res_linear.svg.contains("fill=\"url(#grad-s-0)\""),
            "linear: shape should reference gradient"
        );

        // 2. Radial gradient test
        let curve_radial = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(20.0, 0.0),
                PathElement::LineTo(20.0, 20.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let fills_radial = vec![Fill::RadialGradient {
            cx: 50.0,
            cy: 50.0,
            r: 30.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb {
                        r: 255,
                        g: 255,
                        b: 0,
                    },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb {
                        r: 0,
                        g: 255,
                        b: 255,
                    },
                },
            ],
        }];
        let curves_radial = CurveSet {
            curves: vec![curve_radial],
        };
        let scene_radial = build_scene_graph(
            &curves_radial,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_radial,
            false,
        );
        let res_radial = emit_svg(&scene_radial, 100, 100, &opts, None);

        assert!(
            res_radial.svg.contains("<defs>"),
            "radial: should contain defs block"
        );
        assert!(
            res_radial
                .svg
                .contains("<radialGradient id=\"grad-s-0\" cx=\"50.00\" cy=\"50.00\" r=\"30.00\">"),
            "radial: should contain radialGradient with correct coordinates"
        );
        assert!(
            res_radial
                .svg
                .contains("<stop offset=\"0%\" stop-color=\"#ffff00\" />"),
            "radial: stop 1"
        );
        assert!(
            res_radial
                .svg
                .contains("<stop offset=\"100%\" stop-color=\"#00ffff\" />"),
            "radial: stop 2"
        );
        assert!(
            res_radial.svg.contains("fill=\"url(#grad-s-0)\""),
            "radial: shape should reference gradient"
        );

        // 3. Two different nodes with gradients - check no clash and two distinct definitions
        let curve_two_1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve_two_2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 0.0),
                PathElement::LineTo(30.0, 0.0),
                PathElement::LineTo(30.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves_two = CurveSet {
            curves: vec![curve_two_1, curve_two_2],
        };
        let fills_two = vec![
            Fill::LinearGradient {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Rgb { r: 255, g: 0, b: 0 },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Rgb { r: 0, g: 0, b: 255 },
                    },
                ],
            },
            Fill::RadialGradient {
                cx: 2.0,
                cy: 2.0,
                r: 3.0,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Rgb { r: 0, g: 255, b: 0 },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Rgb {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                    },
                ],
            },
        ];
        let scene_two = build_scene_graph(
            &curves_two,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_two,
            false,
        );
        let res_two = emit_svg(&scene_two, 100, 100, &opts, None);
        assert!(res_two.svg.contains("id=\"grad-s-0\""));
        assert!(res_two.svg.contains("id=\"grad-s-1\""));
        assert!(res_two.svg.contains("fill=\"url(#grad-s-0)\""));
        assert!(res_two.svg.contains("fill=\"url(#grad-s-1)\""));

        // 4. Solid color round-trip (regression guard)
        let curve_solid = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves_solid = CurveSet {
            curves: vec![curve_solid],
        };
        let fills_solid = vec![Fill::Solid(Rgb {
            r: 12,
            g: 34,
            b: 56,
        })];
        let scene_solid = build_scene_graph(
            &curves_solid,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_solid,
            false,
        );
        let res_solid = emit_svg(&scene_solid, 100, 100, &opts, None);
        assert!(
            res_solid.svg.contains("fill=\"#0c2238\""),
            "solid fill serialization regression check"
        );

        // 5. Determinism: emit twice and assert identical output
        let res_two_second = emit_svg(&scene_two, 100, 100, &opts, None);
        assert_eq!(
            res_two.svg, res_two_second.svg,
            "SVG outputs must be byte-identical"
        );
    }

    #[test]
    fn test_current_color_behavior() {
        // 1. Single-color scene + flag -> currentColor and meta flag true
        let curves_single = CurveSet {
            curves: vec![Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                    PathElement::LineTo(10.0, 10.0),
                    PathElement::ClosePath,
                ],
                primitive: None,
            }],
        };
        let fills_single = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];
        let scene_single = build_scene_graph(
            &curves_single,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_single,
            false,
        );
        let mut opts = make_test_options();
        opts.current_color = true;

        let res_single = emit_svg(
            &scene_single,
            100,
            100,
            &opts,
            Some(Rgb { r: 0, g: 255, b: 0 }),
        );
        assert!(res_single.svg.contains("fill=\"currentColor\""));
        // background rect keeps its real color
        assert!(res_single.svg.contains("fill=\"#00ff00\""));
        assert!(res_single.meta.current_color_applied);

        // 2. Two-color scene + flag -> hex fills unchanged and meta flag false
        let curves_two = CurveSet {
            curves: vec![
                Curve {
                    segments: vec![
                        PathElement::MoveTo(0.0, 0.0),
                        PathElement::LineTo(10.0, 0.0),
                        PathElement::LineTo(10.0, 10.0),
                        PathElement::ClosePath,
                    ],
                    primitive: None,
                },
                Curve {
                    segments: vec![
                        PathElement::MoveTo(20.0, 10.0),
                        PathElement::LineTo(30.0, 10.0),
                        PathElement::LineTo(30.0, 20.0),
                        PathElement::ClosePath,
                    ],
                    primitive: None,
                },
            ],
        };
        let fills_two = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];
        let scene_two = build_scene_graph(
            &curves_two,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_two,
            false,
        );
        let res_two = emit_svg(&scene_two, 100, 100, &opts, None);
        assert!(!res_two.svg.contains("fill=\"currentColor\""));
        assert!(res_two.svg.contains("fill=\"#ff0000\""));
        assert!(res_two.svg.contains("fill=\"#0000ff\""));
        assert!(!res_two.meta.current_color_applied);

        // 3. Gradient present + flag -> unchanged/false
        let curves_grad = CurveSet {
            curves: vec![Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                    PathElement::LineTo(10.0, 10.0),
                    PathElement::ClosePath,
                ],
                primitive: None,
            }],
        };
        let fills_grad = vec![Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        }];
        let scene_grad = build_scene_graph(
            &curves_grad,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_grad,
            false,
        );
        let res_grad = emit_svg(&scene_grad, 100, 100, &opts, None);
        assert!(!res_grad.svg.contains("fill=\"currentColor\""));
        assert!(!res_grad.meta.current_color_applied);

        // 4. Flag off -> unchanged/false even for single color
        opts.current_color = false;
        let res_off = emit_svg(&scene_single, 100, 100, &opts, None);
        assert!(!res_off.svg.contains("fill=\"currentColor\""));
        assert!(res_off.svg.contains("fill=\"#ff0000\""));
        assert!(!res_off.meta.current_color_applied);
    }

    #[test]
    fn test_containment_sorting() {
        // (a) Shape inside shape gets higher depth and paints later.
        // We input Inner first, then Outer.
        // Inner: square (20, 20) to (80, 80)
        let inner = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(80.0, 20.0),
                PathElement::LineTo(80.0, 80.0),
                PathElement::LineTo(20.0, 80.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // Outer: square (0, 0) to (100, 100)
        let outer = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(100.0, 0.0),
                PathElement::LineTo(100.0, 100.0),
                PathElement::LineTo(0.0, 100.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };

        let curves = CurveSet {
            curves: vec![inner.clone(), outer.clone()],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }), // Inner fill
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }), // Outer fill
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        // Outer should be painted first (index 0) because it contains Inner.
        // Inner (index 1) should be painted second.
        assert_eq!(scene.groups.len(), 2);
        // We can check their IDs. Inner has original index 0, so its ID is "s-0". Outer has original index 1, ID is "s-1".
        // With reordering, "s-1" (outer) should come first, then "s-0" (inner).
        assert_eq!(scene.groups[0].nodes[0].id, "s-1");
        assert_eq!(scene.groups[1].nodes[0].id, "s-0");

        // (b) Two disjoint shapes keep order.
        // Shape A: (0, 0) to (10, 10)
        let shape_a = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::LineTo(0.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // Shape B: (20, 0) to (30, 10)
        let shape_b = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 0.0),
                PathElement::LineTo(30.0, 0.0),
                PathElement::LineTo(30.0, 10.0),
                PathElement::LineTo(20.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };

        // Order [A, B] -> [A, B]
        let curves_ab = CurveSet {
            curves: vec![shape_a.clone(), shape_b.clone()],
        };
        let scene_ab = build_scene_graph(
            &curves_ab,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_ab.groups[0].nodes[0].id, "s-0");
        assert_eq!(scene_ab.groups[1].nodes[0].id, "s-1");

        // Order [B, A] -> [B, A]
        let curves_ba = CurveSet {
            curves: vec![shape_b.clone(), shape_a.clone()],
        };
        let scene_ba = build_scene_graph(
            &curves_ba,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_ba.groups[0].nodes[0].id, "s-0"); // original index 0 (which was shape_b)
        assert_eq!(scene_ba.groups[1].nodes[0].id, "s-1"); // original index 1 (which was shape_a)

        // (c) Near-identical boundaries keep order.
        // Shape C: (0, 0) to (100, 100) -> area 10000
        let shape_c = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(100.0, 0.0),
                PathElement::LineTo(100.0, 100.0),
                PathElement::LineTo(0.0, 100.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // Shape D: (0.5, 0.5) to (99.5, 99.5) -> area 99 * 99 = 9801.
        // 9801 >= 9800 (which is 10000 * 0.98). So they are near-identical!
        let shape_d = Curve {
            segments: vec![
                PathElement::MoveTo(0.5, 0.5),
                PathElement::LineTo(99.5, 0.5),
                PathElement::LineTo(99.5, 99.5),
                PathElement::LineTo(0.5, 99.5),
                PathElement::ClosePath,
            ],
            primitive: None,
        };

        // Order [C, D] -> [C, D]
        let curves_cd = CurveSet {
            curves: vec![shape_c.clone(), shape_d.clone()],
        };
        let scene_cd = build_scene_graph(
            &curves_cd,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_cd.groups[0].nodes[0].id, "s-0");
        assert_eq!(scene_cd.groups[1].nodes[0].id, "s-1");

        // Order [D, C] -> [D, C]
        let curves_dc = CurveSet {
            curves: vec![shape_d.clone(), shape_c.clone()],
        };
        let scene_dc = build_scene_graph(
            &curves_dc,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_dc.groups[0].nodes[0].id, "s-0");
        assert_eq!(scene_dc.groups[1].nodes[0].id, "s-1");
    }

    #[test]
    fn test_nested_groups_build_node_metas() {
        let inner_node = Node {
            id: "s-inner".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 5.0,
                cy: 5.0,
                r: 3.0,
            }),
        };
        let outer_node = Node {
            id: "s-outer".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 0.0,
                cy: 0.0,
                r: 10.0,
            }),
        };
        let inner_group = Group {
            id: "g-s-inner".to_string(),
            nodes: vec![inner_node],
            groups: vec![],
        };
        let outer_group = Group {
            id: "g-s-outer".to_string(),
            nodes: vec![outer_node],
            groups: vec![inner_group],
        };
        let scene = SceneGraph {
            groups: vec![outer_group],
        };

        let metas = build_node_metas(&scene, false);
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].id, "s-outer");
        assert_eq!(metas[0].group, "g-s-outer");
        assert_eq!(metas[0].z_order, 0);
        assert_eq!(metas[1].id, "s-inner");
        assert_eq!(metas[1].group, "g-s-inner");
        assert_eq!(metas[1].z_order, 1);
    }

    #[test]
    fn test_nested_groups_serialization() {
        let inner_node = Node {
            id: "s-inner".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 5.0,
                cy: 5.0,
                r: 3.0,
            }),
        };
        let outer_node = Node {
            id: "s-outer".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 0.0,
                cy: 0.0,
                r: 10.0,
            }),
        };
        let inner_group = Group {
            id: "g-s-inner".to_string(),
            nodes: vec![inner_node],
            groups: vec![],
        };
        let outer_group = Group {
            id: "g-s-outer".to_string(),
            nodes: vec![outer_node],
            groups: vec![inner_group],
        };
        let scene = SceneGraph {
            groups: vec![outer_group],
        };

        let opts = make_test_options();
        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(svg.contains("<g id=\"g-s-outer\">"));
        assert!(svg.contains("<g id=\"g-s-inner\">"));

        let outer_open = svg.find("<g id=\"g-s-outer\">").unwrap();
        let inner_open = svg.find("<g id=\"g-s-inner\">").unwrap();
        let outer_close = svg.rfind("</svg>").unwrap();

        assert!(
            outer_open < inner_open,
            "outer group must open before inner"
        );
        assert!(
            inner_open < outer_close,
            "inner group must close before svg end"
        );

        let outer_nodes = svg.find("id=\"s-outer\"").unwrap();
        let _inner_nodes = svg.find("id=\"s-inner\"").unwrap();
        assert!(
            outer_open < outer_nodes,
            "outer node after outer group open"
        );
        assert!(outer_nodes < inner_open, "outer node before inner group");
    }

    #[test]
    fn test_nested_group_wrapper_no_nodes() {
        let inner_node = Node {
            id: "s-leaf".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 255, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 2.0,
                cy: 2.0,
                r: 1.0,
            }),
        };
        let leaf_group = Group {
            id: "g-s-leaf".to_string(),
            nodes: vec![inner_node],
            groups: vec![],
        };
        // g-mask wrapper with no nodes, one child group
        let mask_group = Group {
            id: "g-mask-A".to_string(),
            nodes: vec![],
            groups: vec![leaf_group],
        };
        let scene = SceneGraph {
            groups: vec![mask_group],
        };

        let opts = make_test_options();
        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        // mask wrapper must be emitted even though it has no nodes
        assert!(
            svg.contains("<g id=\"g-mask-A\">"),
            "mask wrapper must be present"
        );
        assert!(
            svg.contains("<g id=\"g-s-leaf\">"),
            "leaf group must be nested"
        );

        let mask_open = svg.find("<g id=\"g-mask-A\">").unwrap();
        let leaf_open = svg.find("<g id=\"g-s-leaf\">").unwrap();
        assert!(
            mask_open < leaf_open,
            "mask wrapper opens before leaf group"
        );
    }

    #[test]
    fn test_flat_scene_unchanged_via_build_node_metas() {
        let node1 = Node {
            id: "s-0".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 5.0,
                translate_y: 10.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 0.0,
                cy: 0.0,
                r: 3.0,
            }),
        };
        let node2 = Node {
            id: "s-1".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::ClosePath,
            ]),
        };
        let group1 = Group {
            id: "g-s-0".to_string(),
            nodes: vec![node1],
            groups: vec![],
        };
        let group2 = Group {
            id: "g-s-1".to_string(),
            nodes: vec![node2],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group1, group2],
        };

        let opts = make_test_options();
        let res_old = emit_svg(&scene, 100, 100, &opts, None);

        // Verify meta structure: build_node_metas gives the same result as before
        let metas = build_node_metas(&scene, false);
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].id, "s-0");
        assert_eq!(metas[0].group, "g-s-0");
        assert_eq!(metas[0].z_order, 0);
        assert_eq!(metas[1].id, "s-1");
        assert_eq!(metas[1].group, "g-s-1");
        assert_eq!(metas[1].z_order, 1);

        // Verify SVG output is deterministic
        let res_new = emit_svg(&scene, 100, 100, &opts, None);
        assert_eq!(res_old.svg, res_new.svg);
    }

    #[test]
    fn test_deep_nesting_four_levels() {
        fn make_node(id: &str) -> Node {
            Node {
                id: id.to_string(),
                fill: Some(Fill::Solid(Rgb {
                    r: 128,
                    g: 128,
                    b: 128,
                })),
                stroke: None,
                transform: Transform {
                    translate_x: 0.0,
                    translate_y: 0.0,
                },
                shape: Shape::Primitive(Primitive::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                    rx: None,
                    ry: None,
                }),
            }
        }
        fn make_group(id: &str, node: Node, children: Vec<Group>) -> Group {
            Group {
                id: id.to_string(),
                nodes: vec![node],
                groups: children,
            }
        }

        let d = make_group("g-d", make_node("s-d"), vec![]);
        let c = make_group("g-c", make_node("s-c"), vec![d]);
        let b = make_group("g-b", make_node("s-b"), vec![c]);
        let a = make_group("g-a", make_node("s-a"), vec![b]);
        let scene = SceneGraph { groups: vec![a] };

        let metas = build_node_metas(&scene, false);
        assert_eq!(metas.len(), 4);
        assert_eq!(metas[0].id, "s-a");
        assert_eq!(metas[0].group, "g-a");
        assert_eq!(metas[1].id, "s-b");
        assert_eq!(metas[1].group, "g-b");
        assert_eq!(metas[2].id, "s-c");
        assert_eq!(metas[2].group, "g-c");
        assert_eq!(metas[3].id, "s-d");
        assert_eq!(metas[3].group, "g-d");

        let opts = make_test_options();
        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        // Verify correct nesting depth
        let idx_a_open = svg.find("<g id=\"g-a\">").unwrap();
        let idx_a_close = svg.rfind("</g>").unwrap();
        let idx_b = svg.find("<g id=\"g-b\">").unwrap();
        let idx_c = svg.find("<g id=\"g-c\">").unwrap();
        let idx_d = svg.find("<g id=\"g-d\">").unwrap();
        assert!(
            idx_a_open < idx_b && idx_b < idx_c && idx_c < idx_d,
            "groups must be nested in order a<b<c<d"
        );
        assert!(
            idx_d < idx_a_close,
            "innermost group must close before outermost"
        );
    }

    #[test]
    fn test_fill_emit_css_draw_grouped() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(30.0, 20.0),
                PathElement::LineTo(30.0, 30.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;
        opts.emit_css = Some(Preset::Draw);
        opts.output = OutputFormat::SvgPretty;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(svg.contains("<style>"), "should contain style tag");
        assert!(
            svg.contains("@keyframes sc-draw"),
            "should contain keyframes"
        );
        assert!(
            svg.contains("stroke-dasharray: 100;"),
            "should contain stroke-dasharray"
        );
        assert!(
            svg.contains("pathLength=\"100\""),
            "should contain pathLength"
        );
        assert!(
            svg.contains("animation-delay: 0ms;"),
            "should have delay for first node"
        );
        assert!(
            svg.contains("animation-delay: 100ms;"),
            "should have delay for second node"
        );
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "should preserve original fill"
        );
        assert!(
            svg.contains("fill=\"#0000ff\""),
            "should preserve original fill"
        );
    }

    #[test]
    fn test_fill_emit_css_draw_flat_grouping() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(30.0, 20.0),
                PathElement::LineTo(30.0, 30.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;
        opts.grouping = Grouping::Flat;
        opts.emit_css = Some(Preset::Draw);
        opts.output = OutputFormat::SvgPretty;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(svg.contains("<style>"), "should contain style tag");
        assert!(
            svg.contains("@keyframes sc-draw"),
            "should contain keyframes"
        );
        assert!(
            svg.contains("stroke-dasharray: 100;"),
            "should contain stroke-dasharray"
        );
        assert!(
            svg.contains("pathLength=\"100\""),
            "should contain pathLength"
        );
        assert!(
            svg.contains("animation-delay: 0ms;"),
            "should have delay for first node"
        );
        assert!(
            svg.contains("animation-delay: 100ms;"),
            "should have delay for second node"
        );
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "should preserve original fill"
        );
        assert!(
            svg.contains("fill=\"#0000ff\""),
            "should preserve original fill"
        );
    }

    #[test]
    fn test_fill_emit_css_none_unchanged() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(!svg.contains("<style"), "should NOT contain style tag");
        assert!(!svg.contains("@keyframes"), "should NOT contain keyframes");
        assert!(
            !svg.contains("stroke-dasharray"),
            "should NOT contain stroke-dasharray in CSS"
        );
        assert!(
            !svg.contains("pathLength=\"100\""),
            "should NOT contain pathLength"
        );
        // Should NOT have an added stroke attribute
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "should contain original fill"
        );
    }

    #[test]
    fn test_fill_emit_css_draw_no_ids_is_noop() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];
        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);

        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;
        opts.emit_css = Some(Preset::Draw);

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(
            !svg.contains("<style"),
            "should NOT contain style tag when no ids present"
        );
        assert!(!svg.contains("@keyframes"), "should NOT contain keyframes");
    }

    #[test]
    fn test_build_scene_graph_drops_zero_area_curve() {
        // A normal triangle with area 50.0
        let normal = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // A degenerate curve with area 0.0 (single point + ClosePath)
        let degenerate = Curve {
            segments: vec![PathElement::MoveTo(5.0, 5.0), PathElement::ClosePath],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![normal, degenerate],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        assert_eq!(
            scene.groups.len(),
            1,
            "only the normal curve should survive"
        );
        // The surviving node should be the non-degenerate one (index 0, "s-0")
        assert_eq!(scene.groups[0].nodes[0].id, "s-0");
    }

    #[test]
    fn test_build_scene_graph_keeps_tiny_but_real_area() {
        // A small 2x2 square with area 4.0 (well above epsilon)
        let tiny = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(2.0, 0.0),
                PathElement::LineTo(2.0, 2.0),
                PathElement::LineTo(0.0, 2.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // A normal triangle with area 50.0
        let normal = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 10.0),
                PathElement::LineTo(20.0, 20.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![tiny, normal],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        assert_eq!(
            scene.groups.len(),
            2,
            "both tiny-but-real and normal shapes should survive"
        );
    }

    #[test]
    fn test_build_scene_graph_all_degenerate_produces_empty_scene() {
        let deg1 = Curve {
            segments: vec![PathElement::MoveTo(1.0, 1.0), PathElement::ClosePath],
            primitive: None,
        };
        let deg2 = Curve {
            segments: vec![
                PathElement::MoveTo(2.0, 2.0),
                PathElement::LineTo(3.0, 3.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![deg1, deg2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        assert!(
            scene.groups.is_empty(),
            "all-degenerate input should produce empty scene"
        );
    }
}
