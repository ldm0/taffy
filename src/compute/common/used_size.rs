//! Shared resolution of final border-box sizes at layout-algorithm boundaries.

use crate::{AutoSizeBehavior, AvailableSpace, MaybeMath, Size, WritingMode};

/// Resolve the containing formatting context's policy for an authored
/// `inline-size: auto`.
///
/// This runs after preferred aspect-ratio transfer. Implicit stretch therefore
/// preserves a ratio-derived inline size, while explicit stretch receives an
/// unresolved inline size and fills the available space. A fallback available
/// size for orthogonal writing modes remains only a fit-content wrapping cap
/// when `behavior` is [`AutoSizeBehavior::FitContent`].
#[inline(always)]
pub(crate) fn resolve_inline_auto_size(
    preferred_size: Size<Option<f32>>,
    size_is_auto: Size<bool>,
    writing_mode: WritingMode,
    behavior: AutoSizeBehavior,
    available_space: Size<AvailableSpace>,
) -> Size<Option<f32>> {
    if behavior == AutoSizeBehavior::FitContent {
        return preferred_size;
    }

    let mut logical_preferred_size = writing_mode.to_logical(preferred_size);
    let logical_size_is_auto = writing_mode.to_logical(size_is_auto);
    let logical_available_space = writing_mode.to_logical(available_space);
    if logical_size_is_auto.inline_size && logical_preferred_size.inline_size.is_none() {
        logical_preferred_size.inline_size = match logical_available_space.inline_size {
            AvailableSpace::Definite(size) => Some(size),
            AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
        };
    }
    writing_mode.to_physical(logical_preferred_size)
}

/// Resolve one used border-box axis while preserving a fixed size supplied by
/// the parent formatting context.
///
/// A known dimension is already the result of the parent's sizing algorithm,
/// so the child must not apply its style min/max constraints again. The
/// padding-border floor remains an unconditional border-box invariant.
#[inline(always)]
pub(crate) fn resolve_used_axis(
    known_dimension: Option<f32>,
    synthesized_size: Option<f32>,
    min_size: Option<f32>,
    max_size: Option<f32>,
    minimum_border_box_size: f32,
) -> Option<f32> {
    known_dimension
        .map(|size| size.max(minimum_border_box_size))
        .or_else(|| synthesized_size.map(|size| size.maybe_clamp(min_size, max_size).max(minimum_border_box_size)))
}

/// Resolve synthesized preferred sizes without re-clamping dimensions that a
/// parent formatting context has already fixed.
///
/// `LayoutInput::known_dimensions` is an exact used border-box size, not a
/// style suggestion. Min/max constraints apply only to axes still synthesized
/// by the child algorithm; the structural padding/border floor applies to both.
#[inline(always)]
pub(crate) fn resolve_used_size(
    known_dimensions: Size<Option<f32>>,
    synthesized_size: Size<Option<f32>>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    minimum_border_box_size: Size<f32>,
) -> Size<Option<f32>> {
    Size {
        width: resolve_used_axis(
            known_dimensions.width,
            synthesized_size.width,
            min_size.width,
            max_size.width,
            minimum_border_box_size.width,
        ),
        height: resolve_used_axis(
            known_dimensions.height,
            synthesized_size.height,
            min_size.height,
            max_size.height,
            minimum_border_box_size.height,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_auto_policy_separates_fit_content_from_stretch() {
        let available = Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::MaxContent };
        let auto = Size { width: true, height: true };

        assert_eq!(
            resolve_inline_auto_size(
                Size { width: Some(120.0), height: None },
                auto,
                WritingMode::HorizontalTb,
                AutoSizeBehavior::StretchImplicit,
                available,
            ),
            Size { width: Some(120.0), height: None },
        );
        assert_eq!(
            resolve_inline_auto_size(
                Size::NONE,
                auto,
                WritingMode::HorizontalTb,
                AutoSizeBehavior::StretchExplicit,
                available,
            ),
            Size { width: Some(300.0), height: None },
        );
        assert_eq!(
            resolve_inline_auto_size(
                Size::NONE,
                auto,
                WritingMode::HorizontalTb,
                AutoSizeBehavior::FitContent,
                available,
            ),
            Size::NONE,
        );
    }

    #[test]
    fn known_used_axis_is_not_reclamped_by_child_constraints() {
        assert_eq!(resolve_used_axis(Some(100.0), Some(50.0), None, Some(50.0), 20.0), Some(100.0));
        assert_eq!(resolve_used_axis(None, Some(100.0), None, Some(50.0), 20.0), Some(50.0));
    }

    #[test]
    fn padding_and_border_floor_known_used_axis() {
        assert_eq!(resolve_used_axis(Some(12.0), None, None, Some(10.0), 22.0), Some(22.0));
    }
}
