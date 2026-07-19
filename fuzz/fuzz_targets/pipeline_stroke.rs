#![no_main]

use libfuzzer_sys::fuzz_target;
use spryteo_core::ConvertOptions;

// Full stroke-mode pipeline target: decode -> skeletonize -> trace ->
// emit. Separate from `pipeline` because stroke mode exercises an
// entirely different code path (spryteo-stroke) with its own graph
// traversal / width-estimation logic worth fuzzing independently.
fuzz_target!(|data: &[u8]| {
    let opts = ConvertOptions {
        stroke: true,
        ..ConvertOptions::default()
    };
    let _ = spryteo_cli::run_convert_stroke(data, &opts);
});
