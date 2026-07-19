//! SceneGraph to SVG emission, optimizer, metadata sidecar.

use spryteo_core::ir::{
    Bbox, ConvertResult, CurveSet, Fill, Group, Meta, Node, NodeMeta, PathElement, Primitive, Rgb,
    SceneGraph, Shape, Stats, Stroke, Transform,
};
use spryteo_core::options::{ConvertOptions, Grouping, IdStyle, OutputFormat, Preset, TOrigin};
use spryteo_geom::{dedupe_ids, stable_id};
use std::fmt::Write;

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

fn collect_gradients(scene: &SceneGraph) -> Vec<(String, &Fill)> {
    let mut grads = Vec::new();
    for group in &scene.groups {
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
    }
    grads
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

/// Serializes the scene graph into an SVG string according to conversion options.
fn serialize_svg(
    scene: &SceneGraph,
    width: u32,
    height: u32,
    opts: &ConvertOptions,
    background_rect_color: Option<Rgb>,
) -> String {
    let precision = opts.precision as usize;
    let pretty = matches!(opts.output, OutputFormat::SvgPretty | OutputFormat::Jsx);
    let is_jsx = matches!(opts.output, OutputFormat::Jsx);

    let mut out = String::new();
    let fill_rule_attr = if is_jsx { "fillRule" } else { "fill-rule" };

    if pretty {
        writeln!(out, "<svg viewBox=\"0 0 {} {}\">", width, height).unwrap();
    } else {
        write!(out, "<svg viewBox=\"0 0 {} {}\">", width, height).unwrap();
    }

    let grads = collect_gradients(scene);
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
            }

            match &node.fill {
                Some(Fill::Solid(rgb)) => {
                    write!(
                        node_attrs,
                        " fill=\"#{:02x}{:02x}{:02x}\"",
                        rgb.r, rgb.g, rgb.b
                    )
                    .unwrap();
                }
                Some(Fill::LinearGradient { .. }) | Some(Fill::RadialGradient { .. }) => {
                    let grad_id = format!("grad-{}", node.id);
                    write!(node_attrs, " fill=\"url(#{})\"", escape_html(&grad_id)).unwrap();
                }
                None => {
                    write!(node_attrs, " fill=\"none\"").unwrap();
                }
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
                        // Rotation is ignored for now as a documented limitation.
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

        if use_group {
            let indent = if pretty { "  " } else { "" };
            if pretty {
                writeln!(out, "{}</g>", indent).unwrap();
            } else {
                write!(out, "</g>").unwrap();
            }
        }
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

    for (i, curve) in curves.curves.iter().enumerate() {
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

        let (_, centroid, _) = get_shape_geom(&shape, arcs);

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

    let svg = serialize_svg(scene, width, height, opts, background_rect_color);
    let byte_count = svg.len();

    let stats = Stats {
        node_count: node_index,
        path_count,
        byte_count,
    };

    let meta = Meta {
        nodes: nodes_meta,
        stats,
    };

    ConvertResult { svg, meta }
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
        writeln!(out, "<svg viewBox=\"0 0 {} {}\">", width, height).unwrap();
    } else {
        write!(out, "<svg viewBox=\"0 0 {} {}\">", width, height).unwrap();
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
        if !nodes_with_ids.is_empty() {
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
                PathElement::MoveTo(1.0, 2.0),
                PathElement::LineTo(3.0, 4.0),
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
        assert!(res.svg.contains("d=\"M 1.00 2.00 L 3.00 4.00 Z\""));
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
            ],
            primitive: None,
        };
        let curve_two_2 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(20.0, 0.0),
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
}
