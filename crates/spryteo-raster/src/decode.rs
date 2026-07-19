use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
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
/// # EXIF orientation
///
/// If the source embeds an EXIF orientation tag, the decoded pixels are
/// rotated/flipped so the output is always upright (`image`'s
/// `ImageDecoder::orientation()` reads the tag; unsupported/absent formats
/// resolve to `NoTransforms`, a no-op).
///
/// # ICC profile → sRGB
///
/// If the source embeds an ICC profile whose declared color space is RGB,
/// pixels are transformed to sRGB via `moxcms` (pure-Rust, Apache-2.0; see
/// NOTICE) before any other processing. Profiles that fail to parse, that
/// declare a non-RGB color space (e.g. Gray/CMYK — out of scope, since
/// `image` only decodes RGB-family formats here), or for which a transform
/// cannot be built are left unconverted rather than failing the decode.
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
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| SpryteoError::InvalidInput(e.to_string()))?;

    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let icc_profile_bytes = decoder.icc_profile().ok().flatten();

    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|e| SpryteoError::InvalidInput(e.to_string()))?;

    img.apply_orientation(orientation);

    if let Some(icc_bytes) = icc_profile_bytes {
        convert_icc_to_srgb(&mut img, &icc_bytes);
    }

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

/// Converts `img` in place from the color space declared by `icc_bytes` to
/// sRGB. A no-op if the profile fails to parse, is not RGB-family, or no
/// transform can be built for it — see [`decode`]'s docs.
fn convert_icc_to_srgb(img: &mut DynamicImage, icc_bytes: &[u8]) {
    let Ok(src_profile) = ColorProfile::new_from_slice(icc_bytes) else {
        return;
    };
    if src_profile.color_space != DataColorSpace::Rgb {
        return;
    }
    let dst_profile = ColorProfile::new_srgb();
    let Ok(transform) = src_profile.create_transform_8bit(
        Layout::Rgba,
        &dst_profile,
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return;
    };

    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let src_raw = rgba.into_raw();
    let mut out = vec![0u8; src_raw.len()];
    if transform.transform(&src_raw, &mut out).is_err() {
        return;
    }
    if let Some(converted) = image::RgbaImage::from_raw(width, height, out) {
        *img = DynamicImage::ImageRgba8(converted);
    }
}

/// Whether `bytes` sniffs (by magic bytes, same as `decode`) as JPEG.
///
/// Exposed so callers can decide whether to run JPEG-specific
/// preprocessing (deblocking) without re-implementing format sniffing
/// or depending on the `image` crate directly.
pub fn is_jpeg(bytes: &[u8]) -> bool {
    matches!(image::guess_format(bytes), Ok(ImageFormat::Jpeg))
}
