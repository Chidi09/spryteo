use std::io::Cursor;

use image::ImageFormat;
use spryteo_core::{ConvertOptions, LimitKind, RasterImage, SpryteoError};

/// Hardcoded sanity ceiling per §3.1: no dimension may exceed this value.
const MAX_DIMENSION: u32 = 8192;

/// Decode a raw image byte buffer into a [`RasterImage`].
///
/// # Supported formats
///
/// PNG, JPEG, GIF (first frame only), WebP, BMP.  Format is sniffed by magic
/// bytes only (never by extension or content-type).
///
/// # Limits enforced (before full decode when possible)
///
/// - `max_input_bytes` — raw input size
/// - `max_pixels` — total pixel count (`width × height`)
/// - A hardcoded dimension ceiling of 8192 px per §3.1
///
/// # Alpha un-premultiplication
///
/// All five formats currently decoded by the `image` crate use straight
/// (non-premultiplied) alpha, so no un-premultiplication is performed.
/// This is a deliberate no-op documented here for awareness.
///
/// # GIF behaviour
///
/// Only the first frame is decoded.  The `image` crate's default decode
/// path for GIF reads a single frame (it does not auto-animate), so no
/// special frame-selection logic is needed.
///
/// # Deferred work (not yet implemented — see ROADMAP.md §3.1)
///
/// - EXIF orientation correction (TODO(phase1))
/// - ICC profile → sRGB conversion (TODO(phase1))
pub fn decode(bytes: &[u8], opts: &ConvertOptions) -> Result<RasterImage, SpryteoError> {
    if (bytes.len() as u64) > opts.max_input_bytes {
        return Err(SpryteoError::LimitExceeded {
            kind: LimitKind::InputBytes,
            value: bytes.len() as u64,
            max: opts.max_input_bytes,
        });
    }

    let format =
        image::guess_format(bytes).map_err(|e| SpryteoError::InvalidInput(e.to_string()))?;

    if !matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::WebP
            | ImageFormat::Bmp
    ) {
        return Err(SpryteoError::InvalidInput(format!(
            "Unsupported image format: {format:?}"
        )));
    }

    let mut reader = image::ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| SpryteoError::InvalidInput(e.to_string()))?;

    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(SpryteoError::LimitExceeded {
            kind: LimitKind::Dimension,
            value: u64::from(width.max(height)),
            max: u64::from(MAX_DIMENSION),
        });
    }

    let pixel_count = u64::from(width) * u64::from(height);
    if pixel_count > opts.max_pixels {
        return Err(SpryteoError::LimitExceeded {
            kind: LimitKind::PixelCount,
            value: pixel_count,
            max: opts.max_pixels,
        });
    }

    let mut reader = image::ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    let img = reader
        .decode()
        .map_err(|e| SpryteoError::InvalidInput(e.to_string()))?;

    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let pixels = rgba.into_raw();

    Ok(RasterImage {
        width,
        height,
        pixels,
    })
}
