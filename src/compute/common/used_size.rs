//! Shared resolution of final border-box sizes at layout-algorithm boundaries.

use crate::{AutoSizeBehavior, AvailableSpace, Dimension, MaybeMath, Rect, Size, WritingMode};

/// Remove the margin sides that participate in explicit `stretch` sizing from
/// a formatting context's physical margin-box opportunity.
///
/// The ignored-side mask is deliberately applied here rather than by the
/// parent algorithm. This keeps [`crate::LayoutInput::available_space`] a
/// margin-box constraint for every child and ensures ordinary auto and
/// fit-content sizing continue to account for all margins.
#[inline(always)]
pub fn stretch_border_box_available_space(
    available_margin_box_space: Size<AvailableSpace>,
    margins: Rect<f32>,
    ignored_margins: Rect<bool>,
) -> Size<AvailableSpace> {
    let accounted_margins = Rect {
        left: if ignored_margins.left { 0.0 } else { margins.left },
        right: if ignored_margins.right { 0.0 } else { margins.right },
        top: if ignored_margins.top { 0.0 } else { margins.top },
        bottom: if ignored_margins.bottom { 0.0 } else { margins.bottom },
    };
    available_margin_box_space.maybe_sub(accounted_margins.sum_axes())
}

/// Which authored sizing property is being resolved.
///
/// An indefinite `stretch` opportunity has different fallback semantics for
/// the preferred size and the two limiting constraints. Keeping that choice
/// typed prevents formatting contexts from open-coding subtly different
/// behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeConstraintRole {
    /// `width` or `height`.
    Preferred,
    /// `min-width` or `min-height`.
    Minimum,
    /// `max-width` or `max-height`.
    Maximum,
}

/// Resolve one authored `stretch` value against its margin-adjusted
/// border-box opportunity.
#[inline(always)]
pub(crate) fn resolve_stretch_axis_value(
    value: Dimension,
    role: SizeConstraintRole,
    available_border_box_size: AvailableSpace,
    minimum_border_box_size: f32,
) -> Option<f32> {
    resolve_stretch_axis(value.is_stretch(), role, available_border_box_size.into_option(), minimum_border_box_size)
}

/// Resolve captured stretch provenance without reconstructing a `Dimension`.
#[inline(always)]
fn resolve_stretch_axis(
    is_stretch: bool,
    role: SizeConstraintRole,
    available_border_box_size: Option<f32>,
    minimum_border_box_size: f32,
) -> Option<f32> {
    if !is_stretch {
        return None;
    }

    match role {
        SizeConstraintRole::Preferred | SizeConstraintRole::Maximum => {
            available_border_box_size.map(|size| size.max(minimum_border_box_size))
        }
        SizeConstraintRole::Minimum => Some(available_border_box_size.unwrap_or(0.0).max(minimum_border_box_size)),
    }
}

/// Preferred and limiting border-box sizes resolved from authored `stretch`
/// sizing values.
///
/// A formatting context supplies the available border-box opportunity after
/// margins. Keeping these results separate from ordinary length resolution
/// preserves the distinction between an unresolved `stretch` value and an
/// authored `auto` size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ResolvedStretchSizeConstraints {
    /// Preferred border-box sizes resolved from `stretch`.
    pub preferred: Size<Option<f32>>,
    /// Minimum border-box sizes resolved from `stretch`.
    pub min: Size<Option<f32>>,
    /// Maximum border-box sizes resolved from `stretch`.
    pub max: Size<Option<f32>>,
}

/// Authored axes whose preferred, minimum, or maximum size uses `stretch`.
///
/// Flex layout retains this provenance until the final line cross size is
/// known. Other formatting contexts normally resolve it directly at their
/// child sizing boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StretchSizeProperties {
    /// Preferred physical axes authored as `stretch`.
    preferred: Size<bool>,
    /// Minimum physical axes authored as `stretch`.
    min: Size<bool>,
    /// Maximum physical axes authored as `stretch`.
    max: Size<bool>,
}

