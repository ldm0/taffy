//! Shared resolution of final border-box sizes at layout-algorithm boundaries.

use super::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use super::intrinsic_size::measure_intrinsic_width_constraints;
use crate::tree::LayoutPartialTreeExt;
use crate::{
    BoxSizing, CoreStyle, LayoutInput, LayoutPartialTree, MaybeMath, MaybeResolve, ResolveOrZero, Size, SizingMode,
};

/// A final used-width input together with its intrinsic measurement provenance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedWidthInputs {
    /// Input whose synthesized width has passed all applicable constraints.
    /// Parent-supplied fixed dimensions are preserved.
    pub inputs: LayoutInput,
    /// Whether measuring intrinsic contributions observed block constraints.
    pub depends_on_block_constraints: bool,
    /// Whether the preferred aspect ratio supplied the synthesized width.
    pub applied_aspect_ratio: bool,
}

/// Resolve a content-derived preferred width at the shared used-size boundary.
///
/// Measurement supplies candidates, while this stage resolves authored numeric
/// and percentage constraints using the containing block, merges both kinds of
/// limits, and only then assigns a final `known_dimensions.width`. All layout
/// algorithms and browser integrations use the same ordering.
pub fn resolve_used_width_inputs(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut inputs: LayoutInput,
) -> ResolvedWidthInputs {
    if inputs.sizing_mode != SizingMode::InherentSize || inputs.known_dimensions.width.is_some() {
        return ResolvedWidthInputs { inputs, depends_on_block_constraints: false, applied_aspect_ratio: false };
    }

    let intrinsic = measure_intrinsic_width_constraints(tree, node_id, inputs);
    if intrinsic.preferred.is_none() && intrinsic.min.is_none() && intrinsic.max.is_none() {
        return ResolvedWidthInputs {
            inputs,
            depends_on_block_constraints: intrinsic.depends_on_block_constraints,
            applied_aspect_ratio: intrinsic.applied_aspect_ratio,
        };
    }

    let writing_mode = tree.get_writing_mode(node_id);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let aspect_ratio = tree.get_resolved_aspect_ratio(node_id);
    let style = tree.get_core_container_style(node_id);
    let padding = style.padding().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
    let padding_border = (padding + border).sum_axes();
    let adjustment = if style.box_sizing() == BoxSizing::ContentBox { padding_border } else { Size::ZERO };
    let raw_size = style.size();
    let resolved = resolve_size_constraints(SizeConstraintInput {
        size: raw_size.maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis)).maybe_add(adjustment),
        min_size: style
            .min_size()
            .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
            .maybe_add(adjustment),
        max_size: style
            .max_size()
            .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
            .maybe_add(adjustment),
        size_is_auto: raw_size.map(|value| value.is_auto()),
        writing_mode,
        block_auto_behavior: inputs.block_auto_behavior,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border,
    })
    .with_intrinsic_width(intrinsic);

    inputs.known_dimensions.width = resolve_used_axis(
        None,
        resolved.size.width,
        resolved.min_size.width,
        resolved.max_size.width,
        padding_border.width,
    );
    ResolvedWidthInputs {
        inputs,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints,
        applied_aspect_ratio: resolved.aspect_ratio_applied.width,
    }
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
    fn known_used_axis_is_not_reclamped_by_child_constraints() {
        assert_eq!(resolve_used_axis(Some(100.0), Some(50.0), None, Some(50.0), 20.0), Some(100.0));
        assert_eq!(resolve_used_axis(None, Some(100.0), None, Some(50.0), 20.0), Some(50.0));
    }

    #[test]
    fn padding_and_border_floor_known_used_axis() {
        assert_eq!(resolve_used_axis(Some(12.0), None, None, Some(10.0), 22.0), Some(22.0));
    }
}
