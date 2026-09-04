//! SAM (Segment Anything Model) ONNX inference and automatic mask generation.
//!
//! This module provides:
//! - `SamModel`: holds encoder/decoder ONNX sessions for SAM inference.
//! - `preprocess_for_sam`: pure function for image preprocessing.
//! - `generate_grid`: pure function for grid point generation.
//! - `compute_stability_score`: pure function for SAM stability scoring.
//! - `apply_nms`: pure function for greedy IoU-based NMS deduplication.
//! - `segment_everything`: full automatic mask generation pipeline.
//!
//! DETERMINISM: Every non-ort function in this module is fully deterministic
//! (no HashMap iteration, stable sorts, explicit tie-breaks).  However, the
//! **ONNX inference step itself is a deliberate, scoped exception** to the
//! project's byte-identical-output guarantee (ROADMAP §1).  ONNX Runtime's
//! float reduction ops are not guaranteed bit-reproducible across different
//! machines, builds, or ORT versions.  Every other stage of the engine remains
//! fully deterministic.

use spryteo_core::RasterImage;

use super::Mask;

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SamError {
    #[error("Failed to load SAM model: {0}")]
    ModelLoad(String),
    #[error("ONNX inference failed: {0}")]
    Inference(String),
    #[error("Unexpected tensor shape: {0}")]
    Shape(String),
}

// ── Options ──────────────────────────────────────────────────────────────────

/// Options for automatic mask generation via `segment_everything`.
///
/// Defaults match Meta's published SAM automatic mask generator defaults:
/// - `points_per_side`: 32
/// - `pred_iou_thresh`: 0.88
/// - `stability_score_thresh`: 0.95
/// - `nms_iou_thresh`: 0.7
#[derive(Debug, Clone)]
pub struct AutoMaskOptions {
    pub points_per_side: u32,
    pub pred_iou_thresh: f64,
    pub stability_score_thresh: f64,
    pub nms_iou_thresh: f64,
}

impl Default for AutoMaskOptions {
    fn default() -> Self {
        // Meta published SAM automatic mask generator defaults
        Self {
            points_per_side: 32,
            pred_iou_thresh: 0.88,
            stability_score_thresh: 0.95,
            nms_iou_thresh: 0.7,
        }
    }
}

// ── Resize metadata ──────────────────────────────────────────────────────────

/// Metadata about the resize/pad applied during preprocessing.
///
/// Stored so downstream code can map between original image coordinates,
/// resized (unpadded) coordinates, and the SAM 1024×1024 padded space.
#[derive(Debug, Clone, Copy)]
pub struct ResizeInfo {
    pub original_width: u32,
    pub original_height: u32,
    /// Scale factor applied so longest side = 1024.
    pub scale: f32,
    /// Width after scaling (before padding to 1024).
    pub resized_width: u32,
    /// Height after scaling (before padding to 1024).
    pub resized_height: u32,
}

// ── Embedding ────────────────────────────────────────────────────────────────

/// Cached encoder output (image embedding) for reuse across grid points.
pub struct Embedding {
    raw: Vec<f32>,
    /// Preprocessing metadata (scale, dimensions) used to create this embedding.
    /// Needed downstream to map coordinates correctly between original image
    /// space and the encoder's input space.
    pub resize_info: ResizeInfo,
}

impl Embedding {
    pub fn new(data: Vec<f32>) -> Self {
        Self {
            raw: data,
            resize_info: ResizeInfo {
                original_width: 0,
                original_height: 0,
                scale: 1.0,
                resized_width: 0,
                resized_height: 0,
            },
        }
    }

    pub fn with_resize_info(data: Vec<f32>, resize_info: ResizeInfo) -> Self {
        Self {
            raw: data,
            resize_info,
        }
    }

    pub fn data(&self) -> &[f32] {
        &self.raw
    }
}

// ── Resize helper (pure, no ort types) ───────────────────────────────────────