impl StretchSizeProperties {
    /// Capture `stretch` provenance before ordinary length resolution erases
    /// all unresolved sizing keywords into `None`.
    #[inline(always)]
    pub(crate) fn new(preferred: Size<Dimension>, min: Size<Dimension>, max: Size<Dimension>) -> Self {
        Self {
            preferred: preferred.map(Dimension::is_stretch),
            min: min.map(Dimension::is_stretch),
            max: max.map(Dimension::is_stretch),
        }
    }

    /// Resolve the captured properties against the available border-box
    /// opportunity in each physical axis.
    ///
    /// Preferred and maximum `stretch` remain unresolved when the available
    /// size is indefinite. An indefinite minimum `stretch` contributes zero,
    /// still floored by padding and border. This mirrors the common
    /// inline/block length resolution used by browser engines.
    #[inline(always)]
    pub(crate) fn resolve(
        self,
        available_border_box_size: Size<AvailableSpace>,
        minimum_border_box_size: Size<f32>,
    ) -> ResolvedStretchSizeConstraints {
        let available = available_border_box_size.map(AvailableSpace::into_option);
        let input = available.zip_map(minimum_border_box_size, |available, minimum| (available, minimum));

        ResolvedStretchSizeConstraints {
            preferred: self.preferred.zip_map(input, |value, (available, minimum)| {
                resolve_stretch_axis(value, SizeConstraintRole::Preferred, available, minimum)
            }),
            min: self.min.zip_map(input, |value, (available, minimum)| {
                resolve_stretch_axis(value, SizeConstraintRole::Minimum, available, minimum)
            }),
            max: self.max.zip_map(input, |value, (available, minimum)| {
                resolve_stretch_axis(value, SizeConstraintRole::Maximum, available, minimum)
            }),
        }
    }
}

/// Resolve the containing formatting context's policy for an authored
/// `inline-size: auto`.
///
/// This runs after preferred aspect-ratio transfer. Implicit stretch therefore
/// preserves a ratio-derived inline size, while explicit stretch receives an
/// unresolved inline size and fills the available space. A fallback available
/// size for orthogonal writing modes remains only a fit-content wrapping cap
/// when `behavior` is content-based.
#[inline(always)]
pub(crate) fn resolve_inline_auto_size(
    preferred_size: Size<Option<f32>>,
    size_is_auto: Size<bool>,
    writing_mode: WritingMode,
    behavior: AutoSizeBehavior,
    available_space: Size<AvailableSpace>,
) -> Size<Option<f32>> {
    if behavior.is_fit_content() {
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

    #[test]
    fn stretch_constraints_resolve_definite_and_indefinite_axes() {
        let properties = StretchSizeProperties::new(
            Size { width: Dimension::stretch(), height: Dimension::auto() },
            Size { width: Dimension::auto(), height: Dimension::stretch() },
            Size { width: Dimension::auto(), height: Dimension::stretch() },
        );
        let resolved = properties.resolve(
            Size { width: AvailableSpace::Definite(80.0), height: AvailableSpace::MaxContent },
            Size { width: 10.0, height: 12.0 },
        );

        assert_eq!(resolved.preferred, Size { width: Some(80.0), height: None });
        assert_eq!(resolved.min, Size { width: None, height: Some(12.0) });
        assert_eq!(resolved.max, Size::NONE);
    }

    #[test]
    fn stretch_available_space_omits_only_selected_margin_sides() {
        let available = Size { width: AvailableSpace::Definite(200.0), height: AvailableSpace::Definite(100.0) };
        let margins = Rect { left: 5.0, right: 7.0, top: 11.0, bottom: 13.0 };

        assert_eq!(
            stretch_border_box_available_space(
                available,
                margins,
                Rect { left: false, right: false, top: true, bottom: false },
            ),
            Size { width: AvailableSpace::Definite(188.0), height: AvailableSpace::Definite(87.0) },
        );
    }
}
