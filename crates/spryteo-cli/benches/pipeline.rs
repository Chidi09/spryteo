use criterion::{criterion_group, criterion_main, Criterion};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use spryteo_cli::run_convert;
use spryteo_core::{ColorSpec, ConvertOptions, Mode};
use std::io::Cursor;

fn generate_icon_256() -> Vec<u8> {
    let mut img = RgbaImage::new(256, 256);
    for pixel in img.pixels_mut() {
        *pixel = Rgba([255, 255, 255, 255]);
    }
    // Shape 1: Red circle
    for y in 0..256 {
        for x in 0..256 {
            let dx = x as f32 - 128.0;
            let dy = y as f32 - 128.0;
            if dx * dx + dy * dy <= 80.0 * 80.0 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
    }
    // Shape 2: Green rect
    for y in 40..100 {
        for x in 40..100 {
            img.put_pixel(x, y, Rgba([0, 255, 0, 255]));
        }
    }
    // Shape 3: Blue circle
    for y in 0..256 {
        for x in 0..256 {
            let dx = x as f32 - 200.0;
            let dy = y as f32 - 60.0;
            if dx * dx + dy * dy <= 30.0 * 30.0 {
                img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
    }
    // Shape 4: Yellow rect
    for y in 160..220 {
        for x in 160..220 {
            img.put_pixel(x, y, Rgba([255, 255, 0, 255]));
        }
    }

    let mut png_bytes = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .unwrap();
    png_bytes
}

fn generate_photo_1024() -> Vec<u8> {
    let mut img = RgbaImage::new(1024, 1024);
    for y in 0..1024 {
        for x in 0..1024 {
            let r = (x as f32 / 1024.0 * 255.0) as u8;
            let g = (y as f32 / 1024.0 * 255.0) as u8;
            let b = (((x + y) as f32 / 2048.0) * 255.0) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    let mut png_bytes = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .unwrap();
    png_bytes
}

fn generate_photo_2048() -> Vec<u8> {
    let mut img = RgbaImage::new(2048, 2048);
    for y in 0..2048 {
        for x in 0..2048 {
            let r = (x as f32 / 2048.0 * 255.0) as u8;
            let g = (y as f32 / 2048.0 * 255.0) as u8;
            let b = (((x + y) as f32 / 4096.0) * 255.0) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    let mut png_bytes = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .unwrap();
    png_bytes
}

fn bench_pipeline(c: &mut Criterion) {
    let icon_256_bytes = generate_icon_256();
    let photo_1024_bytes = generate_photo_1024();
    let photo_2048_bytes = generate_photo_2048();

    let mut group = c.benchmark_group("pipeline");
    // Reduce sample size to 10 for the entire group to avoid excessively long benchmark runs
    group.sample_size(10);

    // 1. icon_256_8colors: < 50ms (ROADMAP.md §8)
    group.bench_function("icon_256_8colors", |b| {
        let opts = ConvertOptions {
            mode: Mode::Icon,
            colors: ColorSpec::N(8),
            ..Default::default()
        };
        b.iter(|| {
            let _ = run_convert(&icon_256_bytes, &opts).unwrap();
        });
    });

    // 2. photo_1024_12colors: < 1.5s (ROADMAP.md §8)
    group.bench_function("photo_1024_12colors", |b| {
        let opts = ConvertOptions {
            mode: Mode::Photo,
            colors: ColorSpec::N(12),
            ..Default::default()
        };
        b.iter(|| {
            let _ = run_convert(&photo_1024_bytes, &opts).unwrap();
        });
    });

    // 3. photo_2048_downscaled: < 2s (ROADMAP.md §8, after Part 1's auto-downscale-to-1024)
    group.bench_function("photo_2048_downscaled", |b| {
        let opts = ConvertOptions {
            mode: Mode::Photo,
            ..Default::default()
        };
        b.iter(|| {
            let _ = run_convert(&photo_2048_bytes, &opts).unwrap();
        });
    });

    group.finish();
}

criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
