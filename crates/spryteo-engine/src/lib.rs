//! The single conversion engine behind every Spryteo surface (ROADMAP §5).
//!
//! Before this crate existed, the CLI, the napi addon, the WASM binding and
//! the MCP server each carried their own copy of the same twelve-stage
//! orchestration. The copies drifted: semantic grouping was wired only into
//! the CLI, and every option fix had to be written four times (issue #19).
//!
//! Surfaces are now adapters. They own bytes in, options parsing, and
//! result serialization; they do not own pipeline order, limits,
//! cancellation, or error mapping. Anything a surface needs to vary is
//! expressed as a field on [`EngineRequest`] rather than as a forked copy
//! of the pipeline.

pub mod validate;

use spryteo_core::{
    CancelToken, ClassifiedInput, ContourSet, ConvertOptions, ConvertResult, CurveOrigin, Fill,
    Grouping, LayerStack, Meta, Mode, RasterImage, SpryteoError, Stats, Tri,
};
use spryteo_semantic::Mask;

pub use validate::validate;

/// Fixed RNG seed for k-means. Conversion is deterministic by contract:
/// the same bytes and options must produce byte-identical SVG on every
/// surface and every run, which the parity tests in each adapter assert.
const FIXED_SEED: u64 = 42;

/// Residual tolerance (Lab distance units) for gradient-vs-flat-fill
/// detection.
///
/// Deliberately NOT `ConvertOptions.tolerance`: that field is a *geometric*
/// curve-fit budget in pixels (default 0.5), while gradient detection
/// compares *colour* residuals in Lab space, where a just-noticeable
/// difference is already ~1-2 units. Reusing 0.5 made every real gradient
/// — including clean synthetic ones — fail to promote. This value matches
/// what spryteo-quant's own gradient tests use to separate real gradients
/// from noise.
const GRADIENT_LAB_TOLERANCE: f64 = 12.0;

/// ROADMAP §3.11: a colour region joins the object whose mask covers at
/// least this fraction of it.
const SEMANTIC_COVERAGE_THRESHOLD: f64 = 0.6;

/// One conversion, with every surface-specific knob expressed as data.
pub struct EngineRequest<'a> {
    /// Raw encoded image bytes (PNG/JPEG/GIF/WebP/BMP).
    pub bytes: &'a [u8],
    /// Validated options. Use [`EngineRequest::new`] to get validation.
    pub options: &'a ConvertOptions,
    /// Pre-computed semantic masks. Empty means "no masks supplied": with
    /// `Grouping::Semantic` the engine falls back to containment grouping,
    /// which needs no model. This is the extension point that lets a
    /// surface run SAM (or any other segmenter) without forking the
    /// pipeline.
    pub masks: &'a [Mask],
    /// Cooperative cancellation / deadline. `None` means an unbounded run;
    /// the engine substitutes an inert token.
    pub cancel: Option<&'a CancelToken>,
}

impl<'a> EngineRequest<'a> {
    /// A request with no masks and no cancellation.
    pub fn new(bytes: &'a [u8], options: &'a ConvertOptions) -> Self {
        Self {
            bytes,
            options,
            masks: &[],
            cancel: None,
        }
    }

    /// Supply pre-computed semantic masks (see the `masks` field).
    pub fn with_masks(mut self, masks: &'a [Mask]) -> Self {
        self.masks = masks;
        self
    }

