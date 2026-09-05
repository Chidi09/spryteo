//! Alpha-channel handling (ROADMAP §3.1, `ConvertOptions::alpha_mode`).
//!
//! Applied immediately after decode and before classification, so every
//! downstream stage — the classifier, the quantizer's transparent-pixel
//! skip, contour extraction — sees one consistent alpha representation.
//! Running it later would let the classifier decide a mode from pixels the
//! matte is about to replace.

use spryteo_core::{AlphaMode, RasterImage, Rgb};

/// Composite `src` over `bg` for one channel using straight (un-premultiplied)
/// alpha, in sRGB space.
///
/// sRGB rather than linear light is a deliberate choice: the decoded buffer
/// is sRGB-encoded, every other stage (quantization, gradient fitting,
/// classification) treats those bytes as sRGB, and PNG/CSS compositing is
/// conventionally specified this way. Compositing in linear light here
/// would make matted edges disagree with how the same colours are matched
/// everywhere else in the pipeline.
#[inline]
fn composite_channel(src: u8, bg: u8, alpha: u8) -> u8 {
    if alpha == 255 {
        return src;
    }
    if alpha == 0 {
        return bg;
    }
    let a = alpha as f64 / 255.0;
    let value = src as f64 * a + bg as f64 * (1.0 - a);
    value.round().clamp(0.0, 255.0) as u8
}

/// Apply the configured [`AlphaMode`] to a decoded image.
///
/// - [`AlphaMode::Keep`] returns the image untouched, preserving soft
///   coverage for the stages that understand it.
/// - [`AlphaMode::Matte`] composites every pixel over the given colour and
///   returns a fully opaque image. Fully transparent pixels take the matte
///   colour exactly, which also neutralises the arbitrary RGB garbage many
///   encoders leave under `alpha == 0`.
/// - [`AlphaMode::Threshold`] snaps alpha to 0 or 255 at the given cutoff
///   (`alpha >= t` is opaque), leaving RGB alone. `t == 0` makes everything
///   opaque; `t == 255` keeps only fully opaque pixels.
pub fn apply_alpha_mode(image: RasterImage, mode: &AlphaMode) -> RasterImage {
    match mode {
        AlphaMode::Keep => image,
        AlphaMode::Matte(bg) => matte(image, *bg),
        AlphaMode::Threshold(t) => threshold(image, *t),
    }
}

fn matte(mut image: RasterImage, bg: Rgb) -> RasterImage {
    for px in image.pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        px[0] = composite_channel(px[0], bg.r, a);
        px[1] = composite_channel(px[1], bg.g, a);
        px[2] = composite_channel(px[2], bg.b, a);
        px[3] = 255;
    }
    image
}

fn threshold(mut image: RasterImage, t: u8) -> RasterImage {
    for px in image.pixels.as_chunks_mut::<4>().0 {
        px[3] = if px[3] >= t { 255 } else { 0 };
    }
    image
}

#[cfg(test)]
#[path = "alpha_tests.rs"]
mod tests;
