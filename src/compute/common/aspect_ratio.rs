//! Shared preferred-size and min/max transfer rules for `aspect-ratio`.

use crate::{BoxSizing, ResolvedAspectRatio, Size};

/// Preferred and limiting sizes after applying a preferred aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedSizeConstraints {
    pub size: Size<Option<f32>>,
    pub min_size: Size<Option<f32>>,
    pub max_size: Size<Option<f32>>,
}

/// Apply a preferred aspect ratio to resolved border-box sizes and merge
/// min/max sizes transferred from the opposite axis with explicitly specified
/// constraints.
///
/// A constraint is transferred into an axis only while the preferred size in
/// that axis is `auto`. An explicit maximum caps a transferred minimum, and an
/// explicit minimum floors a transferred maximum. This retains the provenance
/// that would be lost by independently applying the ratio to `min_size` and
/// `max_size` and then using the generic "minimum wins" clamp.
pub(crate) fn resolve_size_constraints(
    size: Size<Option<f32>>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    size_is_auto: Size<bool>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> ResolvedSizeConstraints {
    let transferred_min = transferred_constraints(min_size, size_is_auto, aspect_ratio, padding_border);
    let transferred_max = transferred_constraints(max_size, size_is_auto, aspect_ratio, padding_border);

    let min_size = Size {
        width: merge_minimum(min_size.width, transferred_min.width, max_size.width),
        height: merge_minimum(min_size.height, transferred_min.height, max_size.height),
    };
    let max_size = Size {
        width: merge_maximum(max_size.width, transferred_max.width, min_size.width),
        height: merge_maximum(max_size.height, transferred_max.height, min_size.height),
    };

    ResolvedSizeConstraints {
        size: size.maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border),
        min_size,
        max_size,
    }
}

fn transferred_constraints(
    constraints: Size<Option<f32>>,
    target_is_auto: Size<bool>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<Option<f32>> {
    Size {
        width: target_is_auto
            .width
            .then(|| {
                Size { width: None, height: constraints.height }
                    .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
                    .width
            })
            .flatten(),
        height: target_is_auto
            .height
            .then(|| {
                Size { width: constraints.width, height: None }
                    .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
                    .height
            })
            .flatten(),
    }
}

fn merge_minimum(explicit_min: Option<f32>, transferred_min: Option<f32>, explicit_max: Option<f32>) -> Option<f32> {
    let transferred_min = match (transferred_min, explicit_max) {
        (Some(transferred), Some(maximum)) => Some(transferred.min(maximum)),
        (transferred, _) => transferred,
    };
    match (explicit_min, transferred_min) {
        (Some(explicit), Some(transferred)) => Some(explicit.max(transferred)),
        (Some(explicit), None) => Some(explicit),
        (None, transferred) => transferred,
    }
}

fn merge_maximum(explicit_max: Option<f32>, transferred_max: Option<f32>, used_min: Option<f32>) -> Option<f32> {
    let maximum = match (explicit_max, transferred_max) {
        (Some(explicit), Some(transferred)) => Some(explicit.min(transferred)),
        (Some(explicit), None) => Some(explicit),
        (None, transferred) => transferred,
    };
    match (maximum, used_min) {
        (Some(maximum), Some(minimum)) => Some(maximum.max(minimum)),
        (maximum, _) => maximum,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_constraints_win_over_conflicting_transferred_constraints() {
        let resolved = resolve_size_constraints(
            Size { width: Some(50.0), height: None },
            Size { width: Some(100.0), height: None },
            Size { width: None, height: Some(100.0) },
            Size { width: false, height: true },
            ResolvedAspectRatio { ratio: Some(0.5), box_sizing: BoxSizing::BorderBox },
            Size::ZERO,
        );

        assert_eq!(resolved.size, Size { width: Some(50.0), height: Some(100.0) });
        assert_eq!(resolved.min_size, Size { width: Some(100.0), height: Some(100.0) });
        assert_eq!(resolved.max_size, Size { width: None, height: Some(100.0) });
    }

    #[test]
    fn minimum_wins_when_the_transferred_pair_conflicts() {
        let resolved = resolve_size_constraints(
            Size::NONE,
            Size { width: None, height: Some(150.0) },
            Size { width: None, height: Some(100.0) },
            Size { width: true, height: true },
            ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::BorderBox },
            Size::ZERO,
        );

        assert_eq!(resolved.min_size.width, Some(300.0));
        assert_eq!(resolved.max_size.width, Some(300.0));
    }
}
