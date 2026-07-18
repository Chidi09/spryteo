use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbaImage};
use spryteo_core::{ConvertOptions, LimitKind, SpryteoError};

fn test_options() -> ConvertOptions {
    ConvertOptions {
        max_pixels: 16_000_000,
        max_input_bytes: 8_000_000,
        ..Default::default()
    }
}

/// Encode an RGBA image to bytes in the given format.
fn encode(img: &RgbaImage, fmt: ImageFormat) -> Vec<u8> {
    let dyn_img = DynamicImage::from(img.clone());
    let mut buf = Vec::new();
    dyn_img
        .write_to(&mut Cursor::new(&mut buf), fmt)
        .expect("encoding should succeed");
    buf
}

fn make_4x4_rgba() -> RgbaImage {
    let mut img = RgbaImage::new(4, 4);
    // Fill with a simple pattern.
    for y in 0..4 {
        for x in 0..4 {
            let r = (x * 64) as u8;
            let g = (y * 64) as u8;
            let b = 128u8;
            let a = if (x + y) % 2 == 0 { 255 } else { 128 };
            img.put_pixel(x, y, image::Rgba([r, g, b, a]));
        }
    }
    img
}

// ── Round-trip tests ────────────────────────────────────────────────────

#[test]
fn decode_png_roundtrip() {
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::Png);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.width, 4);
    assert_eq!(result.height, 4);
    assert_eq!(result.pixels.len(), 4 * 4 * 4);
    assert_eq!(result.pixels, original.into_raw());
}

#[test]
fn decode_bmp_roundtrip() {
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::Bmp);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.width, 4);
    assert_eq!(result.height, 4);
    assert_eq!(result.pixels.len(), 4 * 4 * 4);
    // BMP is lossless — pixels should match.
    assert_eq!(result.pixels, original.into_raw());
}

#[test]
fn decode_jpeg_dimensions() {
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::Jpeg);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.width, 4);
    assert_eq!(result.height, 4);
    // JPEG is lossy — don't check pixel values.
}

#[test]
fn decode_gif_dimensions() {
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::Gif);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.width, 4);
    assert_eq!(result.height, 4);
    // GIF uses indexed colour — pixel values may differ.
}

#[test]
fn decode_webp_dimensions() {
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::WebP);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.width, 4);
    assert_eq!(result.height, 4);
    // WebP may be lossy — don't check pixel values.
}

// ── Limit tests ─────────────────────────────────────────────────────────

#[test]
fn reject_exceeding_max_input_bytes() {
    let bytes = encode(&make_4x4_rgba(), ImageFormat::Png);
    let opts = ConvertOptions {
        max_input_bytes: 1,
        ..Default::default()
    };
    let err = spryteo_raster::decode(&bytes, &opts).unwrap_err();
    match err {
        SpryteoError::LimitExceeded {
            kind: LimitKind::InputBytes,
            value,
            max,
        } => {
            assert_eq!(max, 1);
            assert!(value > 1);
        }
        other => panic!("expected LimitExceeded(InputBytes), got {other:?}"),
    }
}

#[test]
fn reject_exceeding_max_pixels() {
    let bytes = encode(&make_4x4_rgba(), ImageFormat::Png);
    let opts = ConvertOptions {
        max_pixels: 15,
        ..Default::default()
    };
    let err = spryteo_raster::decode(&bytes, &opts).unwrap_err();
    match err {
        SpryteoError::LimitExceeded {
            kind: LimitKind::PixelCount,
            value: 16,
            max: 15,
        } => {}
        other => panic!("expected LimitExceeded(PixelCount), got {other:?}"),
    }
}

#[test]
fn reject_exceeding_dimension_ceiling() {
    // Create an image wider than the 8192px ceiling.
    let mut huge = RgbaImage::new(8193, 1);
    huge.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
    let bytes = encode(&huge, ImageFormat::Png);
    let opts = test_options();
    let err = spryteo_raster::decode(&bytes, &opts).unwrap_err();
    match err {
        SpryteoError::LimitExceeded {
            kind: LimitKind::Dimension,
            value: 8193,
            max: 8192,
        } => {}
        other => panic!("expected LimitExceeded(Dimension), got {other:?}"),
    }
}

// ── Garbage / unsupported input ─────────────────────────────────────────

#[test]
fn reject_garbage_bytes() {
    let garbage = b"this is not a valid image of any kind, obviously";
    let opts = test_options();
    let err = spryteo_raster::decode(garbage, &opts).unwrap_err();
    assert!(matches!(err, SpryteoError::InvalidInput(_)));
}

#[test]
fn reject_unsupported_format() {
    // Minimal TIFF header — valid magic bytes for an unsupported format.
    let tiff_header = b"II\x2a\x00\x08\x00\x00\x00some more data";
    let opts = test_options();
    let err = spryteo_raster::decode(tiff_header, &opts).unwrap_err();
    assert!(matches!(err, SpryteoError::InvalidInput(_)));
}
