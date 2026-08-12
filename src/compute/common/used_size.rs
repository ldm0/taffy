//! Shared resolution of final border-box sizes at layout-algorithm boundaries.

use crate::{AutoSizeBehavior, AvailableSpace, BoxSizing, MaybeMath, ResolvedAspectRatio, Size, WritingMode};

/// Child-owned inline-size preference resolved from a constraint space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InlineAutoSizeResolution {
    /// Preferred sizes after resolving the logical inline axis.
    pub size: Size<Option<f32>>,
    /// Whether the logical inline axis was synthesized through the preferred
    /// aspect ratio.
    pub aspect_ratio_applied: Size<bool>,
}

/// Constraint-space state used to resolve an automatic inline-size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InlineAutoSizeInput {
    /// Child-owned preferred border-box size before auto-size resolution.
    pub preferred_size: Size<Option<f32>>,
    /// Parent-owned fixed border-box size, used only as a ratio basis.
    pub fixed_size: Size<Option<f32>>,
    /// Whether each authored preferred size is `auto`.
    pub size_is_auto: Size<bool>,
    /// Writing mode that defines the inline and block axes.
    pub writing_mode: WritingMode,
    /// Constraint-space policy for automatic inline sizing.
    pub inline_behavior: AutoSizeBehavior,
    /// Constraint-space policy for automatic block sizing.
    pub block_behavior: AutoSizeBehavior,
    /// Available border-box space supplied by the formatting context.
    pub available_space: Size<AvailableSpace>,
    /// Resolved minimum border-box constraints.
    pub min_size: Size<Option<f32>>,
    /// Resolved maximum border-box constraints.
    pub max_size: Size<Option<f32>>,
    /// Physical padding-and-border floor.
    pub minimum_border_box_size: Size<f32>,
    /// Preferred aspect ratio and its sizing box.
    pub aspect_ratio: ResolvedAspectRatio,
}

/// Resolve the automatic inline-size preference from the constraint space.
///
/// A fixed block size or explicit block stretch is available as a preferred
/// ratio basis before weak inline stretch. The block basis itself remains
/// owned by the parent formatting context; only a ratio-derived inline size is
/// copied into the child preference. Block-size resolution remains a later
/// operation so it can observe the final clamped inline size.
///
/// This mirrors Blink's `ComputeInlineSizeForFragmentInternal`: a strong block
/// stretch makes the ratio applicable before weak inline stretch, while two
/// weak stretches prefer the inline axis.
#[inline(always)]
pub(crate) fn resolve_inline_auto_size_preference(input: InlineAutoSizeInput) -> InlineAutoSizeResolution {
    let InlineAutoSizeInput {
        preferred_size,
        fixed_size,
        size_is_auto,
        writing_mode,
        inline_behavior,
        block_behavior,
        available_space,
        min_size,
        max_size,
        minimum_border_box_size,
        aspect_ratio,
    } = input;
    let mut logical_preferred = writing_mode.to_logical(preferred_size);
    let logical_fixed = writing_mode.to_logical(fixed_size);
    let logical_size_is_auto = writing_mode.to_logical(size_is_auto);
    let logical_available = writing_mode.to_logical(available_space);
    let logical_min = writing_mode.to_logical(min_size);
    let logical_max = writing_mode.to_logical(max_size);
    let logical_minimum_border_box_size = writing_mode.to_logical(minimum_border_box_size);
    let mut logical_ratio_applied = writing_mode.to_logical(Size { width: false, height: false });

    let explicit_block_stretch =
        if block_behavior == AutoSizeBehavior::StretchExplicit && logical_size_is_auto.block_size {
            match logical_available.block_size {
                AvailableSpace::Definite(size) => Some(
                    size.maybe_clamp(logical_min.block_size, logical_max.block_size)
                        .max(logical_minimum_border_box_size.block_size),
                ),
                AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
            }
        } else {
            None
        };

    let ratio_basis = crate::geometry::LogicalSize {
        inline_size: logical_fixed.inline_size.or(logical_preferred.inline_size),
        block_size: logical_fixed.block_size.or(logical_preferred.block_size).or(explicit_block_stretch),
    };
    let physical_ratio_basis = writing_mode.to_physical(ratio_basis);
    let ratio_resolved = writing_mode.to_logical(physical_ratio_basis.maybe_apply_aspect_ratio_with_box_sizing(
        aspect_ratio,
        BoxSizing::BorderBox,
        minimum_border_box_size,
    ));
    if inline_behavior != AutoSizeBehavior::StretchExplicit
        && ratio_basis.inline_size.is_none()
        && ratio_resolved.inline_size.is_some()
    {
        logical_preferred.inline_size = ratio_resolved.inline_size;
        logical_ratio_applied.inline_size = true;
    }

    if inline_behavior != AutoSizeBehavior::FitContent
        && logical_size_is_auto.inline_size
        && logical_fixed.inline_size.is_none()
        && logical_preferred.inline_size.is_none()
    {
        logical_preferred.inline_size = match logical_available.inline_size {
            AvailableSpace::Definite(size) => Some(
                size.maybe_clamp(logical_min.inline_size, logical_max.inline_size)
                    .max(logical_minimum_border_box_size.inline_size),
            ),
            AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
        };
    }

    InlineAutoSizeResolution {
        size: writing_mode.to_physical(logical_preferred),
        aspect_ratio_applied: writing_mode.to_physical(logical_ratio_applied),
    }
}

