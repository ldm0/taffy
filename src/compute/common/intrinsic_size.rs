//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use crate::geometry::{AbsoluteAxis, Size};
use crate::style::{AvailableSpace, CoreStyle, Dimension};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, RequestedAxis,
    SizingMode,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::BoxSizing;

/// Measure one intrinsic contribution along a physical axis.
fn measure_intrinsic_axis(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    constraint: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicSizeResult {
    let known_dimensions = match axis {
        AbsoluteAxis::Horizontal => Size { width: None, height: inputs.known_dimensions.height },
        AbsoluteAxis::Vertical => Size { width: inputs.known_dimensions.width, height: None },
    };
    let available_space = match axis {
        AbsoluteAxis::Horizontal => Size { width: constraint, height: inputs.available_space.height },
        AbsoluteAxis::Vertical => Size { width: inputs.available_space.width, height: constraint },
    };
    tree.measure_child_size_with_metadata(
        node_id,
        ChildLayoutInput::new(
            known_dimensions,
            inputs.parent_size,
            inputs.parent_writing_mode,
            available_space,
            SizingMode::ContentSize,
            inputs.vertical_margins_are_collapsible,
        ),
        RequestedAxis::from(axis),
    )
}

/// One resolved intrinsic axis value together with cache dependency metadata.
#[derive(Clone, Copy, Debug, Default)]
struct IntrinsicAxisValue {
    /// Resolved border-box size, or `None` when the value is not intrinsic.
    value: Option<f32>,
    /// Whether measuring the value observed a block-constraint dependency.
    depends_on_block_constraints: bool,
}

/// Resolve one sizing value that may depend on the box's intrinsic content
/// contributions.
///
/// `available_size` is the border-box space left after margins in `axis`.
/// Returned values are border-box sizes, matching `LayoutInput::known_dimensions`.
fn resolve_intrinsic_axis_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    value: Dimension,
    available_size: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicAxisValue {
    if value.is_stretch() {
        return IntrinsicAxisValue { value: available_size.into_option(), depends_on_block_constraints: false };
    }
    if !value.is_intrinsic() {
        return IntrinsicAxisValue::default();
    }

    if value.is_min_content() {
        let measured = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
        return IntrinsicAxisValue {
            value: Some(measured.size.get_abs(axis)),
            depends_on_block_constraints: measured.depends_on_block_constraints,
        };
    }

    let max_content = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MaxContent, axis);
    if value.is_max_content() {
        return IntrinsicAxisValue {
            value: Some(max_content.size.get_abs(axis)),
            depends_on_block_constraints: max_content.depends_on_block_constraints,
        };
    }

    let min_content = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
    IntrinsicAxisValue {
        value: Some(match available_size {
            AvailableSpace::MinContent => min_content.size.get_abs(axis),
            AvailableSpace::MaxContent => max_content.size.get_abs(axis),
            AvailableSpace::Definite(limit) => {
                limit.clamp(min_content.size.get_abs(axis), max_content.size.get_abs(axis))
            }
        }),
        depends_on_block_constraints: min_content.depends_on_block_constraints
            || max_content.depends_on_block_constraints,
    }
}

/// Intrinsic components of preferred, minimum and maximum sizes in one axis.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain only the
/// values that required intrinsic content measurement (or `stretch`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicSizeConstraints {
    /// Intrinsic component of the preferred size.
    pub preferred: Option<f32>,
    /// Intrinsic component of the minimum size.
    pub min: Option<f32>,
    /// Intrinsic component of the maximum size.
    pub max: Option<f32>,
    /// Whether any measured contribution changes with the containing block's
    /// block-size.
    pub depends_on_block_constraints: bool,
}

/// Authored constraints and available space for one intrinsic sizing axis.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntrinsicAxisInput {
    /// Preferred size property in the selected axis.
    pub preferred: Dimension,
    /// Minimum size property in the selected axis.
    pub min: Dimension,
    /// Maximum size property in the selected axis.
    pub max: Dimension,
    /// Available border-box space after margins in the selected axis.
    pub available_space: AvailableSpace,
    /// Physical axis corresponding to the formatting context's logical axis.
    pub axis: AbsoluteAxis,
}

/// Resolve all three horizontal intrinsic sizing properties at one ownership
/// seam.
///
/// Keeping the triplet together prevents block, flex, and grid from drifting
/// into subtly different preferred/min/max ordering. Repeated content probes
/// remain pass-local and are deduplicated by Taffy's layout cache.
pub(crate) fn resolve_intrinsic_width_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    preferred: Dimension,
    min: Dimension,
    max: Dimension,
    available_width: AvailableSpace,
) -> IntrinsicSizeConstraints {
    resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        inputs,
        IntrinsicAxisInput { preferred, min, max, available_space: available_width, axis: AbsoluteAxis::Horizontal },
    )
}

