use super::*;

fn img(pixels: Vec<u8>) -> RasterImage {
    RasterImage {
        width: (pixels.len() / 4) as u32,
        height: 1,
        pixels,
    }
}

const MAGENTA: Rgb = Rgb {
    r: 255,
    g: 0,
    b: 255,
};

#[test]
fn keep_is_the_identity() {
    let original = vec![10, 20, 30, 40, 50, 60, 70, 0];
    let out = apply_alpha_mode(img(original.clone()), &AlphaMode::Keep);
    assert_eq!(out.pixels, original);
}

#[test]
fn matte_makes_every_pixel_opaque() {
    let out = apply_alpha_mode(
        img(vec![10, 20, 30, 0, 10, 20, 30, 128, 10, 20, 30, 255]),
        &AlphaMode::Matte(MAGENTA),
    );
    for px in out.pixels.chunks_exact(4) {
        assert_eq!(px[3], 255, "matte must produce a fully opaque image");
    }
}

#[test]
fn matte_replaces_rgb_under_fully_transparent_pixels() {
    // Encoders leave arbitrary RGB under alpha==0; it must not survive.
    let out = apply_alpha_mode(img(vec![7, 200, 3, 0]), &AlphaMode::Matte(MAGENTA));
    assert_eq!(&out.pixels[0..3], &[255, 0, 255]);
}

#[test]
fn matte_leaves_fully_opaque_pixels_bit_exact() {
    let out = apply_alpha_mode(img(vec![10, 20, 30, 255]), &AlphaMode::Matte(MAGENTA));
    assert_eq!(&out.pixels[0..3], &[10, 20, 30]);
}

#[test]
fn matte_composites_half_alpha_at_the_midpoint() {
    // black over white at alpha 128: 0*(128/255) + 255*(1-128/255) = 127.0
    let white = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    let out = apply_alpha_mode(img(vec![0, 0, 0, 128]), &AlphaMode::Matte(white));
    assert_eq!(&out.pixels[0..3], &[127, 127, 127]);
}

#[test]
fn matte_is_monotonic_in_alpha() {
    // Compositing black over white must darken monotonically as alpha rises.
    let white = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    let mut last = 256i32;
    for a in [0u8, 32, 64, 128, 192, 255] {
        let out = apply_alpha_mode(img(vec![0, 0, 0, a]), &AlphaMode::Matte(white));
        let v = out.pixels[0] as i32;
        assert!(v < last, "alpha {a} produced {v}, not darker than {last}");
        last = v;
    }
}

#[test]
fn threshold_is_binary_and_keeps_rgb() {
    let out = apply_alpha_mode(
        img(vec![1, 2, 3, 0, 4, 5, 6, 127, 7, 8, 9, 128, 1, 1, 1, 255]),
        &AlphaMode::Threshold(128),
    );
    let alphas: Vec<u8> = out.pixels.chunks_exact(4).map(|p| p[3]).collect();
    assert_eq!(alphas, vec![0, 0, 255, 255], "alpha >= t is opaque");
    // RGB is untouched by thresholding.
    assert_eq!(&out.pixels[0..3], &[1, 2, 3]);
    assert_eq!(&out.pixels[8..11], &[7, 8, 9]);
}

#[test]
fn threshold_boundary_is_inclusive() {
    let out = apply_alpha_mode(img(vec![0, 0, 0, 128]), &AlphaMode::Threshold(128));
    assert_eq!(out.pixels[3], 255, "alpha == t must be opaque");
}

#[test]
fn threshold_zero_makes_everything_opaque() {
    let out = apply_alpha_mode(img(vec![0, 0, 0, 0, 0, 0, 0, 1]), &AlphaMode::Threshold(0));
    assert!(out.pixels.chunks_exact(4).all(|p| p[3] == 255));
}

#[test]
fn threshold_255_keeps_only_fully_opaque() {
    let out = apply_alpha_mode(
        img(vec![0, 0, 0, 254, 0, 0, 0, 255]),
        &AlphaMode::Threshold(255),
    );
    let alphas: Vec<u8> = out.pixels.chunks_exact(4).map(|p| p[3]).collect();
    assert_eq!(alphas, vec![0, 255]);
}

#[test]
fn threshold_is_idempotent() {
    let once = apply_alpha_mode(img(vec![1, 2, 3, 100]), &AlphaMode::Threshold(128));
    let twice = apply_alpha_mode(once.clone(), &AlphaMode::Threshold(128));
    assert_eq!(once.pixels, twice.pixels);
}

#[test]
fn matte_is_idempotent() {
    let once = apply_alpha_mode(img(vec![1, 2, 3, 100]), &AlphaMode::Matte(MAGENTA));
    let twice = apply_alpha_mode(once.clone(), &AlphaMode::Matte(MAGENTA));
    assert_eq!(once.pixels, twice.pixels);
}

#[test]
fn distinct_matte_colors_produce_distinct_output() {
    // Regression guard for issue #5: the three alpha modes produced
    // byte-identical SVGs because the option was never read at all.
    let black = Rgb { r: 0, g: 0, b: 0 };
    let a = apply_alpha_mode(img(vec![10, 10, 10, 0]), &AlphaMode::Matte(MAGENTA));
    let b = apply_alpha_mode(img(vec![10, 10, 10, 0]), &AlphaMode::Matte(black));
    assert_ne!(a.pixels, b.pixels);
}

#[test]
fn dimensions_are_preserved_by_every_mode() {
    let src = img(vec![1, 2, 3, 128, 4, 5, 6, 200]);
    for mode in [
        AlphaMode::Keep,
        AlphaMode::Matte(MAGENTA),
        AlphaMode::Threshold(100),
    ] {
        let out = apply_alpha_mode(src.clone(), &mode);
        assert_eq!((out.width, out.height), (src.width, src.height));
        assert_eq!(out.pixels.len(), src.pixels.len());
    }
}
