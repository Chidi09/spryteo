use spryteo_core::ir::{LayerStack, Rgb};
use spryteo_core::options::Background;
use crate::color::{srgb_to_lab, lab_distance_sq};

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
mod tests {
    use super::*;
    use spryteo_core::ir::Layer;

    fn make_test_layer(r: u8, g: u8, b: u8) -> Layer {
        Layer {
            mask: vec![255],
            color: Rgb { r, g, b },
            z_order: 0,
        }
    }

    #[test]
    fn test_drop_removes_nearest_layer_within_threshold() {
        let mut stack = LayerStack {
            layers: vec![
                make_test_layer(255, 0, 0),   // Red
                make_test_layer(0, 255, 0),   // Green
                make_test_layer(0, 0, 255),   // Blue
            ],
        };

        // Green-ish color very close to Green (0, 255, 0)
        let bg_color = Some(Rgb { r: 0, g: 250, b: 0 });
        let result = apply_background_policy(&mut stack, bg_color, &Background::Drop);

        assert_eq!(result, None);
        assert_eq!(stack.layers.len(), 2);
        // The green layer should be removed
        assert_eq!(stack.layers[0].color, Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(stack.layers[1].color, Rgb { r: 0, g: 0, b: 255 });
    }

    #[test]
    fn test_rect_removes_nearest_layer_and_returns_color() {
        let mut stack = LayerStack {
            layers: vec![
                make_test_layer(255, 0, 0),   // Red
                make_test_layer(0, 255, 0),   // Green
                make_test_layer(0, 0, 255),   // Blue
            ],
        };

        // Green-ish color very close to Green (0, 255, 0)
        let bg_color = Some(Rgb { r: 0, g: 250, b: 0 });
        let result = apply_background_policy(&mut stack, bg_color, &Background::Rect);

        assert_eq!(result, Some(Rgb { r: 0, g: 250, b: 0 }));
        assert_eq!(stack.layers.len(), 2);
        // The green layer should be removed
        assert_eq!(stack.layers[0].color, Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(stack.layers[1].color, Rgb { r: 0, g: 0, b: 255 });
    }

    #[test]
    fn test_noop_when_nothing_within_threshold() {
        let mut stack = LayerStack {
            layers: vec![
                make_test_layer(255, 0, 0),
                make_test_layer(0, 255, 0),
                make_test_layer(0, 0, 255),
            ],
        };

        // White color, far from Red, Green, Blue in Lab space
        let bg_color = Some(Rgb { r: 255, g: 255, b: 255 });
        let result = apply_background_policy(&mut stack, bg_color, &Background::Drop);

        assert_eq!(result, None);
        assert_eq!(stack.layers.len(), 3);
    }

    #[test]
    fn test_noop_when_background_color_is_none() {
        let mut stack = LayerStack {
            layers: vec![
                make_test_layer(255, 0, 0),
                make_test_layer(0, 255, 0),
                make_test_layer(0, 0, 255),
            ],
        };

        let result = apply_background_policy(&mut stack, None, &Background::Drop);

        assert_eq!(result, None);
        assert_eq!(stack.layers.len(), 3);
    }

    #[test]
    fn test_keep_never_modifies_stack() {
        let mut stack = LayerStack {
            layers: vec![
                make_test_layer(255, 0, 0),
                make_test_layer(0, 255, 0),
                make_test_layer(0, 0, 255),
            ],
        };

        // Exact match with Green
        let bg_color = Some(Rgb { r: 0, g: 255, b: 0 });
        let result = apply_background_policy(&mut stack, bg_color, &Background::Keep);

        assert_eq!(result, None);
        assert_eq!(stack.layers.len(), 3);
    }
}