/// Resolve preferred/minimum/maximum intrinsic sizing values along one
/// physical axis. Formatting contexts select the axis by projecting their
/// logical inline axis through their writing mode.
pub(crate) fn resolve_intrinsic_axis_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    axis_input: IntrinsicAxisInput,
) -> IntrinsicSizeConstraints {
    let IntrinsicAxisInput { preferred, min, max, available_space, axis } = axis_input;
    let preferred = resolve_intrinsic_axis_value(tree, node_id, inputs, preferred, available_space, axis);
    let min = resolve_intrinsic_axis_value(tree, node_id, inputs, min, available_space, axis);
    let max = resolve_intrinsic_axis_value(tree, node_id, inputs, max, available_space, axis);
    IntrinsicSizeConstraints {
        preferred: preferred.value,
        min: min.value,
        max: max.value,
        depends_on_block_constraints: preferred.depends_on_block_constraints
            || min.depends_on_block_constraints
            || max.depends_on_block_constraints,
    }
}

/// Resolve intrinsic width/min-width/max-width values on a node before its
/// formatting-context algorithm consumes `known_dimensions`.
///
/// This is public for custom [`LayoutPartialTree`] implementations that
/// dispatch Taffy's low-level algorithms themselves. It is a pure, pass-local
/// sizing step: recursive measurements use `SizingMode::ContentSize` and the
/// existing tree cache.
pub fn resolve_intrinsic_width_inputs(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
) -> LayoutInput {
    resolve_intrinsic_width_inputs_with_provenance(tree, node_id, inputs).inputs
}

/// Resolved input for an intrinsic sizing operation and the provenance needed
/// by the node sizing boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedIntrinsicWidthInputs {
    /// Layout input with intrinsic inline-size keywords resolved into a known
    /// border-box width.
    pub inputs: LayoutInput,
    /// Whether resolving those keywords measured content whose inline
    /// contribution depends on the containing block's block-size.
    pub depends_on_block_constraints: bool,
    /// Whether resolving the preferred inline size synthesized it from the
    /// block axis through the node's preferred aspect ratio.
    pub applied_aspect_ratio: bool,
}

/// Resolve intrinsic inline-size keywords while retaining dependency
/// provenance from recursive content measurements.
///
/// Browser integrations that implement [`LayoutPartialTree`] directly should
/// use this entry point before [`crate::compute_cached_size`], so a resolved
/// `known_dimensions.width` does not erase the dependency that produced it.
pub fn resolve_intrinsic_width_inputs_with_provenance(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut inputs: LayoutInput,
) -> ResolvedIntrinsicWidthInputs {
    if inputs.sizing_mode != SizingMode::InherentSize {
        return ResolvedIntrinsicWidthInputs {
            inputs,
            depends_on_block_constraints: false,
            applied_aspect_ratio: false,
        };
    }

    let percentage_basis = inputs.constraint_space(tree.get_writing_mode(node_id)).margin_padding_percentage_basis();
    let (
        raw_size,
        raw_min_size,
        raw_max_size,
        margin,
        transferred_preferred_width,
        transferred_min_width,
        transferred_max_width,
    ) = {
        let aspect_ratio = tree.get_resolved_aspect_ratio(node_id);
        let style = tree.get_core_container_style(node_id);
        let raw_size = style.size();
        let raw_min_size = style.min_size();
        let raw_max_size = style.max_size();
        let margin = style.margin().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let padding = style.padding().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let box_sizing_adjustment =
            if style.box_sizing() == BoxSizing::ContentBox { (padding + border).sum_axes() } else { Size::ZERO };
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing = style.box_sizing();
        let transferred_width = |raw: Size<Dimension>| {
            raw.maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, box_sizing, padding_border_size)
                .maybe_add(box_sizing_adjustment)
                .width
        };
        (
            raw_size,
            raw_min_size,
            raw_max_size,
            margin,
            raw_size.width.is_intrinsic().then(|| transferred_width(raw_size)).flatten(),
            raw_min_size.width.is_intrinsic().then(|| transferred_width(raw_min_size)).flatten(),
            raw_max_size.width.is_intrinsic().then(|| transferred_width(raw_max_size)).flatten(),
        )
    };
    let available_width = inputs.available_space.width.maybe_sub(margin.horizontal_axis_sum());

    let intrinsic = resolve_intrinsic_width_constraints(
        tree,
        node_id,
        inputs,
        raw_size.width,
        raw_min_size.width,
        raw_max_size.width,
        available_width,
    );
    let applied_aspect_ratio = inputs.known_dimensions.width.is_none() && transferred_preferred_width.is_some();
    let preferred = transferred_preferred_width.or(intrinsic.preferred);
    let min_size = transferred_min_width.or(intrinsic.min);
    let max_size = transferred_max_width.or(intrinsic.max);

    inputs.known_dimensions.width = inputs.known_dimensions.width.or(preferred).maybe_clamp(min_size, max_size);
    ResolvedIntrinsicWidthInputs {
        inputs,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints,
        applied_aspect_ratio,
    }
}
