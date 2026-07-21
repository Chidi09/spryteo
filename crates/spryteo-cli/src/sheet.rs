use serde::{Deserialize, Serialize};
use spryteo_core::{ConvertOptions, SpryteoError};
use std::path::PathBuf;

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
    pub gradient_residual: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regularize: Option<IconRegularizeReport>,
    pub warnings: Vec<String>,
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

    // 2. Chroma mask
    let sat_thresh = sheet
        .sat_threshold
        .unwrap_or_else(|| spryteo_sheet::auto_sat_threshold(&image));
    let chroma_cfg = spryteo_sheet::ChromaConfig {
        sat_min: sat_thresh,
        ..Default::default()
    };
    let mask = spryteo_sheet::chroma_mask(&image, &chroma_cfg);
    if let Err(e) = spryteo_sheet::check_chroma_usable(&mask) {
        return Err(SpryteoError::InvalidInput(format!(
            "{}: chroma segmentation is not viable for this image",
            e
        )));
    }

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

    if !sheet.dry_run {
        if let Err(e) = std::fs::create_dir_all(&sheet.out_dir) {
            return Err(SpryteoError::Internal(format!(
                "Failed to create output directory '{}': {}",
                sheet.out_dir.display(),
                e
            )));
        }
    }

    let mut icon_reports = Vec::with_capacity(cells.len());
    let mut written_count = 0usize;

    // 6. Iterate cells in deterministic order
    for cell in &cells {
        let icon_name = format!("{}-{:02}-{:02}", sheet.name_prefix, cell.col, cell.row);
        let cell_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<(IconReport, bool), String> {
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

                // 6c. ink_coverage on upscaled crop with sat_ref = sat_threshold in use; threshold at 0.5 into a Vec<bool> ink mask
                let cov = spryteo_sheet::ink_coverage(&upscaled_crop, sat_thresh, &chroma_cfg);
                let ink_mask: Vec<bool> = cov.iter().map(|&c| c >= 0.5).collect();

                // 6d. trace_stroke_ex
                let stroke_opts = spryteo_stroke::StrokeOptions {
                    tolerance: opts.tolerance,
                    ink: spryteo_stroke::InkSource::Mask(ink_mask),
                    prune_spurs: false,
                    min_component_pixels: 0,
                    extend_caps: true,
                };
                let stroke_ex = spryteo_stroke::trace_stroke_ex(&upscaled_crop, &stroke_opts);

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

                let crop_mask = spryteo_sheet::chroma_mask(&original_crop, &chroma_cfg);
                let crop_rect = spryteo_sheet::Bbox {
                    x1: 0,
                    y1: 0,
                    x2: original_crop.width,
                    y2: original_crop.height,
                };
                let color_cfg = spryteo_sheet::ColorConfig::default();

                let (stroke_color, stroke_paint) =
                    if sheet.flat {
                        (spryteo_core::Rgb { r: 0, g: 0, b: 0 }, None)
                    } else {
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
                                let (gx1, gy1) =
                                    transform.apply_point(fit.x1 * s_factor, fit.y1 * s_factor);
                                let (gx2, gy2) =
                                    transform.apply_point(fit.x2 * s_factor, fit.y2 * s_factor);

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

                let mut icon_opts = opts.clone();
                if sheet.flat {
                    icon_opts.current_color = true;
                }

                let canonical_dim = sheet.canonical_size as u32;
                let convert_res =
                    spryteo_svg::emit_stroke_svg(&scene, canonical_dim, canonical_dim, &icon_opts);

                let out_filename = format!("{}.svg", icon_name);
                let out_path = sheet.out_dir.join(&out_filename);

                let mut is_written = false;
                if !sheet.dry_run {
                    std::fs::write(&out_path, &convert_res.svg)
                        .map_err(|e| format!("Failed to write SVG: {}", e))?;
                    is_written = true;
                }

                let mean_width = if transformed_widths.is_empty() {
                    0.0
                } else {
                    transformed_widths.iter().sum::<f64>() / transformed_widths.len() as f64
                };

                Ok((
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
                        path_count: stroke_ex.paths.len(),
                        stroke_width: mean_width,
                        gradient_residual,
                        regularize: reg_info,
                        warnings: icon_warnings,
                    },
                    is_written,
                ))
            },
        ));

        match cell_res {
            Ok(Ok((report, was_written))) => {
                if was_written {
                    written_count += 1;
                }
                icon_reports.push(report);
            }
            Ok(Err(msg)) => {
                icon_reports.push(IconReport {
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
                    gradient_residual: None,
                    regularize: None,
                    warnings: vec![msg],
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
                icon_reports.push(IconReport {
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
                    gradient_residual: None,
                    regularize: None,
                    warnings: vec![format!("Panic while processing icon: {}", msg)],
                });
            }
        }
    }

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
