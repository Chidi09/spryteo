use serde::{Deserialize, Serialize};
use spryteo_core::{ConvertOptions, SpryteoError};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SegChoice {
    #[default]
    Auto,
    Chroma,
    Luma,
}

/// Options configuring icon contact sheet extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetOptions {
    pub out_dir: PathBuf,
    pub supersample: u32,    // default 4
    pub canonical_size: f64, // default 24.0
    pub pad: u32,            // default 2
    pub flat: bool,          // emit currentColor instead of a gradient
    pub sat_threshold: Option<u8>,
    pub name_prefix: String, // default "icon"
    pub manifest: Option<PathBuf>,
    pub dry_run: bool,
    pub regularize: bool, // default false
    pub grid_pitch: f64,  // default 1.0
    pub seg: SegChoice,
    pub unify_widths: bool, // default true
}

impl Default for SheetOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::new(),
            supersample: 4,
            canonical_size: 24.0,
            pad: 2,
            flat: false,
            sat_threshold: None,
            name_prefix: "icon".to_string(),
            manifest: None,
            dry_run: false,
            regularize: false,
            grid_pitch: 1.0,
            seg: SegChoice::Auto,
            unify_widths: true,
        }
    }
}

/// Regularization report details for a single icon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconRegularizeReport {
    pub confidence: f32,
    pub max_displacement: f64,
    pub reverted: bool,
    pub welded: usize,
    pub loops_closed: usize,
    pub lines_fitted: usize,
    pub arcs_fitted: usize,
    pub angles_snapped: usize,
    pub points_gridded: usize,
    pub widths_unified: bool,
    pub mirror_axis: Option<f64>,
}

/// Individual report for a single icon extracted from the contact sheet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconReport {
    pub name: String,
    pub col: usize,
    pub row: usize,
    pub source_rect: (u32, u32, u32, u32),
    pub ink_pixels: u32,
    pub path_count: usize,
    pub stroke_width: f64,
    pub filled_components: usize,
    pub gradient_residual: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regularize: Option<IconRegularizeReport>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_rect: Option<(u32, u32, u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_unified: Option<f64>,
}

fn extract_endpoints(segments: &[spryteo_core::PathElement]) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    for seg in segments {
        match seg {
            spryteo_core::PathElement::MoveTo(x, y) | spryteo_core::PathElement::LineTo(x, y) => {
                points.push((*x, *y))
            }
            spryteo_core::PathElement::CurveTo(_, _, _, _, x3, y3) => points.push((*x3, *y3)),
            spryteo_core::PathElement::ClosePath => {}
        }
    }
    points
}

/// Overall report returned by `run_sheet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetReport {
    pub icons: Vec<IconReport>,
    pub cols: usize,
    pub rows: usize,
    pub confidence: f32,
    pub matched: usize,
    pub straddling: usize,
    pub orphans: usize,
    pub empty_cells: usize,
    pub written: usize,
    pub segmentation: String,
    pub polarity: Option<String>,
    pub text_rows_removed: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_stroke_width: Option<f64>,
}

enum Segmentation {
    Chroma,
    Luminance {
        info: spryteo_sheet::LumaInfo,
        text_rows_removed: usize,
    },
}

/// Maps upscaled-crop pixel space to the canonical icon viewBox.
#[derive(Debug, Clone, Copy)]
struct IconTransform {
    scale: f64,
    tx: f64,
    ty: f64,
}

impl IconTransform {
    fn apply_point(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.scale + self.tx, y * self.scale + self.ty)
    }

    /// Lengths (stroke widths, radii) scale but do NOT translate.
    fn apply_length(&self, l: f64) -> f64 {
        l * self.scale
    }
}