    /// Supply a cancellation token / deadline.
    pub fn with_cancel(mut self, cancel: &'a CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }
}

/// Convert `bytes` with `opts`, catching panics and enforcing the deadline.
///
/// This is what a surface should call for a plain conversion.
pub fn convert(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    convert_with(EngineRequest::new(bytes, opts))
}

/// Convert, honouring `opts.timeout_ms` with a real clock.
///
/// Separate from [`convert`] because the WASM surface cannot use
/// `std::time::Instant` and supplies its own clock-backed token instead.
#[cfg(not(target_arch = "wasm32"))]
pub fn convert_with_timeout(
    bytes: &[u8],
    opts: &ConvertOptions,
) -> Result<ConvertResult, SpryteoError> {
    let cancel = CancelToken::from_timeout_opt(opts.timeout_ms);
    convert_with(EngineRequest::new(bytes, opts).with_cancel(&cancel))
}

/// The full engine entry point.
///
/// Validates options, then runs either the stroke or the fill pipeline
/// inside a panic guard so a bug deep in tracing surfaces as
/// `SpryteoError::Internal` rather than unwinding across an FFI boundary.
pub fn convert_with(req: EngineRequest<'_>) -> Result<ConvertResult, SpryteoError> {
    validate(req.options)?;
    let EngineRequest {
        bytes,
        options,
        masks,
        cancel,
    } = req;
    let inert = CancelToken::none();
    let cancel = cancel.unwrap_or(&inert);
    catch_pipeline_panic(|| {
        if options.stroke {
            run_stroke(bytes, options, cancel)
        } else {
            run_fill(bytes, options, masks, cancel)
        }
    })
}

/// Decode and resolve alpha: bytes → a raster every later stage can trust.
///
/// Shared by the fill and stroke pipelines so `alpha_mode` applies
/// identically to both — issue #5 called out that stroke mode silently
/// skipped alpha handling entirely.
fn decode_and_resolve_alpha(
    bytes: &[u8],
    opts: &ConvertOptions,
    cancel: &CancelToken,
) -> Result<(RasterImage, bool), SpryteoError> {
    cancel.check()?;
    let image = spryteo_raster::decode(bytes, opts)?;
    let was_jpeg = spryteo_raster::is_jpeg(bytes);

    // Alpha is resolved before anything inspects colour, so the classifier
    // and quantizer never see pixels a matte is about to replace.
    let image = spryteo_raster::apply_alpha_mode(image, &opts.alpha_mode);
    cancel.check()?;

    Ok((image, was_jpeg))
}

/// Geometry options rescaled into a reduced working coordinate system.
///
/// `tolerance` is a distance in pixels and `turdsize` an area in square
/// pixels, so tracing a half-size raster with the caller's raw values would
/// silently double the effective simplification and quadruple the effective
/// despeckling. Issue #4 requires these scale consistently.
struct ScaledGeometry {
    tolerance: f32,
    turdsize: u32,
}

impl ScaledGeometry {
    fn for_scale(opts: &ConvertOptions, scale: f64) -> Self {
        if (scale - 1.0).abs() < f64::EPSILON {
            return Self {
                tolerance: opts.tolerance,
                turdsize: opts.turdsize,
            };
        }
        Self {
            tolerance: (opts.tolerance as f64 * scale) as f32,
            // Area scales with the square of a linear factor. Keep at least
            // 1 so despeckling never switches off entirely.
            turdsize: ((opts.turdsize as f64 * scale * scale).round() as u32).max(1),
        }
    }
}

fn run_fill(
    bytes: &[u8],
    opts: &ConvertOptions,
    masks: &[Mask],
    cancel: &CancelToken,
) -> Result<ConvertResult, SpryteoError> {
    let (image, was_jpeg) = decode_and_resolve_alpha(bytes, opts, cancel)?;

    // Classification runs at full resolution — it is a cheap O(pixels) scan
    // next to preprocessing and quantization, and deciding the mode from the
    // real image keeps `--max-trace-dimension` from changing which pipeline
    // profile an input gets. Its result then selects the resampling filter.
    let classified = spryteo_quant::classify(image, &opts.mode);
    cancel.check()?;

    let geom = match opts.max_trace_dimension {
        Some(max_dim) => ScaledGeometry::for_scale(
            opts,
            spryteo_raster::downscale_factor(
                classified.image.width,
                classified.image.height,
                max_dim,
            ),
        ),
        None => ScaledGeometry::for_scale(opts, 1.0),
    };

    // The explicit limit runs before the automatic photo rule; because it
    // can only shrink, an image reduced to <= 1600px leaves the automatic
    // rule with nothing to do. The tighter of the two always wins.
    let image = match opts.max_trace_dimension {
        Some(max_dim) => {
            spryteo_raster::downscale_to_max_dimension(classified.image, max_dim, &classified.mode)
        }
        None => classified.image,
    };
    let classified = ClassifiedInput {
        image,
        mode: classified.mode,
        background_color: classified.background_color,
    };

    let downscaled_image =
        spryteo_raster::downscale_large_photo(classified.image, &classified.mode);
    let width = downscaled_image.width;
    let height = downscaled_image.height;

    let preprocessed_image = spryteo_raster::preprocess_cancellable(
        downscaled_image,
        &classified.mode,
        was_jpeg,
        cancel,
    )?;
    let classified = ClassifiedInput {
        image: preprocessed_image,
        mode: classified.mode,
        background_color: classified.background_color,
    };

    let mut layer_stack = spryteo_quant::quantize_cancellable(
        &classified,
        &opts.colors,
        &opts.layering,
        FIXED_SEED,
        cancel,
    )?;
    let rect_color = spryteo_quant::apply_background_policy(
        &mut layer_stack,
        classified.background_color,
        &opts.background,
    );

    let contour_set = spryteo_trace::extract_contours_cancellable(
        &layer_stack,
        width,
        height,
        geom.turdsize,
        cancel,
    )?;
    let curve_set = spryteo_fit::fit_contours_cancellable(
        &contour_set,
        geom.tolerance,
        opts.smoothness,
        cancel,
    )?;

    let fills = build_fills(
        &classified.image,
        &layer_stack,
        &contour_set,
        &classified.mode,
        opts,
        cancel,
    )?;

    let scene = spryteo_svg::build_scene_graph_with_origins(
        &curve_set,
        &opts.id_style,
        &opts.transform_origin,
        &fills,
        opts.arcs,
        &spryteo_svg::SceneGrouping {
            origins: &build_curve_origins(&contour_set),
            canvas: Some((width, height)),
        },
    );

    let scene = apply_semantic_grouping(scene, &layer_stack, &contour_set, masks, opts, cancel)?;

    cancel.check()?;
    Ok(spryteo_svg::emit_svg(
        &scene, width, height, opts, rect_color,
    ))
}

/// Regroup the scene when `Grouping::Semantic` is requested.
///
/// Runs on every surface now, not just the CLI. With masks it uses
/// mask-guided grouping; without them it falls back to geometric
/// containment, which needs no model and is always available.
fn apply_semantic_grouping(
    scene: spryteo_core::SceneGraph,
    layer_stack: &LayerStack,
    contour_set: &ContourSet,
    masks: &[Mask],
    opts: &ConvertOptions,
    cancel: &CancelToken,
) -> Result<spryteo_core::SceneGraph, SpryteoError> {
    if !matches!(opts.grouping, Grouping::Semantic) {
        return Ok(scene);
    }
    cancel.check()?;

    let meta = Meta {
        schema_version: spryteo_core::ir::META_SCHEMA_VERSION,
        nodes: spryteo_svg::build_node_metas(&scene, opts.arcs),
        groups: spryteo_svg::build_group_metas(&scene),
        stats: Stats {
            node_count: 0,
            path_count: 0,
            byte_count: 0,
        },
        current_color_applied: false,
    };

    let regrouped = if masks.is_empty() {
        spryteo_semantic::group_by_containment(&scene, &meta).0
    } else {
        spryteo_semantic::group_by_masks(
            layer_stack,
            contour_set,
            &scene,
            &meta,
            masks,
            SEMANTIC_COVERAGE_THRESHOLD,
        )
        .0
    };
    Ok(regrouped)
}

fn run_stroke(
    bytes: &[u8],
    opts: &ConvertOptions,
    cancel: &CancelToken,
) -> Result<ConvertResult, SpryteoError> {
    let (image, _was_jpeg) = decode_and_resolve_alpha(bytes, opts, cancel)?;

    // Stroke mode has no classifier stage, and centreline tracing is always
    // a line-art problem, so the triangle filter is the right choice.
    let geom = match opts.max_trace_dimension {
        Some(max_dim) => ScaledGeometry::for_scale(
            opts,
            spryteo_raster::downscale_factor(image.width, image.height, max_dim),
        ),
        None => ScaledGeometry::for_scale(opts, 1.0),
    };
    let image = match opts.max_trace_dimension {
        Some(max_dim) => spryteo_raster::downscale_to_max_dimension(image, max_dim, &Mode::LineArt),
        None => image,
    };

    let width = image.width;
    let height = image.height;

    cancel.check()?;
    let stroke_result = spryteo_stroke::trace_stroke(&image, geom.tolerance);

    cancel.check()?;
    let scene = spryteo_svg::build_stroke_scene_graph(
        &stroke_result.curves,
        &opts.id_style,
        &stroke_result.widths,
    );
    Ok(spryteo_svg::emit_stroke_svg(&scene, width, height, opts))
}

/// Decide each layer's paint: a fitted gradient when one explains the
/// layer's pixels within tolerance, otherwise the layer's flat colour.
///
/// The returned vector is flattened to one entry per *emitted curve*, in
/// the same order `build_scene_graph` consumes them.
pub fn build_fills(
    image: &RasterImage,
    layer_stack: &LayerStack,
    contour_set: &ContourSet,
    resolved_mode: &Mode,
    opts: &ConvertOptions,
    cancel: &CancelToken,
) -> Result<Vec<Fill>, SpryteoError> {
    let try_gradients = should_try_gradients(&opts.gradients, resolved_mode);
    let mut fills = Vec::new();
    for (layer, contours) in layer_stack.layers.iter().zip(contour_set.layers.iter()) {
        cancel.check()?;
        let fill = if try_gradients {
            spryteo_quant::detect_gradient(image, layer, GRADIENT_LAB_TOLERANCE)
                .unwrap_or(Fill::Solid(layer.color))
        } else {
            Fill::Solid(layer.color)
        };
        let count: usize = contours.iter().map(|c| c.emitted_curve_count()).sum();
        fills.extend(std::iter::repeat_n(fill, count));
    }
    Ok(fills)
}

/// One [`CurveOrigin`] per emitted curve, in `fit_contours` order (#11).
///
/// Walks layers and top-level contours exactly as [`build_fills`] does, and
/// for the same reason: `emitted_curve_count` is the only correct way to
/// align per-curve data with the flattened `CurveSet`. Keeping the two walks
/// identical is what guarantees a shape's fill and its group agree.
pub fn build_curve_origins(contour_set: &ContourSet) -> Vec<CurveOrigin> {
    let mut origins = Vec::new();
    for (layer, contours) in contour_set.layers.iter().enumerate() {
        for (component, contour) in contours.iter().enumerate() {
            let count = contour.emitted_curve_count();
            origins.extend(std::iter::repeat_n(CurveOrigin { layer, component }, count));
        }
    }
    origins
}

/// Whether gradient detection should run for the resolved mode: `On`
/// always, `Off` never, `Auto` only for photos (icons keep flat fills).
fn should_try_gradients(gradients: &Tri, resolved_mode: &Mode) -> bool {
    match gradients {
        Tri::On => true,
        Tri::Off => false,
        Tri::Auto => matches!(resolved_mode, Mode::Photo),
    }
}

/// Run `f`, converting a caught panic into `SpryteoError::Internal`.
///
/// Every surface needs this — a panic unwinding across the napi, wasm-
/// bindgen, or MCP boundary is undefined behaviour or a hard process
/// abort — so it lives here once rather than in four copies. Public so
/// surfaces can also wrap their own pre/post-processing in it.
pub fn catch_pipeline_panic<F>(f: F) -> Result<ConvertResult, SpryteoError>
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
            Err(SpryteoError::Internal(format!("Pipeline panicked: {msg}")))
        }
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