fn resize_bilinear_rgb(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return vec![0u8; (dst_w * dst_h * 3) as usize];
    }
    let mut out = vec![0u8; (dst_w * dst_h * 3) as usize];
    for y in 0..dst_h {
        for x in 0..dst_w {
            let gx = (x as f32 + 0.5) * src_w as f32 / dst_w as f32 - 0.5;
            let gy = (y as f32 + 0.5) * src_h as f32 / dst_h as f32 - 0.5;
            let x0 = (gx.floor().max(0.0) as u32).min(src_w - 1);
            let x1 = (x0 + 1).min(src_w - 1);
            let y0 = (gy.floor().max(0.0) as u32).min(src_h - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = gx - x0 as f32;
            let fy = gy - y0 as f32;
            for c in 0..3 {
                let p00 = src[((y0 * src_w + x0) * 3 + c) as usize] as f32;
                let p01 = src[((y0 * src_w + x1) * 3 + c) as usize] as f32;
                let p10 = src[((y1 * src_w + x0) * 3 + c) as usize] as f32;
                let p11 = src[((y1 * src_w + x1) * 3 + c) as usize] as f32;
                let v =
                    (1.0 - fy) * ((1.0 - fx) * p00 + fx * p01) + fy * ((1.0 - fx) * p10 + fx * p11);
                out[((y * dst_w + x) * 3 + c) as usize] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

// ── Preprocessing (pure, no ort types) ───────────────────────────────────────

/// Preprocess a raster image for the SAM encoder.
///
/// 1. Extracts RGB from RGBA pixels.
/// 2. Resizes so longest side = 1024px, preserving aspect ratio.
/// 3. Pads to exactly 1024×1024 (right/bottom) with zeros (black).
/// 4. Normalizes with ImageNet per-channel mean/std.
///
/// Returns the NCHW `[1, 3, 1024, 1024]` f32 tensor data and resize metadata.
///
/// This function is pure (no `ort` types) so it can be unit-tested without a
/// loaded model.
pub fn preprocess_for_sam(image: &RasterImage) -> (Vec<f32>, ResizeInfo) {
    let ow = image.width;
    let oh = image.height;
    let max_dim = ow.max(oh).max(1);
    let scale = 1024.0 / max_dim as f32;
    let rw = ((ow as f32 * scale).round().max(1.0)) as u32;
    let rh = ((oh as f32 * scale).round().max(1.0)) as u32;

    let rgb: Vec<u8> = image
        .pixels
        .chunks(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();

    let resized_rgb = resize_bilinear_rgb(&rgb, ow, oh, rw, rh);

    let mean: [f32; 3] = [123.675, 116.28, 103.53];
    let std: [f32; 3] = [58.395, 57.12, 57.375];
    let total_pixels = (1024 * 1024) as usize;
    let mut tensor = vec![0.0f32; 3 * total_pixels];

    // Fill whole tensor with normalized black (padding value)
    for c in 0..3 {
        let black_norm = -mean[c] / std[c];
        let base = c * total_pixels;
        for i in 0..total_pixels {
            tensor[base + i] = black_norm;
        }
    }

    // Overwrite resized region with actual normalized pixel values
    for y in 0..rh {
        for x in 0..rw {
            let src_idx = (y * rw + x) as usize;
            let r = resized_rgb[src_idx * 3] as f32;
            let g = resized_rgb[src_idx * 3 + 1] as f32;
            let b = resized_rgb[src_idx * 3 + 2] as f32;

            let c0_off = (y as usize) * 1024 + x as usize;
            let c1_off = total_pixels + (y as usize) * 1024 + x as usize;
            let c2_off = 2 * total_pixels + (y as usize) * 1024 + x as usize;

            tensor[c0_off] = (r - mean[0]) / std[0];
            tensor[c1_off] = (g - mean[1]) / std[1];
            tensor[c2_off] = (b - mean[2]) / std[2];
        }
    }

    let info = ResizeInfo {
        original_width: ow,
        original_height: oh,
        scale,
        resized_width: rw,
        resized_height: rh,
    };

    (tensor, info)
}

/// Convert a raster image to a raw HWC f32 tensor with no normalization.
///
/// Extracts RGB from RGBA and casts to f32, returning the data in HWC layout
/// (height-major, then width, then 3 RGB channels) at the image's native
/// resolution.  The returned `ResizeInfo` has `scale=1.0` since no resize or
/// padding is applied.
///
/// This is the preprocessing path for encoder models whose ONNX input declares
/// shape `[-1, -1, 3]` (rank 3, HWC, no batch dimension) — the ONNX graph
/// itself is expected to perform internal resize and normalization.
pub fn image_to_hwc_f32(image: &RasterImage) -> (Vec<f32>, ResizeInfo) {
    let h = image.height;
    let w = image.width;
    let total = (h * w) as usize;
    let mut data = Vec::with_capacity(total * 3);
    for pixel in image.pixels.chunks_exact(4) {
        data.push(pixel[0] as f32);
        data.push(pixel[1] as f32);
        data.push(pixel[2] as f32);
    }
    let info = ResizeInfo {
        original_width: w,
        original_height: h,
        scale: 1.0,
        resized_width: w,
        resized_height: h,
    };
    (data, info)
}

// ── Grid generation (pure) ───────────────────────────────────────────────────

/// Generate an evenly-spaced `points_per_side × points_per_side` grid over the
/// original image dimensions, in deterministic row-major order.
///
/// Points are returned as `(x, y)` in the **resized** (unpadded) image's pixel
/// coordinates, as required by the SAM decoder.
pub fn generate_grid(
    original_width: u32,
    original_height: u32,
    points_per_side: u32,
    scale: f32,
) -> Vec<(f32, f32)> {
    if points_per_side == 0 {
        return vec![];
    }
    let cell_size = 1.0 / points_per_side as f32;
    let mut points = Vec::with_capacity((points_per_side * points_per_side) as usize);
    for row in 0..points_per_side {
        for col in 0..points_per_side {
            let nx = (col as f32 + 0.5) * cell_size;
            let ny = (row as f32 + 0.5) * cell_size;
            let orig_x = nx * original_width as f32;
            let orig_y = ny * original_height as f32;
            let resized_x = orig_x * scale;
            let resized_y = orig_y * scale;
            points.push((resized_x, resized_y));
        }
    }
    points
}

// ── Stability score (pure) ──────────────────────────────────────────────────

/// Compute the SAM stability score for a raw logit mask.
///
/// The stability score is the IoU between the mask thresholded at two nearby
/// thresholds: `logit > -1` and `logit > +1`.  This is the standard SAM
/// approach from the original paper/repo.
///
/// A perfectly stable mask (all logits far from zero) gives 1.0; a noisy mask
/// where many logits hover around zero gives a lower score.
pub fn compute_stability_score(logits: &[f32]) -> f64 {
    let n = logits.len();
    if n == 0 {
        return 0.0;
    }
    let mut intersection = 0u64;
    let mut union = 0u64;
    for &v in logits {
        let above_low = v > -1.0;
        let above_high = v > 1.0;
        if above_low || above_high {
            union += 1;
            if above_low && above_high {
                intersection += 1;
            }
        }
    }
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

// ── IoU calculation (pure helper) ────────────────────────────────────────────

/// Compute the intersection-over-union of two binary masks.
///
/// Both masks are `Vec<u8>` where 0 = background, 255 = foreground.
fn mask_iou(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut intersection = 0u64;
    let mut union = 0u64;
    for i in 0..n {
        let a_fg = a[i] >= 128;
        let b_fg = b[i] >= 128;
        if a_fg || b_fg {
            union += 1;
            if a_fg && b_fg {
                intersection += 1;
            }
        }
    }
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

// ── NMS (pure) ───────────────────────────────────────────────────────────────

/// A candidate for NMS: a binary mask, its score, and a tie-break key.
pub struct NmsCandidate {
    /// Binary mask pixels (0 or 255), flattened row-major.
    pub mask: Vec<u8>,
    /// Score (typically IoU prediction from SAM).
    pub score: f64,
    /// (grid_point_index, candidate_index) for deterministic tie-breaking.
    pub tie_break: (usize, usize),
}

/// Greedy IoU-based non-maximum suppression for SAM mask candidates.
///
/// 1. Stably sorts by `score` descending (tie-breaking by `tie_break`).
/// 2. Greedily keeps candidates whose IoU with every already-kept mask is
///    strictly below `iou_thresh`.
///
/// Returns kept masks in their original (post-sort) order.
pub fn apply_nms(candidates: Vec<NmsCandidate>, iou_thresh: f64) -> Vec<(Vec<u8>, f64)> {
    if candidates.is_empty() {
        return vec![];
    }

    // Stable sort: descending score, then tie_break ascending (row-major then
    // candidate index).
    let mut sorted: Vec<_> = candidates.into_iter().enumerate().collect();
    sorted.sort_by(|(_, a), (_, b)| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.tie_break.cmp(&b.tie_break))
    });

    let mut kept: Vec<(Vec<u8>, f64)> = Vec::new();
    for (_, candidate) in sorted {
        let should_keep = kept
            .iter()
            .all(|(kept_mask, _)| mask_iou(&candidate.mask, kept_mask) < iou_thresh);
        if should_keep {
            kept.push((candidate.mask, candidate.score));
        }
    }
    kept
}

// ── Upsampling (pure) ────────────────────────────────────────────────────────

/// Upsample a binary mask from SAM's low resolution back to the original image
/// dimensions using nearest-neighbor interpolation.
///
/// `mask` is a `Vec<u8>` (0 or 255) at `mask_width × mask_height`.
/// The mask covers the padded 1024×1024 space; we crop to the resized
/// (unpadded) region, then upsample to the original image dimensions.
pub fn upsample_mask(
    mask: &[u8],
    mask_width: u32,
    mask_height: u32,
    orig_width: u32,
    orig_height: u32,
    resize_info: &ResizeInfo,
) -> Vec<u8> {
    if orig_width == 0 || orig_height == 0 {
        return vec![];
    }

    let scale_x = mask_width as f32 / 1024.0;
    let scale_y = mask_height as f32 / 1024.0;

    let mut out = vec![0u8; (orig_width * orig_height) as usize];

    for oy in 0..orig_height {
        for ox in 0..orig_width {
            let rx = ox as f32 * resize_info.scale;
            let ry = oy as f32 * resize_info.scale;

            let mx = (rx * scale_x).round() as u32;
            let my = (ry * scale_y).round() as u32;

            let mx = mx.min(mask_width - 1);
            let my = my.min(mask_height - 1);

            let idx = (my * mask_width + mx) as usize;
            let val = if idx < mask.len() { mask[idx] } else { 0 };
            out[(oy * orig_width + ox) as usize] = if val >= 128 { 255 } else { 0 };
        }
    }
    out
}

// ── SamModel (requires ort) ──────────────────────────────────────────────────

/// Encapsulates the encoder and decoder ONNX sessions for a SAM model.
///
/// # Determinism note
///
/// ONNX Runtime's float reduction ops are not guaranteed bit-reproducible
/// across different machines, builds, or ORT versions.  Even with
/// single-threaded execution, this ML path is **not** covered by the engine's
/// byte-identical-output guarantee (ROADMAP §1).  Every other stage of the
/// engine remains fully deterministic.
pub struct SamModel {
    encoder: ort::session::Session,
    decoder: ort::session::Session,
}

impl SamModel {
    /// Load encoder and decoder models from ONNX files.
    ///
    /// Both sessions are built with a single intra-op thread for best-effort
    /// reproducibility (multi-threaded reduction ops in ONNX Runtime are not
    /// guaranteed bit-reproducible run-to-run).
    ///
    /// NOTE: Even single-threaded, this ML path is NOT covered by the engine's
    /// byte-identical-output guarantee the way the rest of the pipeline is
    /// -- this is a known, deliberate exception.
    pub fn load(
        encoder_path: &std::path::Path,
        decoder_path: &std::path::Path,
    ) -> Result<Self, SamError> {
        let encoder = ort::session::Session::builder()
            .map_err(|e| SamError::ModelLoad(e.to_string()))?
            .with_intra_threads(1)
            .map_err(|e| SamError::ModelLoad(e.to_string()))?
            .commit_from_file(encoder_path)
            .map_err(|e| SamError::ModelLoad(e.to_string()))?;

        let decoder = ort::session::Session::builder()
            .map_err(|e| SamError::ModelLoad(e.to_string()))?
            .with_intra_threads(1)
            .map_err(|e| SamError::ModelLoad(e.to_string()))?
            .commit_from_file(decoder_path)
            .map_err(|e| SamError::ModelLoad(e.to_string()))?;

        Ok(Self { encoder, decoder })
    }

    /// Run the encoder once on the given image.
    ///
    /// This is the expensive step.  The returned `Embedding` should be reused
    /// across all decoder calls in `segment_everything`.
    pub fn embed(&mut self, image: &RasterImage) -> Result<Embedding, SamError> {
        // ── Introspect encoder input shape at runtime ────────────────────
        //
        // Collect owned metadata from the first input so we don't hold a
        // borrow on `self.encoder` across the `run()` call below.
        let (first_input_name, input_rank) = {
            let inputs = self.encoder.inputs();
            let first = inputs
                .first()
                .ok_or_else(|| SamError::Shape("Encoder has no inputs".to_string()))?;
            let name = first.name().to_string();
            let rank = match first.dtype() {
                ort::value::ValueType::Tensor { shape, .. } => shape.len(),
                _ => return Err(SamError::Shape("Encoder input is not a tensor".to_string())),
            };
            (name, rank)
        };

        let first_output_name: String = self
            .encoder
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .ok_or_else(|| SamError::Shape("Encoder has no outputs".to_string()))?;

        let embedding = match input_rank {
            // Rank 4: NCHW (e.g. [1,3,1024,1024]) — the standard SAM
            // preprocessing with resize-to-1024, pad, ImageNet normalize.
            4 => {
                let (tensor_data, resize_info) = preprocess_for_sam(image);
                let array = ndarray::Array4::from_shape_vec(
                    ndarray::Dim([1usize, 3, 1024, 1024]),
                    tensor_data,
                )
                .map_err(|e| SamError::Shape(e.to_string()))?;

                let tr = ort::value::TensorRef::from_array_view(array.view())
                    .map_err(|e| SamError::Inference(e.to_string()))?;

                let outputs = self
                    .encoder
                    .run(ort::inputs![first_input_name => tr])
                    .map_err(|e| SamError::Inference(e.to_string()))?;

                let out_array = outputs[first_output_name.as_str()]
                    .try_extract_array::<f32>()
                    .map_err(|e| SamError::Shape(e.to_string()))?;

                let data: Vec<f32> = out_array.iter().copied().collect();
                Embedding::with_resize_info(data, resize_info)
            }
            // Rank 3: HWC (e.g. [-1,-1,3]) — community MobileSAM export where
            // the graph does its own internal resize and normalization.  Feed
            // raw RGB pixels (0-255, f32, no normalize, no pad) at native
            // resolution.
            3 => {
                let (pixels_hwc, resize_info) = image_to_hwc_f32(image);
                let h = resize_info.original_height as usize;
                let w = resize_info.original_width as usize;
                let array =
                    ndarray::Array3::from_shape_vec(ndarray::Dim([h, w, 3usize]), pixels_hwc)
                        .map_err(|e| SamError::Shape(e.to_string()))?;

                let tr = ort::value::TensorRef::from_array_view(array.view())
                    .map_err(|e| SamError::Inference(e.to_string()))?;

                let outputs = self
                    .encoder
                    .run(ort::inputs![first_input_name => tr])
                    .map_err(|e| SamError::Inference(e.to_string()))?;

                let out_array = outputs[first_output_name.as_str()]
                    .try_extract_array::<f32>()
                    .map_err(|e| SamError::Shape(e.to_string()))?;

                let data: Vec<f32> = out_array.iter().copied().collect();
                Embedding::with_resize_info(data, resize_info)
            }
            other => {
                return Err(SamError::Shape(format!(
                    "Unexpected encoder input rank: {}. Expected 3 (HWC) or 4 (NCHW).",
                    other
                )))
            }
        };

        Ok(embedding)
    }

    /// Run the decoder for a single point prompt.
    fn decode_point(
        &mut self,
        embedding: &Embedding,
        point: (f32, f32),
        orig_im_size: (f32, f32),
    ) -> Result<(Vec<Vec<f32>>, Vec<f32>), SamError> {
        let emb_array = ndarray::Array4::from_shape_vec(
            ndarray::Dim([1usize, 256, 64, 64]),
            embedding.data().to_vec(),
        )
        .map_err(|e| SamError::Shape(e.to_string()))?;

        let coords =
            ndarray::Array3::from_shape_vec(ndarray::Dim([1usize, 1, 2]), vec![point.0, point.1])
                .map_err(|e| SamError::Shape(e.to_string()))?;

        let labels = ndarray::Array2::from_shape_vec(ndarray::Dim([1usize, 1]), vec![1.0f32])
            .map_err(|e| SamError::Shape(e.to_string()))?;

        let mask_input = ndarray::Array4::<f32>::zeros(ndarray::Dim([1usize, 1, 256, 256]));

        let has_mask = ndarray::Array1::from_shape_vec(ndarray::Dim([1usize]), vec![0.0f32])
            .map_err(|e| SamError::Shape(e.to_string()))?;

        let orig_size = ndarray::Array1::from_shape_vec(
            ndarray::Dim([2usize]),
            vec![orig_im_size.0, orig_im_size.1],
        )
        .map_err(|e| SamError::Shape(e.to_string()))?;

        // Detect output names at runtime.
        let output_names: Vec<String> = self
            .decoder
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();

        // Detect input names at runtime per the task spec — different community
        // ONNX exports use slightly different names.
        let input_names: Vec<String> = self
            .decoder
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();

        // Build inputs in the order the ONNX graph expects them, using the
        // detected names to map from standard SAM decoder inputs.
        let mut named_inputs: Vec<(String, ndarray::ArrayViewD<'_, f32>)> =
            Vec::with_capacity(input_names.len());

        for name in &input_names {
            let lower = name.to_lowercase();
            let view: ndarray::ArrayViewD<'_, f32> = if lower.contains("image_embed") {
                emb_array.view().into_dyn()
            } else if lower.contains("point_coord") {
                coords.view().into_dyn()
            } else if lower.contains("point_label") {
                labels.view().into_dyn()
            } else if lower.contains("has_mask") {
                has_mask.view().into_dyn()
            } else if lower.contains("mask_input") {
                mask_input.view().into_dyn()
            } else if lower.contains("orig_im") || lower.contains("orig_size") {
                orig_size.view().into_dyn()
            } else {
                return Err(SamError::Shape(format!("Unknown decoder input: {}", name)));
            };
            named_inputs.push((name.clone(), view));
        }

        // Create TensorRefs for the ort inputs! macro.  The macro requires
        // compile-time-known number of arguments, which is always 6 for the
        // standard SAM decoder.
        if named_inputs.len() != 6 {
            return Err(SamError::Shape(format!(
                "Expected 6 decoder inputs, got {}",
                named_inputs.len()
            )));
        }

        let tr0 = ort::value::TensorRef::from_array_view(named_inputs[0].1.view())
            .map_err(|e| SamError::Inference(e.to_string()))?;
        let tr1 = ort::value::TensorRef::from_array_view(named_inputs[1].1.view())
            .map_err(|e| SamError::Inference(e.to_string()))?;
        let tr2 = ort::value::TensorRef::from_array_view(named_inputs[2].1.view())
            .map_err(|e| SamError::Inference(e.to_string()))?;
        let tr3 = ort::value::TensorRef::from_array_view(named_inputs[3].1.view())
            .map_err(|e| SamError::Inference(e.to_string()))?;
        let tr4 = ort::value::TensorRef::from_array_view(named_inputs[4].1.view())
            .map_err(|e| SamError::Inference(e.to_string()))?;
        let tr5 = ort::value::TensorRef::from_array_view(named_inputs[5].1.view())
            .map_err(|e| SamError::Inference(e.to_string()))?;

        let outputs = self
            .decoder
            .run(ort::inputs![
                named_inputs[0].0.as_str() => tr0,
                named_inputs[1].0.as_str() => tr1,
                named_inputs[2].0.as_str() => tr2,
                named_inputs[3].0.as_str() => tr3,
                named_inputs[4].0.as_str() => tr4,
                named_inputs[5].0.as_str() => tr5,
            ])
            .map_err(|e| SamError::Inference(e.to_string()))?;

        let mut masks_out: Option<Vec<Vec<f32>>> = None;
        let mut ious_out: Option<Vec<f32>> = None;

        for oname in &output_names {
            let lower = oname.to_lowercase();
            let v = outputs[oname.as_str()]
                .try_extract_array::<f32>()
                .map_err(|e| SamError::Shape(e.to_string()))?;

            if lower.contains("iou") || lower.contains("score") {
                let flat: Vec<f32> = v.iter().copied().collect();
                ious_out = Some(flat);
            } else if lower.contains("mask") || lower.contains("low_res") {
                let shape = v.shape().to_vec();
                if shape.len() == 4 && shape[0] == 1 {
                    let c = shape[1];
                    let h = shape[2];
                    let w = shape[3];
                    let flat: Vec<f32> = v.iter().copied().collect();
                    let mut candidates = Vec::with_capacity(c);
                    let plane = (h * w) as usize;
                    for ci in 0..c {
                        let start = ci * plane;
                        let end = start + plane;
                        candidates.push(flat[start..end].to_vec());
                    }
                    masks_out = Some(candidates);
                } else {
                    return Err(SamError::Shape(format!(
                        "Unexpected masks shape: {:?}",
                        shape
                    )));
                }
            }
        }

        let masks = masks_out.ok_or_else(|| {
            SamError::Shape("No masks output found in decoder outputs".to_string())
        })?;
        let ious = ious_out.ok_or_else(|| {
            SamError::Shape("No iou_predictions output found in decoder outputs".to_string())
        })?;

        Ok((masks, ious))
    }

    /// Run the full automatic mask generation pipeline.
    ///
    /// 1. Generates a `points_per_side × points_per_side` grid over the image.
    /// 2. For each grid point, runs the decoder and collects candidates.
    /// 3. Filters by IoU prediction and stability score.
    /// 4. Deduplicates via greedy IoU-based NMS.
    /// 5. Returns a `Vec<Mask>` where each mask ID is `sam-<index>`.
    pub fn segment_everything(
        &mut self,
        _image: &RasterImage,
        embedding: &Embedding,
        opts: &AutoMaskOptions,
    ) -> Result<Vec<Mask>, SamError> {
        let ri = &embedding.resize_info;
        let ow = ri.original_width;
        let oh = ri.original_height;
        let scale = ri.scale;

        let points = generate_grid(ow, oh, opts.points_per_side, scale);
        let orig_size = (oh as f32, ow as f32);

        // Run one dummy point to discover the output mask resolution.
        let mask_dims = if points.is_empty() {
            (0, 0)
        } else {
            let (masks, _) = self.decode_point(embedding, points[0], orig_size)?;
            if masks.is_empty() {
                (0, 0)
            } else {
                let len = masks[0].len();
                let side = (len as f64).sqrt().round() as u32;
                (side, side)
            }
        };

        if mask_dims.0 == 0 || mask_dims.1 == 0 {
            return Ok(vec![]);
        }

        let mut all_candidates: Vec<NmsCandidate> = Vec::new();

        for (grid_idx, &point) in points.iter().enumerate() {
            let (mask_logits, ious) = self.decode_point(embedding, point, orig_size)?;

            for cand_idx in 0..mask_logits.len() {
                let iou_pred = ious.get(cand_idx).copied().unwrap_or(0.0) as f64;
                if iou_pred < opts.pred_iou_thresh {
                    continue;
                }

                let stability = compute_stability_score(&mask_logits[cand_idx]);
                if stability < opts.stability_score_thresh {
                    continue;
                }

                let binary: Vec<u8> = mask_logits[cand_idx]
                    .iter()
                    .map(|&v| if v > 0.0 { 255 } else { 0 })
                    .collect();

                all_candidates.push(NmsCandidate {
                    mask: binary,
                    score: iou_pred,
                    tie_break: (grid_idx, cand_idx),
                });
            }
        }

        let kept = apply_nms(all_candidates, opts.nms_iou_thresh);

        let mask_w = mask_dims.1;
        let mask_h = mask_dims.0;

        let mut result = Vec::with_capacity(kept.len());
        for (i, (binary_mask, _score)) in kept.into_iter().enumerate() {
            let upsampled = upsample_mask(&binary_mask, mask_w, mask_h, ow, oh, ri);
            result.push(Mask {
                id: format!("sam-{}", i),
                width: ow,
                height: oh,
                pixels: upsampled,
            });
        }

        Ok(result)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
#[path = "sam_tests.rs"]
mod tests;