pub fn run_sheet(
    bytes: &[u8],
    opts: &ConvertOptions,
    sheet: &SheetOptions,
) -> Result<SheetReport, SpryteoError> {
    // 1. Decode the image ONCE via spryteo_raster::decode. Never decode per icon.
    let image = spryteo_raster::decode(bytes, opts)?;

    // 2. Segmentation selection
    let sat_thresh = sheet
        .sat_threshold
        .unwrap_or_else(|| spryteo_sheet::auto_sat_threshold(&image));
    let chroma_cfg = spryteo_sheet::ChromaConfig {
        sat_min: sat_thresh,
        ..Default::default()
    };
    let luma_cfg = spryteo_sheet::LumaConfig::default();

    let (mask, segmentation, tb_opt) = match sheet.seg {
        SegChoice::Chroma => {
            let mask = spryteo_sheet::chroma_mask(&image, &chroma_cfg);
            if let Err(e) = spryteo_sheet::check_chroma_usable(&mask) {
                return Err(SpryteoError::InvalidInput(format!(
                    "{}: chroma segmentation is not viable for this image",
                    e
                )));
            }
            (mask, Segmentation::Chroma, None)
        }
        SegChoice::Luma => {
            let (raw_mask, info) = spryteo_sheet::luma_mask(&image, &luma_cfg);
            if spryteo_sheet::check_luma_usable(&raw_mask).is_err() {
                return Err(SpryteoError::InvalidInput(
                    "neither chroma nor luminance segmentation is viable for this image"
                        .to_string(),
                ));
            }
            let tb =
                spryteo_sheet::classify_rows(&raw_mask, &spryteo_sheet::TextBandConfig::default());
            let mask = spryteo_sheet::strip_text_rows(&raw_mask, &tb);
            let text_rows_removed = tb.text.len();
            (
                mask,
                Segmentation::Luminance {
                    info,
                    text_rows_removed,
                },
                Some(tb),
            )
        }
        SegChoice::Auto => {
            let c_mask = spryteo_sheet::chroma_mask(&image, &chroma_cfg);
            if spryteo_sheet::check_chroma_usable(&c_mask).is_ok() {
                (c_mask, Segmentation::Chroma, None)
            } else {
                let (raw_mask, info) = spryteo_sheet::luma_mask(&image, &luma_cfg);
                if spryteo_sheet::check_luma_usable(&raw_mask).is_err() {
                    return Err(SpryteoError::InvalidInput(
                        "neither chroma nor luminance segmentation is viable for this image"
                            .to_string(),
                    ));
                }
                let tb = spryteo_sheet::classify_rows(
                    &raw_mask,
                    &spryteo_sheet::TextBandConfig::default(),
                );
                let mask = spryteo_sheet::strip_text_rows(&raw_mask, &tb);
                let text_rows_removed = tb.text.len();
                (
                    mask,
                    Segmentation::Luminance {
                        info,
                        text_rows_removed,
                    },
                    Some(tb),
                )
            }
        }
    };

    // 3. Infer lattice
    let lattice_cfg = spryteo_sheet::LatticeConfig::default();
    let lattice = match spryteo_sheet::infer_lattice(&mask, &lattice_cfg) {
        Some(lat) => lat,
        None => {
            return Err(SpryteoError::InvalidInput(format!(
                "no grid was found in image with dimensions {}x{}",
                image.width, image.height
            )));
        }
    };

    // 4. Clusters and Reconcile
    let cluster_cfg = spryteo_sheet::ClusterConfig::default();
    let clusters = spryteo_sheet::find_clusters(&mask, &cluster_cfg);
    let rec = spryteo_sheet::reconcile(&lattice, &clusters);

    // 5. Extract cells with pad from options
    let cell_cfg = spryteo_sheet::CellConfig {
        pad: sheet.pad,
        ..Default::default()
    };
    let cells = spryteo_sheet::extract_cells(&mask, &lattice, &cell_cfg);

    let widths = spryteo_stroke::component_widths(&mask.bits, mask.width, mask.height);
    let mut max_widths: Vec<f64> = widths.iter().map(|w| w.max_full_width).collect();
    max_widths.sort_by(|a, b| a.total_cmp(b));
    let sheet_stroke_w = if max_widths.is_empty() {
        3.0
    } else {
        max_widths[max_widths.len() / 2]
    };
    let route_t = 1.3 * sheet_stroke_w * sheet.supersample as f64;

    let is_jpeg_input = spryteo_raster::is_jpeg(bytes);
    let min_comp_pixels = if is_jpeg_input {
        6 * (sheet.supersample as usize).pow(2)
    } else {
        0
    };

    if !sheet.dry_run {
        if let Err(e) = std::fs::create_dir_all(&sheet.out_dir) {
            return Err(SpryteoError::Internal(format!(
                "Failed to create output directory '{}': {}",
                sheet.out_dir.display(),
                e
            )));
        }
    }

    struct PendingIcon {
        scene_and_opts: Option<(spryteo_core::ir::SceneGraph, ConvertOptions)>,
        report: IconReport,
    }

    let mut pending_icons = Vec::with_capacity(cells.len());

    // 6. Iterate cells in deterministic order
    for cell in &cells {
        let icon_name = format!("{}-{:02}-{:02}", sheet.name_prefix, cell.col, cell.row);
        let cell_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<(spryteo_core::ir::SceneGraph, ConvertOptions, IconReport), String> {
                // 6a. crop the DECODED IMAGE (not the mask) to cell.crop
                let crop_w = cell.crop.width();
                let crop_h = cell.crop.height();
                let original_crop =
                    spryteo_raster::crop(&image, cell.crop.x1, cell.crop.y1, crop_w, crop_h);
                if original_crop.width == 0 || original_crop.height == 0 {
                    return Err("crop region is empty".to_string());
                }

                // 6b. upscale_bicubic by sheet.supersample
                let upscaled_crop =
                    spryteo_raster::upscale_bicubic(&original_crop, sheet.supersample);

                // 6c. ink_coverage
                let cov = match &segmentation {
                    Segmentation::Chroma => {
                        spryteo_sheet::ink_coverage(&upscaled_crop, sat_thresh, &chroma_cfg)
                    }
                    Segmentation::Luminance { info, .. } => {
                        spryteo_sheet::luma_coverage(&upscaled_crop, info, &luma_cfg)
                    }
                };
                let ink_mask: Vec<bool> = cov.iter().map(|&c| c >= 0.5).collect();

                // 6d. trace_stroke_ex
                let stroke_opts = spryteo_stroke::StrokeOptions {
                    tolerance: opts.tolerance,
                    ink: spryteo_stroke::InkSource::Mask(ink_mask),
                    prune_spurs: false,
                    min_component_pixels: min_comp_pixels,
                    extend_caps: true,
                    route_fill_above: Some(route_t),
                };
                let stroke_ex = spryteo_stroke::trace_stroke_ex(&upscaled_crop, &stroke_opts);

                let fill_curves = if stroke_ex.filled_count > 0 && !stroke_ex.filled_mask.is_empty()
                {
                    let mut layer_mask = vec![0u8; stroke_ex.filled_mask.len()];
                    for i in 0..stroke_ex.filled_mask.len() {
                        if stroke_ex.filled_mask[i] {
                            layer_mask[i] = (cov[i] * 255.0).round() as u8;
                        }
                    }
                    let layer_stack = spryteo_core::ir::LayerStack {
                        layers: vec![spryteo_core::ir::Layer {
                            mask: layer_mask,
                            color: spryteo_core::ir::Rgb { r: 0, g: 0, b: 0 },
                            z_order: 0,
                        }],
                    };
                    let contour_set = spryteo_trace::extract_contours(
                        &layer_stack,
                        upscaled_crop.width,
                        upscaled_crop.height,
                        2, /* turdsize */
                    );
                    spryteo_fit::fit_contours(&contour_set, opts.tolerance, opts.smoothness)
                } else {
                    spryteo_core::ir::CurveSet { curves: Vec::new() }
                };

                // 6e. Apply the transform chain
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                let mut point_count = 0usize;

                for path in &stroke_ex.paths {
                    for seg in &path.curve.segments {
                        match seg {
                            spryteo_core::PathElement::MoveTo(x, y)
                            | spryteo_core::PathElement::LineTo(x, y) => {
                                min_x = min_x.min(*x);
                                max_x = max_x.max(*x);
                                min_y = min_y.min(*y);
                                max_y = max_y.max(*y);
                                point_count += 1;
                            }
                            spryteo_core::PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                                min_x = min_x.min(*x1).min(*x2).min(*x3);
                                max_x = max_x.max(*x1).max(*x2).max(*x3);
                                min_y = min_y.min(*y1).min(*y2).min(*y3);
                                max_y = max_y.max(*y1).max(*y2).max(*y3);
                                point_count += 3;
                            }
                            spryteo_core::PathElement::ClosePath => {}
                        }
                    }
                }

                for curve in &fill_curves.curves {
                    for seg in &curve.segments {
                        match seg {
                            spryteo_core::PathElement::MoveTo(x, y)
                            | spryteo_core::PathElement::LineTo(x, y) => {
                                min_x = min_x.min(*x);
                                max_x = max_x.max(*x);
                                min_y = min_y.min(*y);
                                max_y = max_y.max(*y);
                                point_count += 1;
                            }
                            spryteo_core::PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                                min_x = min_x.min(*x1).min(*x2).min(*x3);
                                max_x = max_x.max(*x1).max(*x2).max(*x3);
                                min_y = min_y.min(*y1).min(*y2).min(*y3);
                                max_y = max_y.max(*y1).max(*y2).max(*y3);
                                point_count += 3;
                            }
                            spryteo_core::PathElement::ClosePath => {}
                        }
                    }
                }

                let bw = max_x - min_x;
                let bh = max_y - min_y;
                let max_dim = bw.max(bh);
                if point_count == 0 || max_dim <= 0.0 {
                    return Err("traced geometry has zero dimension".to_string());
                }

                let s = sheet.canonical_size;
                let scale = s / max_dim;
                let tx = -min_x * scale + (s - bw * scale) / 2.0;
                let ty = -min_y * scale + (s - bh * scale) / 2.0;
                let transform = IconTransform { scale, tx, ty };

                let mut transformed_curves = Vec::with_capacity(stroke_ex.paths.len());
                let mut transformed_widths = Vec::with_capacity(stroke_ex.paths.len());

                for path in &stroke_ex.paths {
                    let mut new_segs = Vec::with_capacity(path.curve.segments.len());
                    for seg in &path.curve.segments {
                        let new_seg = match seg {
                            spryteo_core::PathElement::MoveTo(x, y) => {
                                let (nx, ny) = transform.apply_point(*x, *y);
                                spryteo_core::PathElement::MoveTo(nx, ny)
                            }
                            spryteo_core::PathElement::LineTo(x, y) => {
                                let (nx, ny) = transform.apply_point(*x, *y);
                                spryteo_core::PathElement::LineTo(nx, ny)
                            }
                            spryteo_core::PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                                let (nx1, ny1) = transform.apply_point(*x1, *y1);
                                let (nx2, ny2) = transform.apply_point(*x2, *y2);
                                let (nx3, ny3) = transform.apply_point(*x3, *y3);
                                spryteo_core::PathElement::CurveTo(nx1, ny1, nx2, ny2, nx3, ny3)
                            }
                            spryteo_core::PathElement::ClosePath => {
                                spryteo_core::PathElement::ClosePath
                            }
                        };
                        new_segs.push(new_seg);
                    }
                    transformed_curves.push(spryteo_core::Curve {
                        segments: new_segs,
                        primitive: path.curve.primitive.clone(),
                    });
                    transformed_widths.push(transform.apply_length(path.width));
                }

                let mut transformed_fill_curves = Vec::with_capacity(fill_curves.curves.len());
                for curve in &fill_curves.curves {
                    let mut new_segs = Vec::with_capacity(curve.segments.len());
                    for seg in &curve.segments {
                        let new_seg = match seg {
                            spryteo_core::PathElement::MoveTo(x, y) => {
                                let (nx, ny) = transform.apply_point(*x, *y);
                                spryteo_core::PathElement::MoveTo(nx, ny)
                            }
                            spryteo_core::PathElement::LineTo(x, y) => {
                                let (nx, ny) = transform.apply_point(*x, *y);
                                spryteo_core::PathElement::LineTo(nx, ny)
                            }
                            spryteo_core::PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                                let (nx1, ny1) = transform.apply_point(*x1, *y1);
                                let (nx2, ny2) = transform.apply_point(*x2, *y2);
                                let (nx3, ny3) = transform.apply_point(*x3, *y3);
                                spryteo_core::PathElement::CurveTo(nx1, ny1, nx2, ny2, nx3, ny3)
                            }
                            spryteo_core::PathElement::ClosePath => {
                                spryteo_core::PathElement::ClosePath
                            }
                        };
                        new_segs.push(new_seg);
                    }
                    transformed_fill_curves.push(spryteo_core::Curve {
                        segments: new_segs,
                        primitive: curve.primitive.clone(),
                    });
                }

                let mut icon_warnings = Vec::new();
                let mut reg_info = None;

                if sheet.regularize {
                    let unreg_curves = transformed_curves.clone();
                    let unreg_widths = transformed_widths.clone();

                    let cfg = spryteo_geom::regularize::RegularizeConfig {
                        grid_pitch: sheet.grid_pitch,
                        ..Default::default()
                    };

                    let report = spryteo_geom::regularize::regularize(
                        &mut transformed_curves,
                        &mut transformed_widths,
                        &cfg,
                    );

                    let reverted = if report.confidence < 0.5 {
                        transformed_curves = unreg_curves;
                        transformed_widths = unreg_widths;
                        icon_warnings.push(format!(
                            "regularization reverted due to low confidence ({:.2})",
                            report.confidence
                        ));
                        true
                    } else {
                        false
                    };

                    reg_info = Some(IconRegularizeReport {
                        confidence: report.confidence,
                        max_displacement: report.max_displacement,
                        reverted,
                        welded: report.welded,
                        loops_closed: report.loops_closed,
                        lines_fitted: report.lines_fitted,
                        arcs_fitted: report.arcs_fitted,
                        angles_snapped: report.angles_snapped,
                        points_gridded: report.points_gridded,
                        widths_unified: report.widths_unified,
                        mirror_axis: report.mirror_axis,
                    });
                }

                let curve_set = spryteo_core::CurveSet {
                    curves: transformed_curves,
                };

                let mut gradient_residual = None;
                let color_cfg = spryteo_sheet::ColorConfig::default();

                let (stroke_color, stroke_paint) =
                    if sheet.flat {
                        (spryteo_core::Rgb { r: 0, g: 0, b: 0 }, None)
                    } else {
                        match &segmentation {
                            Segmentation::Chroma => {
                                let crop_mask =
                                    spryteo_sheet::chroma_mask(&original_crop, &chroma_cfg);
                                let crop_rect = spryteo_sheet::Bbox {
                                    x1: 0,
                                    y1: 0,
                                    x2: original_crop.width,
                                    y2: original_crop.height,
                                };
                                match spryteo_sheet::fit_linear_gradient(
                                    &original_crop,
                                    &crop_mask,
                                    &crop_rect,
                                    &color_cfg,
                                ) {
                                    Some(fit) => {
                                        gradient_residual = Some(fit.residual);
                                        // GRADIENT COORDINATE SPACE: fit_linear_gradient is run on the ORIGINAL (non-upscaled) crop,
                                        // so its coordinates are in ORIGINAL-crop space, while the transform expects UPSCALED-crop space.
                                        // Multiply the fitted gradient endpoints by supersample BEFORE passing them through apply_point.
                                        let s_factor = sheet.supersample as f64;
                                        let (gx1, gy1) = transform
                                            .apply_point(fit.x1 * s_factor, fit.y1 * s_factor);
                                        let (gx2, gy2) = transform
                                            .apply_point(fit.x2 * s_factor, fit.y2 * s_factor);

                                        let stops = vec![
                                            spryteo_core::GradientStop {
                                                offset: 0.0,
                                                color: fit.start,
                                            },
                                            spryteo_core::GradientStop {
                                                offset: 1.0,
                                                color: fit.end,
                                            },
                                        ];
                                        (
                                            fit.start,
                                            Some(spryteo_core::Fill::LinearGradient {
                                                x1: gx1,
                                                y1: gy1,
                                                x2: gx2,
                                                y2: gy2,
                                                stops,
                                            }),
                                        )
                                    }
                                    None => {
                                        icon_warnings.push(
                                        "linear gradient fit failed, fell back to dominant color"
                                            .to_string(),
                                    );
                                        let dom = spryteo_sheet::dominant_color(
                                            &original_crop,
                                            &crop_mask,
                                            &crop_rect,
                                            color_cfg.core_sat_min,
                                        )
                                        .unwrap_or(spryteo_core::Rgb { r: 0, g: 0, b: 0 });
                                        (dom, None)
                                    }
                                }
                            }
                            Segmentation::Luminance { info, .. } => {
                                let dom = spryteo_sheet::luma::dominant_ink_color(
                                    &original_crop,
                                    info,
                                    &luma_cfg,
                                )
                                .unwrap_or(spryteo_core::Rgb { r: 0, g: 0, b: 0 });
                                (dom, None)
                            }
                        }
                    };

                let label_rect = match &segmentation {
                    Segmentation::Chroma => None,
                    Segmentation::Luminance { .. } => {
                        let tb = tb_opt.as_ref().unwrap();
                        let icon_band = tb
                            .icon
                            .iter()
                            .find(|b| cell.cell_rect.y1 >= b.start && cell.cell_rect.y1 < b.end)
                            .or_else(|| {
                                let y_mid = (cell.cell_rect.y1 + cell.cell_rect.y2) / 2;
                                tb.icon.iter().find(|b| y_mid >= b.start && y_mid < b.end)
                            });
                        icon_band
                            .and_then(|ib| spryteo_sheet::label_band_below(tb, ib))
                            .map(|b| (cell.crop.x1, b.start, cell.crop.x2, b.end))
                    }
                };

                let mut scene = spryteo_svg::build_stroke_scene_graph(
                    &curve_set,
                    &opts.id_style,
                    &transformed_widths,
                );

                for group in &mut scene.groups {
                    for node in &mut group.nodes {
                        if let Some(ref mut stroke) = node.stroke {
                            stroke.color = stroke_color;
                            stroke.paint = stroke_paint.clone();
                        }
                    }
                }

                let n_stroke = stroke_ex.paths.len();
                let n_fill = transformed_fill_curves.len();

                if n_fill > 0 {
                    let fill_paint = stroke_paint
                        .clone()
                        .unwrap_or(spryteo_core::Fill::Solid(stroke_color));

                    let mut all_ids: Vec<String> = Vec::with_capacity(n_stroke + n_fill);
                    for group in &scene.groups {
                        if let Some(node) = group.nodes.first() {
                            all_ids.push(node.id.clone());
                        } else {
                            all_ids.push(String::new());
                        }
                    }

                    for (j, fill_curve) in transformed_fill_curves.iter().enumerate() {
                        let idx = n_stroke + j;
                        let id = match opts.id_style {
                            spryteo_core::options::IdStyle::Hash => {
                                let endpoints = extract_endpoints(&fill_curve.segments);
                                spryteo_geom::stable_id(&endpoints, None, idx)
                            }
                            spryteo_core::options::IdStyle::Sequential => {
                                format!("s-{}", idx)
                            }
                            spryteo_core::options::IdStyle::None => "".to_string(),
                        };
                        all_ids.push(id);
                    }

                    if !matches!(opts.id_style, spryteo_core::options::IdStyle::None) {
                        spryteo_geom::dedupe_ids(&mut all_ids);
                    }

                    for (i, group) in scene.groups.iter_mut().enumerate() {
                        let id = all_ids[i].clone();
                        let group_id = if id.is_empty() {
                            "".to_string()
                        } else {
                            format!("g-{}", id)
                        };
                        group.id = group_id;
                        if let Some(node) = group.nodes.first_mut() {
                            node.id = id;
                        }
                    }

                    for (j, fill_curve) in transformed_fill_curves.iter().enumerate() {
                        let id = all_ids[n_stroke + j].clone();
                        let group_id = if id.is_empty() {
                            "".to_string()
                        } else {
                            format!("g-{}", id)
                        };
                        let node = spryteo_core::ir::Node {
                            id: id.clone(),
                            fill: Some(fill_paint.clone()),
                            stroke: None,
                            transform: spryteo_core::ir::Transform {
                                translate_x: 0.0,
                                translate_y: 0.0,
                            },
                            shape: spryteo_core::ir::Shape::Path(fill_curve.segments.clone()),
                        };
                        let group = spryteo_core::ir::Group {
                            id: group_id,
                            nodes: vec![node],
                            groups: vec![],
                        };
                        scene.groups.push(group);
                    }
                }

                let mut icon_opts = opts.clone();
                if sheet.flat {
                    icon_opts.current_color = true;
                }

                let mean_width = if transformed_widths.is_empty() {
                    0.0
                } else {
                    transformed_widths.iter().sum::<f64>() / transformed_widths.len() as f64
                };

                let total_path_count = stroke_ex.paths.len() + transformed_fill_curves.len();

                Ok((
                    scene,
                    icon_opts,
                    IconReport {
                        name: icon_name.clone(),
                        col: cell.col,
                        row: cell.row,
                        source_rect: (
                            cell.cell_rect.x1,
                            cell.cell_rect.y1,
                            cell.cell_rect.width(),
                            cell.cell_rect.height(),
                        ),
                        ink_pixels: cell.ink_pixels,
                        path_count: total_path_count,
                        stroke_width: mean_width,
                        filled_components: stroke_ex.filled_count,
                        gradient_residual,
                        regularize: reg_info,
                        warnings: icon_warnings,
                        label_rect,
                        width_unified: None,
                    },
                ))
            },
        ));

        match cell_res {
            Ok(Ok((scene, icon_opts, report))) => {
                pending_icons.push(PendingIcon {
                    scene_and_opts: Some((scene, icon_opts)),
                    report,
                });
            }
            Ok(Err(msg)) => {
                pending_icons.push(PendingIcon {
                    scene_and_opts: None,
                    report: IconReport {
                        name: icon_name,
                        col: cell.col,
                        row: cell.row,
                        source_rect: (
                            cell.cell_rect.x1,
                            cell.cell_rect.y1,
                            cell.cell_rect.width(),
                            cell.cell_rect.height(),
                        ),
                        ink_pixels: cell.ink_pixels,
                        path_count: 0,
                        stroke_width: 0.0,
                        filled_components: 0,
                        gradient_residual: None,
                        regularize: None,
                        warnings: vec![msg],
                        label_rect: None,
                        width_unified: None,
                    },
                });
            }
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                pending_icons.push(PendingIcon {
                    scene_and_opts: None,
                    report: IconReport {
                        name: icon_name,
                        col: cell.col,
                        row: cell.row,
                        source_rect: (
                            cell.cell_rect.x1,
                            cell.cell_rect.y1,
                            cell.cell_rect.width(),
                            cell.cell_rect.height(),
                        ),
                        ink_pixels: cell.ink_pixels,
                        path_count: 0,
                        stroke_width: 0.0,
                        filled_components: 0,
                        gradient_residual: None,
                        regularize: None,
                        warnings: vec![format!("Panic while processing icon: {}", msg)],
                        label_rect: None,
                        width_unified: None,
                    },
                });
            }
        }
    }

    let mut unified_stroke_width = None;
    if sheet.unify_widths {
        let mut valid_widths: Vec<f64> = pending_icons
            .iter()
            .filter(|p| p.report.path_count > 0 && p.report.stroke_width > 0.0)
            .map(|p| p.report.stroke_width)
            .collect();

        if !valid_widths.is_empty() {
            valid_widths.sort_by(|a, b| a.total_cmp(b));
            let median = valid_widths[valid_widths.len() / 2];
            unified_stroke_width = Some(median);

            let lower = median * 0.8;
            let upper = median * 1.2;

            for item in &mut pending_icons {
                let w = item.report.stroke_width;
                if item.report.path_count > 0 && w > 0.0 && w >= lower && w <= upper && w != median
                {
                    item.report.width_unified = Some(median);
                    if let Some((ref mut scene, _)) = item.scene_and_opts {
                        for group in &mut scene.groups {
                            for node in &mut group.nodes {
                                if let Some(ref mut stroke) = node.stroke {
                                    stroke.width = median;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut icon_reports = Vec::with_capacity(pending_icons.len());
    let mut written_count = 0usize;

    for item in pending_icons {
        if let Some((scene, icon_opts)) = item.scene_and_opts {
            let canonical_dim = sheet.canonical_size as u32;
            let convert_res =
                spryteo_svg::emit_stroke_svg(&scene, canonical_dim, canonical_dim, &icon_opts);

            let out_filename = format!("{}.svg", item.report.name);
            let out_path = sheet.out_dir.join(&out_filename);

            if !sheet.dry_run {
                if let Err(e) = std::fs::write(&out_path, &convert_res.svg) {
                    let mut rep = item.report;
                    rep.warnings.push(format!("Failed to write SVG: {}", e));
                    icon_reports.push(rep);
                    continue;
                }
                written_count += 1;
            }
        }
        icon_reports.push(item.report);
    }

    let (segmentation_str, polarity, text_rows_removed) = match segmentation {
        Segmentation::Chroma => ("chroma".to_string(), None, 0),
        Segmentation::Luminance {
            info,
            text_rows_removed,
        } => (
            "luminance".to_string(),
            Some(if info.dark_on_light {
                "dark_on_light".to_string()
            } else {
                "light_on_dark".to_string()
            }),
            text_rows_removed,
        ),
    };

    let report = SheetReport {
        icons: icon_reports,
        cols: lattice.cols.len(),
        rows: lattice.rows.len(),
        confidence: lattice.confidence,
        matched: rec.matched,
        straddling: rec.straddling.len(),
        orphans: rec.orphans.len(),
        empty_cells: rec.empty_cells.len(),
        written: written_count,
        segmentation: segmentation_str,
        polarity,
        text_rows_removed,
        unified_stroke_width,
    };

    // 7. Write the manifest as pretty JSON if sheet.manifest is set and !sheet.dry_run
    if let Some(ref manifest_path) = sheet.manifest {
        if !sheet.dry_run {
            if let Some(parent) = manifest_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let json_str = serde_json::to_string_pretty(&report).map_err(|e| {
                SpryteoError::Internal(format!("Failed to serialize manifest JSON: {}", e))
            })?;
            std::fs::write(manifest_path, json_str).map_err(|e| {
                SpryteoError::Internal(format!(
                    "Failed to write manifest to '{}': {}",
                    manifest_path.display(),
                    e
                ))
            })?;
        }
    }

    Ok(report)
}
