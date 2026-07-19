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

// ── EXIF orientation / ICC → sRGB ───────────────────────────────────────
//
// Fixtures generated with Pillow (see git history for the generating
// script); expected pixel values were independently cross-checked against
// Pillow's own `ImageOps.exif_transpose` (orientation) and
// `ImageCms.profileToProfile` (ICC), not just derived from this crate's
// own output.

#[test]
fn exif_orientation_6_rotates_to_upright() {
    // A 4x2 PNG with EXIF Orientation=6 (rotate 90 CW to display upright)
    // and four distinct quadrant colours, stored pre-rotation as:
    //   row0: red,  red,  green,  green
    //   row1: blue, blue, yellow, yellow
    // After correction it must become a 2x4 image:
    //   row0: blue,   red
    //   row1: blue,   red
    //   row2: yellow, green
    //   row3: yellow, green
    let bytes = include_bytes!("fixtures/exif_orientation_6.png");
    let opts = test_options();
    let result = spryteo_raster::decode(bytes, &opts).unwrap();

    assert_eq!(result.width, 2);
    assert_eq!(result.height, 4);

    let px = |x: u32, y: u32| -> [u8; 3] {
        let idx = ((y * result.width + x) * 4) as usize;
        [
            result.pixels[idx],
            result.pixels[idx + 1],
            result.pixels[idx + 2],
        ]
    };

    assert_eq!(px(0, 0), [0, 0, 255], "top-left should be blue");
    assert_eq!(px(1, 0), [255, 0, 0], "top-right should be red");
    assert_eq!(px(0, 1), [0, 0, 255], "should still be blue");
    assert_eq!(px(1, 1), [255, 0, 0], "should still be red");
    assert_eq!(px(0, 2), [255, 255, 0], "should be yellow");
    assert_eq!(px(1, 2), [0, 255, 0], "should be green");
    assert_eq!(px(0, 3), [255, 255, 0], "should still be yellow");
    assert_eq!(px(1, 3), [0, 255, 0], "should still be green");
}

#[test]
fn no_exif_orientation_is_a_no_op() {
    // Plain 4x4 PNG (no EXIF at all) must decode unchanged in shape.
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::Png);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.width, 4);
    assert_eq!(result.height, 4);
    assert_eq!(result.pixels, original.into_raw());
}

#[test]
fn icc_adobe_rgb_profile_is_converted_to_srgb() {
    // An 8x8 PNG whose raw stored pixel bytes are (136, 255, 51) but is
    // tagged with an embedded Adobe RGB (1998) ICC profile. Interpreted
    // correctly (converted to sRGB), that value must land at (0, 255, 0) --
    // independently confirmed via Pillow/LittleCMS
    // (ImageCms.profileToProfile, relative colorimetric intent). If ICC
    // conversion were skipped, decode would instead yield the raw stored
    // bytes (136, 255, 51).
    let bytes = include_bytes!("fixtures/adobe_rgb_green.png");
    let opts = test_options();
    let result = spryteo_raster::decode(bytes, &opts).unwrap();

    assert_eq!(result.width, 8);
    assert_eq!(result.height, 8);
    assert_eq!(&result.pixels[0..4], &[0, 255, 0, 255]);

    // Sanity: prove this is a real conversion, not coincidence -- the raw
    // stored bytes are NOT what we expect after conversion.
    assert_ne!(&result.pixels[0..3], &[136, 255, 51]);
}

#[test]
fn png_without_icc_profile_is_unaffected() {
    // No ICC profile present -- decode should return exactly the encoded
    // pixel bytes, same as the existing decode_png_roundtrip test.
    let opts = test_options();
    let original = make_4x4_rgba();
    let bytes = encode(&original, ImageFormat::Png);
    let result = spryteo_raster::decode(&bytes, &opts).unwrap();
    assert_eq!(result.pixels, original.into_raw());
}
