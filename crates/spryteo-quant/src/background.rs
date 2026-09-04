use crate::color::{lab_distance_sq, srgb_to_lab};
use spryteo_core::ir::{LayerStack, Rgb};
use spryteo_core::options::Background;

/// Apply the background treatment policy to the LayerStack.
///
/// Under Drop/Rect, if a background color was detected, the quantized layer whose color is nearest
/// to it in Lab space (within a 10.0 Lab distance threshold) is removed from the LayerStack.
///
/// Returns `Some(background_color)` if the policy is Rect and a layer was successfully removed,
/// otherwise returns `None`.
pub fn apply_background_policy(
    stack: &mut LayerStack,
    background_color: Option<Rgb>,
    policy: &Background,
) -> Option<Rgb> {
    match policy {
        Background::Keep => None,
        Background::Drop | Background::Rect => {
            let bg_color = background_color?;
            let bg_lab = srgb_to_lab(&bg_color);

            let mut nearest_idx = None;
            let mut min_dist_sq = f64::INFINITY;

            for (i, layer) in stack.layers.iter().enumerate() {
                let layer_lab = srgb_to_lab(&layer.color);
                let dist_sq = lab_distance_sq(&bg_lab, &layer_lab);
                if dist_sq < min_dist_sq {
                    min_dist_sq = dist_sq;
                    nearest_idx = Some(i);
                }
            }

            // The threshold is 10.0 Lab distance.
            // 10.0^2 = 100.0
            if let Some(idx) = nearest_idx {
                if min_dist_sq <= 100.0 {
                    stack.layers.remove(idx);
                    if matches!(policy, Background::Rect) {
                        return Some(bg_color);
                    }
                }
            }
            None
        }
    }
}
#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
