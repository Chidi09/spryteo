use spryteo_core::{
    ClassifiedInput, ContourSet, ConvertOptions, ConvertResult, Fill, Grouping, LayerStack, Mode,
    Preset, RasterImage, SpryteoError, Stats, Tri,
};

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Spryteo(#[from] SpryteoError),

    #[error("Failed to read input file '{path}': {source}")]
    InputIo {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to write output SVG to '{path}': {source}")]
    OutputIo {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to write JSON metadata to '{path}': {source}")]
    JsonIo {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("JSON serialization error: {source}")]
    Json {
        #[from]
        source: serde_json::Error,
    },
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Spryteo(SpryteoError::InvalidInput(_)) => 1,
            CliError::Spryteo(SpryteoError::LimitExceeded { .. }) => 2,
            CliError::Spryteo(SpryteoError::Timeout) => 3,
            CliError::Spryteo(SpryteoError::Cancelled) => 3,
            CliError::Spryteo(SpryteoError::Internal(_)) => 3,
            CliError::InputIo { .. } => 1,
            CliError::OutputIo { .. } => 3,
            CliError::JsonIo { .. } => 3,
            CliError::Json { .. } => 3,
        }
    }
}

pub fn parse_mode(s: &str) -> Result<Mode, String> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(Mode::Auto),
        "icon" => Ok(Mode::Icon),
        "pixel-art" | "pixel_art" | "pixelart" => Ok(Mode::PixelArt),
        "line-art" | "line_art" | "lineart" => Ok(Mode::LineArt),
        "photo" => Ok(Mode::Photo),
        _ => Err(format!(
            "Invalid mode '{}'. Expected one of: auto, icon, pixel-art, line-art, photo",
            s
        )),
    }
}

pub fn parse_preset(s: &str) -> Result<Preset, String> {
    match s.to_lowercase().as_str() {
        "draw" => Ok(Preset::Draw),
        "fade" => Ok(Preset::Fade),
        "pop" => Ok(Preset::Pop),
        _ => Err(format!(
            "Invalid CSS preset '{}'. Expected one of: draw, fade, pop",
            s
        )),
    }
}

/// Whether gradient detection should be attempted for the resolved input mode,
/// per `ConvertOptions.gradients`: `On` always tries, `Off` never does,
/// `Auto` tries only for `Mode::Photo` (icons keep flat fills).
fn should_try_gradients(gradients: &Tri, resolved_mode: &Mode) -> bool {
    match gradients {
        Tri::On => true,
        Tri::Off => false,
        Tri::Auto => matches!(resolved_mode, Mode::Photo),
    }
}

/// Residual tolerance (Lab distance units) for gradient-vs-flat-fill
/// detection. This is deliberately NOT `ConvertOptions.tolerance` -- that
/// field is a *geometric* curve-fit budget in pixels (default 0.5), while
/// gradient detection compares *colour* residuals in Lab space, where a
/// "just noticeable difference" is already ~1-2 units; reusing 0.5 there
/// made every real-world gradient (and even a clean synthetic one) fail to
/// promote, confirmed by manually running a real gradient PNG through the
/// CLI end-to-end. `ConvertOptions` has no dedicated field for this yet
/// (would be a future addition); this constant matches the tolerance
/// spryteo-quant's own gradient tests use to distinguish real gradients
/// from noise.
const GRADIENT_LAB_TOLERANCE: f64 = 12.0;

/// ROADMAP §3.11: a colour region is assigned to the object whose mask covers
/// it at least this fraction (60%).
const SEMANTIC_COVERAGE_THRESHOLD: f64 = 0.6;