/// Resolve one used border-box axis while preserving a fixed size supplied by
/// the parent formatting context.
///
/// A known dimension is already the result of the parent's sizing algorithm.
/// Only an axis that this child still has to synthesize participates in its
/// own min/max and padding-border floor.
#[inline(always)]
pub(crate) fn resolve_used_axis(
    known_dimension: Option<f32>,
    synthesized_size: Option<f32>,
    min_size: Option<f32>,
    max_size: Option<f32>,
    minimum_border_box_size: f32,
) -> Option<f32> {
    known_dimension
        .or_else(|| synthesized_size.map(|size| size.maybe_clamp(min_size, max_size).max(minimum_border_box_size)))
}

/// Resolve synthesized preferred sizes without re-clamping dimensions that a
/// parent formatting context has already fixed.
///
/// `LayoutInput::known_dimensions` is an exact used border-box size, not a
/// style suggestion. Min/max constraints and the padding/border floor apply
/// only to axes still synthesized by the child algorithm.
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

    fn resolve_auto_size(
        preferred_size: Size<Option<f32>>,
        fixed_size: Size<Option<f32>>,
        inline_behavior: AutoSizeBehavior,
        block_behavior: AutoSizeBehavior,
        available_space: Size<AvailableSpace>,
        max_size: Size<Option<f32>>,
        ratio: Option<f32>,
    ) -> InlineAutoSizeResolution {
        resolve_inline_auto_size_preference(InlineAutoSizeInput {
            preferred_size,
            fixed_size,
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_behavior,
            block_behavior,
            available_space,
            min_size: Size::NONE,
            max_size,
            minimum_border_box_size: Size::ZERO,
            aspect_ratio: ResolvedAspectRatio { ratio, box_sizing: BoxSizing::BorderBox },
        })
    }

    #[test]
    fn auto_size_policy_preserves_ratio_before_implicit_stretch() {
        let available = Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::MaxContent };

        assert_eq!(
            resolve_auto_size(
                Size { width: Some(120.0), height: None },
                Size::NONE,
                AutoSizeBehavior::StretchImplicit,
                AutoSizeBehavior::FitContent,
                available,
                Size::NONE,
                None,
            )
            .size,
            Size { width: Some(120.0), height: None },
        );
        assert_eq!(
            resolve_auto_size(
                Size::NONE,
                Size::NONE,
                AutoSizeBehavior::StretchExplicit,
                AutoSizeBehavior::FitContent,
                available,
                Size::NONE,
                None,
            )
            .size,
            Size { width: Some(300.0), height: None },
        );
        assert_eq!(
            resolve_auto_size(
                Size::NONE,
                Size::NONE,
                AutoSizeBehavior::FitContent,
                AutoSizeBehavior::FitContent,
                available,
                Size::NONE,
                None,
            )
            .size,
            Size::NONE,
        );
    }

    #[test]
    fn explicit_block_stretch_is_a_ratio_basis_before_implicit_inline_stretch() {
        let available = Size { width: AvailableSpace::Definite(50.0), height: AvailableSpace::Definite(100.0) };
        let resolved = resolve_auto_size(
            Size::NONE,
            Size::NONE,
            AutoSizeBehavior::StretchImplicit,
            AutoSizeBehavior::StretchExplicit,
            available,
            Size::NONE,
            Some(1.0),
        );

        assert_eq!(resolved.size, Size { width: Some(100.0), height: None });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: true, height: false });

        let clamped = resolve_auto_size(
            Size::NONE,
            Size::NONE,
            AutoSizeBehavior::StretchImplicit,
            AutoSizeBehavior::StretchExplicit,
            Size { width: AvailableSpace::Definite(50.0), height: AvailableSpace::Definite(200.0) },
            Size { width: None, height: Some(100.0) },
            Some(1.0),
        );
        assert_eq!(clamped.size.width, Some(100.0));
    }

    #[test]
    fn fixed_block_size_is_a_ratio_basis_without_becoming_child_owned() {
        let resolved = resolve_auto_size(
            Size::NONE,
            Size { width: None, height: Some(80.0) },
            AutoSizeBehavior::StretchImplicit,
            AutoSizeBehavior::FitContent,
            Size { width: AvailableSpace::Definite(50.0), height: AvailableSpace::MaxContent },
            Size::NONE,
            Some(2.0),
        );

        assert_eq!(resolved.size, Size { width: Some(160.0), height: None });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: true, height: false });
    }

    #[test]
    fn two_implicit_stretches_leave_block_resolution_for_the_final_inline_size() {
        let resolved = resolve_auto_size(
            Size::NONE,
            Size::NONE,
            AutoSizeBehavior::StretchImplicit,
            AutoSizeBehavior::StretchImplicit,
            Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(50.0) },
            Size::NONE,
            Some(1.0),
        );

        assert_eq!(resolved.size, Size { width: Some(100.0), height: None });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: false, height: false });
    }

    #[test]
    fn known_used_axis_is_not_reclamped_by_child_constraints() {
        assert_eq!(resolve_used_axis(Some(100.0), Some(50.0), None, Some(50.0), 120.0), Some(100.0));
        assert_eq!(resolve_used_axis(None, Some(100.0), None, Some(50.0), 20.0), Some(50.0));
    }
}
