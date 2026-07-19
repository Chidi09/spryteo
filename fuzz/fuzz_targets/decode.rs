#![no_main]

use libfuzzer_sys::fuzz_target;
use spryteo_core::ConvertOptions;

// Decode-only target: exercises spryteo_raster::decode against arbitrary
// bytes. Never expected to panic -- malformed/adversarial/truncated image
// data, decompression bombs, and non-image bytes should all surface as
// Err(SpryteoError), not a crash (ROADMAP.md §7).
fuzz_target!(|data: &[u8]| {
    let opts = ConvertOptions::default();
    let _ = spryteo_raster::decode(data, &opts);
});