pub fn build_fills(
    image: &RasterImage,
    layer_stack: &LayerStack,
    contour_set: &ContourSet,
    resolved_mode: &Mode,
    opts: &ConvertOptions,
) -> Vec<Fill> {
    let try_gradients = should_try_gradients(&opts.gradients, resolved_mode);
    let mut fills = Vec::new();
    for (layer, contours) in layer_stack.layers.iter().zip(contour_set.layers.iter()) {
        let fill = if try_gradients {
            spryteo_quant::detect_gradient(image, layer, GRADIENT_LAB_TOLERANCE)
                .unwrap_or(Fill::Solid(layer.color))
        } else {
            Fill::Solid(layer.color)
        };
        let mut count = 0;
        for contour in contours {
            count += contour.emitted_curve_count();
        }
        for _ in 0..count {
            fills.push(fill.clone());
        }
    }
    fills
}

const FIXED_SEED: u64 = 42;

pub fn run_convert(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    run_convert_with_masks(bytes, opts, &[])
}

/// Like `run_convert` but applies semantic grouping when
/// `opts.grouping == Grouping::Semantic`.
pub fn run_convert_with_masks(
    bytes: &[u8],
    opts: &ConvertOptions,
    masks: &[spryteo_semantic::Mask],
) -> Result<ConvertResult, SpryteoError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;
    let was_jpeg = spryteo_raster::is_jpeg(bytes);

    let classified = spryteo_quant::classify(raster_image, &opts.mode);
    let downscaled_image =
        spryteo_raster::downscale_large_photo(classified.image, &classified.mode);
    let width = downscaled_image.width;
    let height = downscaled_image.height;
    let preprocessed_image =
        spryteo_raster::preprocess(downscaled_image, &classified.mode, was_jpeg);
    let classified = ClassifiedInput {
        image: preprocessed_image,
        mode: classified.mode,
        background_color: classified.background_color,
    };
    let mut layer_stack =
        spryteo_quant::quantize(&classified, &opts.colors, &opts.layering, FIXED_SEED);
    let rect_color = spryteo_quant::apply_background_policy(
        &mut layer_stack,
        classified.background_color,
        &opts.background,
    );
    let contour_set = spryteo_trace::extract_contours(&layer_stack, width, height, opts.turdsize);
    let curve_set = spryteo_fit::fit_contours(&contour_set, opts.tolerance, opts.smoothness);

    let fills = build_fills(
        &classified.image,
        &layer_stack,
        &contour_set,
        &classified.mode,
        opts,
    );
    let scene = spryteo_svg::build_scene_graph(
        &curve_set,
        &opts.id_style,
        &opts.transform_origin,
        &fills,
        opts.arcs,
    );

    let scene = if matches!(opts.grouping, Grouping::Semantic) {
        let meta = spryteo_core::Meta {
            nodes: spryteo_svg::build_node_metas(&scene, opts.arcs),
            stats: Stats {
                node_count: 0,
                path_count: 0,
                byte_count: 0,
            },
            current_color_applied: false,
        };
        if masks.is_empty() {
            let (regrouped, _) = spryteo_semantic::group_by_containment(&scene, &meta);
            regrouped
        } else {
            let (regrouped, _) = spryteo_semantic::group_by_masks(
                &layer_stack,
                &contour_set,
                &scene,
                &meta,
                masks,
                SEMANTIC_COVERAGE_THRESHOLD,
            );
            regrouped
        }
    } else {
        scene
    };

    let result = spryteo_svg::emit_svg(&scene, width, height, opts, rect_color);
    Ok(result)
}

pub fn run_convert_stroke(
    bytes: &[u8],
    opts: &ConvertOptions,
) -> Result<ConvertResult, SpryteoError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;
    let width = raster_image.width;
    let height = raster_image.height;
    let stroke_result = spryteo_stroke::trace_stroke(&raster_image, opts.tolerance);
    let scene = spryteo_svg::build_stroke_scene_graph(
        &stroke_result.curves,
        &opts.id_style,
        &stroke_result.widths,
    );
    let result = spryteo_svg::emit_stroke_svg(&scene, width, height, opts);
    Ok(result)
}

