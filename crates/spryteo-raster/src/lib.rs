//! Decode (PNG/JPEG/GIF/WebP/BMP), EXIF orientation, ICC to sRGB, preprocessing.

pub mod decode;
pub mod preprocess;
pub mod region;

pub use decode::{decode, is_jpeg};
pub use preprocess::{
    bilateral_filter, deblock, downscale_large_photo, estimate_jpeg_quality, preprocess,
};
pub use region::{crop, upscale_bicubic};
