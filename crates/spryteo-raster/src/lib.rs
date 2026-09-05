//! Decode (PNG/JPEG/GIF/WebP/BMP), EXIF orientation, ICC to sRGB, preprocessing.

pub mod alpha;
pub mod decode;
pub mod preprocess;
pub mod region;

pub use alpha::apply_alpha_mode;
pub use decode::{decode, is_jpeg};
pub use preprocess::{
    bilateral_filter, bilateral_filter_cancellable, deblock, downscale_factor,
    downscale_large_photo, downscale_to_max_dimension, estimate_jpeg_quality, preprocess,
    preprocess_cancellable,
};
pub use region::{crop, upscale_bicubic};