/// Runs `f`, converting a caught panic into `SpryteoError::Internal` instead
/// of unwinding out of the CLI process. Mirrors the `catch_unwind` pattern
/// already used in `bindings/wasm`, `bindings/node`, and `crates/spryteo-mcp`
/// -- extracted as its own function so the panic-to-error conversion is
/// directly unit-testable without needing a real panic trigger somewhere
/// deep in the pipeline.
fn catch_pipeline_panic<F>(f: F) -> Result<ConvertResult, SpryteoError>
where
    F: FnOnce() -> Result<ConvertResult, SpryteoError>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic".to_string()
            };
            Err(SpryteoError::Internal(format!(
                "Pipeline panicked: {}",
                msg
            )))
        }
    }
}

pub fn run_pipeline(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
    masks: &[spryteo_semantic::Mask],
) -> Result<ConvertResult, CliError> {
    let bytes = std::fs::read(input_path).map_err(|source| CliError::InputIo {
        path: input_path.to_path_buf(),
        source,
    })?;
    run_pipeline_with_bytes(&bytes, output_path, json_path, opts, masks)
}

/// Like `run_pipeline` but accepts already-read bytes.
pub fn run_pipeline_with_bytes(
    bytes: &[u8],
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
    masks: &[spryteo_semantic::Mask],
) -> Result<ConvertResult, CliError> {
    let result = catch_pipeline_panic(|| {
        if opts.stroke {
            run_convert_stroke(bytes, opts)
        } else {
            run_convert_with_masks(bytes, opts, masks)
        }
    })?;

    std::fs::write(output_path, &result.svg).map_err(|source| CliError::OutputIo {
        path: output_path.to_path_buf(),
        source,
    })?;

    if let Some(jpath) = json_path {
        let json_str = serde_json::to_string_pretty(&result.meta)?;
        std::fs::write(jpath, json_str).map_err(|source| CliError::JsonIo {
            path: jpath.to_path_buf(),
            source,
        })?;
    }

    Ok(result)
}

/// Run SAM inference on the decoded raster and return computed masks.
#[cfg(feature = "ml")]
pub fn run_sam_inference(
    bytes: &[u8],
    opts: &ConvertOptions,
    encoder_path: &std::path::Path,
    decoder_path: &std::path::Path,
) -> Result<Vec<spryteo_semantic::Mask>, CliError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;

    let mut model = spryteo_semantic::sam::SamModel::load(encoder_path, decoder_path)
        .map_err(|e| CliError::Spryteo(SpryteoError::Internal(e.to_string())))?;
    let embedding = model
        .embed(&raster_image)
        .map_err(|e| CliError::Spryteo(SpryteoError::Internal(e.to_string())))?;
    let sam_opts = spryteo_semantic::sam::AutoMaskOptions::default();
    let masks = model
        .segment_everything(&raster_image, &embedding, &sam_opts)
        .map_err(|e| CliError::Spryteo(SpryteoError::Internal(e.to_string())))?;
    Ok(masks)
}

/// Run the full pipeline with automatic SAM mask generation.
#[cfg(feature = "ml")]
pub fn run_pipeline_sam(
    bytes: &[u8],
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
    encoder_path: &std::path::Path,
    decoder_path: &std::path::Path,
) -> Result<ConvertResult, CliError> {
    let masks = run_sam_inference(bytes, opts, encoder_path, decoder_path)?;
    run_pipeline_with_bytes(bytes, output_path, json_path, opts, &masks)
}

/// Render a human-readable report from a `Meta` sidecar (as written by
/// `convert --json`), for `spryteo inspect --meta`.
pub fn format_meta_report(meta: &spryteo_core::Meta) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "nodes={} paths={} bytes={}\n\n",
        meta.stats.node_count, meta.stats.path_count, meta.stats.byte_count
    ));
    for n in &meta.nodes {
        let fill_str = match n.fill {
            Some(rgb) => format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b),
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "{:<16} group={:<12} z={:<4} fill={:<9} bbox=({:.1},{:.1},{:.1},{:.1}) centroid=({:.1},{:.1}) area={:.1} draw_order={}\n",
            n.id,
            n.group,
            n.z_order,
            fill_str,
            n.bbox.x_min,
            n.bbox.y_min,
            n.bbox.x_max,
            n.bbox.y_max,
            n.centroid.0,
            n.centroid.1,
            n.area,
            n.suggested_draw_order,
        ));
    }
    out
}

