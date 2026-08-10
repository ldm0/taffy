//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use crate::geometry::{AbsoluteAxis, Size};
use crate::style::{AvailableSpace, CoreStyle, Dimension};
use crate::tree::{LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::BoxSizing;

/// Measure one intrinsic inline-size contribution for a node.
fn measure_intrinsic_width(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    constraint: AvailableSpace,
) -> f32 {
    tree.measure_child_size(
        node_id,
        Size { width: None, height: inputs.known_dimensions.height },
        inputs.parent_size,
        Size { width: constraint, height: inputs.available_space.height },
        SizingMode::ContentSize,
        AbsoluteAxis::Horizontal,
        inputs.vertical_margins_are_collapsible,
    )
}

/// Resolve a horizontal sizing value that may depend on the box's intrinsic
/// content contributions.
///
/// `available_width` is the border-box space left after horizontal margins.
/// Returned values are border-box sizes, matching `LayoutInput::known_dimensions`.
pub(crate) fn resolve_intrinsic_width_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    value: Dimension,
    available_width: AvailableSpace,
) -> Option<f32> {
    if value.is_stretch() {
        return available_width.into_option();
    }
    if !value.is_intrinsic() {
        return None;
    }

    if value.is_min_content() {
        return Some(measure_intrinsic_width(tree, node_id, inputs, AvailableSpace::MinContent));
    }

    let max_content = measure_intrinsic_width(tree, node_id, inputs, AvailableSpace::MaxContent);
    if value.is_max_content() {
        return Some(max_content);
    }

    let min_content = measure_intrinsic_width(tree, node_id, inputs, AvailableSpace::MinContent);
    Some(match available_width {
        AvailableSpace::MinContent => min_content,
        AvailableSpace::MaxContent => max_content,
        AvailableSpace::Definite(limit) => limit.clamp(min_content, max_content),
    })
}

/// Intrinsic components of the preferred, minimum, and maximum inline sizes.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain only the
/// values that required intrinsic content measurement (or `stretch`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicWidthConstraints {
    /// Intrinsic component of `width`.
    pub preferred: Option<f32>,
    /// Intrinsic component of `min-width`.
    pub min: Option<f32>,
    /// Intrinsic component of `max-width`.
    pub max: Option<f32>,
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
) -> IntrinsicWidthConstraints {
    IntrinsicWidthConstraints {
        preferred: resolve_intrinsic_width_value(tree, node_id, inputs, preferred, available_width),
        min: resolve_intrinsic_width_value(tree, node_id, inputs, min, available_width),
        max: resolve_intrinsic_width_value(tree, node_id, inputs, max, available_width),
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
    mut inputs: LayoutInput,
) -> LayoutInput {
    if inputs.sizing_mode != SizingMode::InherentSize {
        return inputs;
    }

    let (
        raw_size,
        raw_min_size,
        raw_max_size,
        margin,
        transferred_preferred_width,
        transferred_min_width,
        transferred_max_width,
    ) = {
        let style = tree.get_core_container_style(node_id);
        let raw_size = style.size();
        let raw_min_size = style.min_size();
        let raw_max_size = style.max_size();
        let margin = style.margin().resolve_or_zero(inputs.parent_size.width, |value, basis| tree.calc(value, basis));
        let padding = style.padding().resolve_or_zero(inputs.parent_size.width, |value, basis| tree.calc(value, basis));
        let border = style.border().resolve_or_zero(inputs.parent_size.width, |value, basis| tree.calc(value, basis));
        let box_sizing_adjustment =
            if style.box_sizing() == BoxSizing::ContentBox { (padding + border).sum_axes() } else { Size::ZERO };
        let aspect_ratio = style.aspect_ratio();
        let transferred_width = |raw: Size<Dimension>| {
            raw.maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_apply_aspect_ratio(aspect_ratio)
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
    let preferred = transferred_preferred_width.or(intrinsic.preferred);
    let min_size = transferred_min_width.or(intrinsic.min);
    let max_size = transferred_max_width.or(intrinsic.max);

    inputs.known_dimensions.width = inputs.known_dimensions.width.or(preferred).maybe_clamp(min_size, max_size);
    inputs
}
