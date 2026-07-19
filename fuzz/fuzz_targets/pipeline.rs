#![no_main]

use libfuzzer_sys::fuzz_target;
use spryteo_core::ConvertOptions;

// Full fill-mode pipeline target: decode -> classify -> preprocess ->
// quantize -> trace -> fit -> gradients -> scene graph -> emit. Never
// expected to panic on arbitrary bytes (ROADMAP.md §7's "generated-only
// output" / robustness guarantees).
fuzz_target!(|data: &[u8]| {
    let opts = ConvertOptions::default();
    let _ = spryteo_cli::run_convert(data, &opts);
});