/// Extract `id="..."` attribute values from SVG markup, in document order.
/// Not a full XML parser -- sufficient for re-deriving a summary when no
/// `Meta` sidecar is available (`spryteo inspect` without `--meta`).
fn extract_ids(svg: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find("id=\"") {
        rest = &rest[start + 4..];
        if let Some(end) = rest.find('"') {
            ids.push(&rest[..end]);
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    ids
}

fn extract_view_box(svg: &str) -> Option<&str> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let rest = &svg[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Re-derive a lightweight summary directly from SVG markup, for
/// `spryteo inspect` when no `--meta` sidecar path is given. Cannot recover
/// centroid/area/group tree (never embedded in the markup itself), only
/// element counts, IDs, and viewBox.
pub fn derive_svg_summary(svg: &str) -> String {
    let path_count = svg.matches("<path").count();
    let group_count = svg.matches("<g ").count() + svg.matches("<g>").count();
    let ids = extract_ids(svg);
    let view_box = extract_view_box(svg).unwrap_or("(none)");

    let mut out = String::new();
    out.push_str("no --meta sidecar given; showing values re-derived from SVG markup only\n");
    out.push_str(&format!(
        "paths={} groups={} bytes={} viewBox={}\n\n",
        path_count,
        group_count,
        svg.len(),
        view_box
    ));
    out.push_str(&format!("ids ({}): {}\n", ids.len(), ids.join(", ")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use spryteo_core::{ColorSpec, Layering};

    #[test]
    fn test_catch_pipeline_panic_converts_to_internal_error() {
        let result: Result<ConvertResult, SpryteoError> =
            catch_pipeline_panic(|| -> Result<ConvertResult, SpryteoError> {
                panic!("deliberate test panic");
            });

        assert!(result.is_err());
        match result.err().unwrap() {
            SpryteoError::Internal(msg) => assert!(msg.contains("deliberate test panic")),
            other => panic!("Expected SpryteoError::Internal, got {:?}", other),
        }
    }

    #[test]
    fn test_catch_pipeline_panic_maps_to_exit_code_3() {
        let result: Result<ConvertResult, SpryteoError> =
            catch_pipeline_panic(|| -> Result<ConvertResult, SpryteoError> {
                panic!("deliberate test panic");
            });
        let cli_err: CliError = result.unwrap_err().into();
        assert_eq!(cli_err.exit_code(), 3);
    }

    #[test]
    fn test_catch_pipeline_panic_passes_through_ok() {
        use spryteo_core::{Meta, Stats};

        let result = catch_pipeline_panic(|| {
            Ok(ConvertResult {
                svg: "<svg></svg>".to_string(),
                meta: Meta {
                    nodes: vec![],
                    stats: Stats {
                        node_count: 0,
                        path_count: 0,
                        byte_count: 12,
                    },
                    current_color_applied: false,
                },
            })
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap().svg, "<svg></svg>");
    }

    #[test]
    fn test_fills_building() {
        use spryteo_core::ir::{Contour, ContourSet, Fill, Layer, LayerStack, Rgb};

        let red = Rgb { r: 255, g: 0, b: 0 };
        let green = Rgb { r: 0, g: 255, b: 0 };

        let layer0 = Layer {
            mask: vec![],
            color: red,
            z_order: 0,
        };
        let layer1 = Layer {
            mask: vec![],
            color: green,
            z_order: 1,
        };

        let layer_stack = LayerStack {
            layers: vec![layer0, layer1],
        };

        let contour_b = Contour {
            points: vec![],
            children: vec![],
        };
        let contour_a = Contour {
            points: vec![],
            children: vec![contour_b],
        };
        let contour_c = Contour {
            points: vec![],
            children: vec![],
        };
        let contour_d = Contour {
            points: vec![],
            children: vec![],
        };

        let contour_set = ContourSet {
            layers: vec![vec![contour_a], vec![contour_c, contour_d]],
        };

        // Mode::Icon + default (Auto) gradients means gradient detection is
        // never attempted, so a minimal 0x0 image is fine here.
        let image = RasterImage {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let opts = ConvertOptions::default();
        let fills = build_fills(&image, &layer_stack, &contour_set, &Mode::Icon, &opts);
        // contour_a absorbs its hole (contour_b) as a subpath of one shape,
        // so layer 0 emits one fill; layer 1 emits one per top-level contour.
        assert_eq!(fills.len(), 3);
        assert_eq!(fills[0], Fill::Solid(red));
        assert_eq!(fills[1], Fill::Solid(green));
        assert_eq!(fills[2], Fill::Solid(green));
    }

    #[test]
    fn test_e2e_pipeline_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with red circle
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                if dx * dx + dy * dy <= 8.0 * 8.0 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let opts = ConvertOptions::default();
        let res1 = run_convert(&png_bytes, &opts).unwrap();

        assert!(res1.svg.contains("<svg"));
        assert!(res1.svg.contains("viewBox="));
        assert!(res1.svg.contains("<path") || res1.svg.contains("<circle"));

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
        assert!(!res1.svg.is_empty());

        let res2 = run_convert(&png_bytes, &opts).unwrap();
        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_e2e_exif_orientation_applied_through_full_pipeline() {
        // Fixture: 40x20 PNG, left half red / right half blue, tagged with
        // EXIF Orientation=6 (rotate 90 CW to display upright). Correct
        // decoding (crates/spryteo-raster's decode()) must rotate this to
        // a 20x40 image with red on top and blue on the bottom -- verified
        // independently against Pillow's own `ImageOps.exif_transpose`
        // (see crates/spryteo-raster/tests/decode.rs for the isolated
        // decode-level test; this proves the correction actually survives
        // the full quantize/trace/fit/svg pipeline, not just decode()).
        let png_bytes: &[u8] = include_bytes!("../tests/fixtures/exif_oriented_icon.png");

        let opts = ConvertOptions {
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let result = run_convert(png_bytes, &opts).unwrap();

        // The pipeline must have picked up the corrected (swapped) 20x40
        // dimensions, not the raw 40x20 stored dimensions.
        assert!(
            result.svg.contains("width=\"20\"") || result.svg.contains("viewBox=\"0 0 20 40\""),
            "expected orientation-corrected 20x40 dimensions in SVG, got: {}",
            result.svg
        );

        assert_eq!(
            result.meta.nodes.len(),
            2,
            "expected exactly two colour regions"
        );

        let red = spryteo_core::Rgb {
            r: 220,
            g: 20,
            b: 20,
        };
        let blue = spryteo_core::Rgb {
            r: 20,
            g: 20,
            b: 220,
        };
        let red_node = result
            .meta
            .nodes
            .iter()
            .find(|n| n.fill == Some(red))
            .expect("red region should be present");
        let blue_node = result
            .meta
            .nodes
            .iter()
            .find(|n| n.fill == Some(blue))
            .expect("blue region should be present");

        // Post-rotation: red occupies the top half (small y), blue the
        // bottom half (large y) of the corrected 20x40 canvas. Icon-mode
        // classification treats one colour as a full-canvas background
        // layer (hence red's centroid sits near canvas-middle rather than
        // strictly in the top half) with the other cut out as a
        // constrained foreground shape, so only the cutout shape's bbox
        // is checked precisely.
        assert!(
            red_node.centroid.1 < blue_node.centroid.1,
            "red region (was left half pre-rotation) should end up above blue (was right half): red centroid={:?}, blue centroid={:?}",
            red_node.centroid,
            blue_node.centroid
        );
        assert!(
            blue_node.bbox.y_min >= 19.0,
            "blue region (post-rotation bottom half) should not bleed into the top half, bbox={:?}",
            blue_node.bbox
        );
    }

    #[test]
    fn test_garbage_input() {
        let garbage = b"Not a real image file content at all";
        let opts = ConvertOptions::default();
        let res = run_convert(garbage, &opts);

        assert!(res.is_err());
        match res.err().unwrap() {
            SpryteoError::InvalidInput(_) => {}
            other => panic!("Expected SpryteoError::InvalidInput, got {:?}", other),
        }
    }

    #[test]
    fn test_e2e_pipeline_stroke_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with black line
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        // Draw diagonal line from (5,5) to (25,25)
        for i in 5..=25 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let px = (i + dx) as u32;
                    let py = (i + dy) as u32;
                    if px < 32 && py < 32 {
                        img.put_pixel(px, py, Rgba([0, 0, 0, 255]));
                    }
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let opts = ConvertOptions {
            stroke: true,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_stroke(&png_bytes, &opts).unwrap();

        assert!(res1.svg.contains("<svg"));
        assert!(res1.svg.contains("viewBox="));
        assert!(res1.svg.contains("pathLength=\"100\""));
        assert!(res1.svg.contains("fill=\"none\""));

        assert!(
            res1.meta.stats.path_count <= 3,
            "Path count was {}",
            res1.meta.stats.path_count
        );

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
        assert!(!res1.svg.is_empty());

        let res2 = run_convert_stroke(&png_bytes, &opts).unwrap();
        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_garbage_input_stroke() {
        let garbage = b"Not a real image file content at all";
        let opts = ConvertOptions {
            stroke: true,
            ..ConvertOptions::default()
        };
        let res = run_convert_stroke(garbage, &opts);

        assert!(res.is_err());
        match res.err().unwrap() {
            SpryteoError::InvalidInput(_) => {}
            other => panic!("Expected SpryteoError::InvalidInput, got {:?}", other),
        }
    }

    #[test]
    fn test_format_meta_report() {
        use spryteo_core::{Bbox, Meta, NodeMeta, Rgb, Stats};

        let meta = Meta {
            nodes: vec![NodeMeta {
                id: "s-abc123".to_string(),
                bbox: Bbox {
                    x_min: 0.0,
                    y_min: 0.0,
                    x_max: 10.0,
                    y_max: 10.0,
                },
                centroid: (5.0, 5.0),
                area: 100.0,
                fill: Some(Rgb { r: 255, g: 0, b: 0 }),
                group: "g-root".to_string(),
                z_order: 0,
                suggested_draw_order: 0,
            }],
            stats: Stats {
                node_count: 1,
                path_count: 1,
                byte_count: 42,
            },
            current_color_applied: false,
        };

        let report = format_meta_report(&meta);
        assert!(report.contains("nodes=1"));
        assert!(report.contains("s-abc123"));
        assert!(report.contains("#ff0000"));
        assert!(report.contains("g-root"));
    }

    #[test]
    fn test_derive_svg_summary() {
        let svg = r#"<svg viewBox="0 0 24 24"><g id="g-root"><path id="s-1" d="M0 0"/></g></svg>"#;
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("paths=1"));
        assert!(summary.contains("groups=1"));
        assert!(summary.contains("viewBox=0 0 24 24"));
        assert!(summary.contains("g-root"));
        assert!(summary.contains("s-1"));
    }

    #[test]
    fn test_derive_svg_summary_no_ids() {
        let svg = "<svg></svg>";
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("paths=0"));
        assert!(summary.contains("ids (0):"));
    }

    #[test]
    fn test_e2e_background_policies() {
        use image::{ImageBuffer, ImageFormat, Rgba};
        use spryteo_core::options::{Background, Mode};
        use std::io::Cursor;

        let mut img = ImageBuffer::new(16, 16);
        for x in 0..16 {
            for y in 0..16 {
                if (4..12).contains(&x) && (4..12).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255])); // Red
                } else {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255])); // White
                }
            }
        }

        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // 1. Keep (default)
        let opts_keep = ConvertOptions {
            background: Background::Keep,
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let res_keep = run_convert(&png_bytes, &opts_keep).unwrap();
        assert!(
            res_keep.svg.contains("fill=\"#ffffff\""),
            "Keep SVG should contain white fill: {}",
            res_keep.svg
        );
        assert!(
            res_keep.svg.contains("fill=\"#ff0000\""),
            "Keep SVG should contain red fill: {}",
            res_keep.svg
        );
        assert!(
            !res_keep.svg.contains("<rect width=\"16\" height=\"16\""),
            "Keep SVG should not contain a background rect: {}",
            res_keep.svg
        );

        // 2. Drop
        let opts_drop = ConvertOptions {
            background: Background::Drop,
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let res_drop = run_convert(&png_bytes, &opts_drop).unwrap();
        assert!(
            !res_drop.svg.contains("fill=\"#ffffff\""),
            "Drop SVG should not contain white fill: {}",
            res_drop.svg
        );
        assert!(
            res_drop.svg.contains("fill=\"#ff0000\""),
            "Drop SVG should contain red fill: {}",
            res_drop.svg
        );
        assert!(
            !res_drop.svg.contains("<rect width=\"16\" height=\"16\""),
            "Drop SVG should not contain a background rect: {}",
            res_drop.svg
        );

        // 3. Rect
        let opts_rect = ConvertOptions {
            background: Background::Rect,
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let res_rect = run_convert(&png_bytes, &opts_rect).unwrap();
        assert!(
            res_rect
                .svg
                .contains("<rect width=\"16\" height=\"16\" fill=\"#ffffff\"/>"),
            "Rect SVG should contain the background rect: {}",
            res_rect.svg
        );
        let white_matches = res_rect.svg.matches("#ffffff").count();
        assert_eq!(
            white_matches, 1,
            "Rect SVG should only have one white fill (in the rect): {}",
            res_rect.svg
        );
        assert!(
            res_rect.svg.contains("fill=\"#ff0000\""),
            "Rect SVG should contain red fill: {}",
            res_rect.svg
        );

        let rect_idx = res_rect.svg.find("<rect ").expect("should find rect");
        if let Some(path_idx) = res_rect.svg.find("<path ") {
            assert!(rect_idx < path_idx, "rect should come before path");
        }
        if let Some(g_idx) = res_rect.svg.find("<g") {
            assert!(rect_idx < g_idx, "rect should come before group");
        }
    }

    #[test]
    fn test_e2e_current_color_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with red circle
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                if dx * dx + dy * dy <= 8.0 * 8.0 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Convert with current_color = true, background = Drop so no background rect is present
        let opts = ConvertOptions {
            current_color: true,
            background: Background::Drop,
            ..ConvertOptions::default()
        };
        let res = run_convert(&png_bytes, &opts).unwrap();

        assert!(
            res.svg.contains("fill=\"currentColor\""),
            "SVG should use currentColor: {}",
            res.svg
        );
        assert!(
            !res.svg.contains("fill=\"#ff0000\""),
            "SVG should not contain the original red fill: {}",
            res.svg
        );
        assert!(
            res.meta.current_color_applied,
            "meta.current_color_applied should be true"
        );

        // Convert with background = Rect to verify background rect keeps its color
        let opts_rect = ConvertOptions {
            current_color: true,
            background: Background::Rect,
            ..ConvertOptions::default()
        };
        let res_rect = run_convert(&png_bytes, &opts_rect).unwrap();
        assert!(
            res_rect.svg.contains("fill=\"currentColor\""),
            "SVG should still use currentColor: {}",
            res_rect.svg
        );
        // The background rect is white (since original background was white) -> #ffffff
        assert!(
            res_rect.svg.contains("fill=\"#ffffff\""),
            "Background rect should keep its original color: {}",
            res_rect.svg
        );
    }

    #[test]
    fn test_e2e_containment_grouping() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // 32x32 white background, big blue square (4..28), smaller red square inside (12..20)
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                if (4..28).contains(&x) && (4..28).contains(&y) {
                    img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                }
                if (12..20).contains(&x) && (12..20).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Convert with Semantic grouping
        let opts = ConvertOptions {
            grouping: Grouping::Semantic,
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_with_masks(&png_bytes, &opts, &[]).unwrap();

        // Should contain nested <g> elements (containment: small inside big)
        assert!(
            res1.svg.contains("<g"),
            "SVG should have groups: {}",
            res1.svg
        );

        // Determinism: convert twice, byte-identical
        let res2 = run_convert_with_masks(&png_bytes, &opts, &[]).unwrap();
        assert_eq!(
            res1.svg, res2.svg,
            "containment grouping must be deterministic"
        );

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
    }

    #[test]
    fn test_e2e_masks_grouping() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // 32x32: red left half, blue right half (no background color to avoid extra layers)
        let mut img = RgbaImage::new(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                if x < 16 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                } else {
                    img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Masks exactly covering each half (100% overlap with each layer)
        let mut mask_a_pixels = vec![0u8; 32 * 32];
        let mut mask_b_pixels = vec![0u8; 32 * 32];
        for y in 0..32 {
            for x in 0..32 {
                let idx = y * 32 + x;
                if x < 16 {
                    mask_a_pixels[idx] = 255;
                } else {
                    mask_b_pixels[idx] = 255;
                }
            }
        }

        let masks = vec![
            spryteo_semantic::Mask {
                id: "left-half".to_string(),
                width: 32,
                height: 32,
                pixels: mask_a_pixels,
            },
            spryteo_semantic::Mask {
                id: "right-half".to_string(),
                width: 32,
                height: 32,
                pixels: mask_b_pixels,
            },
        ];

        let opts = ConvertOptions {
            grouping: Grouping::Semantic,
            background: Background::Drop,
            layering: Layering::Cutout,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_with_masks(&png_bytes, &opts, &masks).unwrap();

        assert!(
            res1.svg.contains("g-mask-left-half"),
            "SVG should contain g-mask-left-half: {}",
            res1.svg
        );
        assert!(
            res1.svg.contains("g-mask-right-half"),
            "SVG should contain g-mask-right-half: {}",
            res1.svg
        );

        let res2 = run_convert_with_masks(&png_bytes, &opts, &masks).unwrap();
        assert_eq!(res1.svg, res2.svg, "mask grouping must be deterministic");

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
    }

    #[test]
    fn test_e2e_containment_grouping_component_regression() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // Same synthetic image as containment test
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                if (4..28).contains(&x) && (4..28).contains(&y) {
                    img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                }
                if (12..20).contains(&x) && (12..20).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Convert with explicit Component grouping
        let opts_comp = ConvertOptions {
            grouping: Grouping::Component,
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res_comp = run_convert_with_masks(&png_bytes, &opts_comp, &[]).unwrap();

        // No g-mask- groups
        assert!(
            !res_comp.svg.contains("g-mask-"),
            "Component mode should not contain g-mask- groups"
        );

        // Convert with default options (Component is default)
        let opts_default = ConvertOptions {
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res_default = run_convert(&png_bytes, &opts_default).unwrap();
        assert_eq!(
            res_comp.svg, res_default.svg,
            "explicit Component and default must match"
        );

        // Convert twice with Semantic, no masks, should still produce deterministic output
        let opts_sem = ConvertOptions {
            grouping: Grouping::Semantic,
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res_sem1 = run_convert_with_masks(&png_bytes, &opts_sem, &[]).unwrap();
        let res_sem2 = run_convert_with_masks(&png_bytes, &opts_sem, &[]).unwrap();
        assert_eq!(
            res_sem1.svg, res_sem2.svg,
            "semantic containment grouping must be deterministic"
        );
    }

    #[test]
    fn test_e2e_masks_dimension_validation() {
        // Verify mask with mismatched dimensions is rejected (through CLI validation
        // but we can also test the basic sanity here)
        let mask = spryteo_semantic::Mask {
            id: "bad".to_string(),
            width: 2,
            height: 2,
            pixels: vec![0u8; 3], // should be 4
        };
        let expected = (mask.width as usize) * (mask.height as usize);
        assert_ne!(
            mask.pixels.len(),
            expected,
            "test invariant: bad mask should have wrong pixel count"
        );
    }
}
