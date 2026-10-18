//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use crate::geometry::Size;
use crate::style::{AvailableSpace, CoreStyle, Dimension};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, RequestedAxis,
    SizingMode,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::BoxSizing;

/// Measure one intrinsic inline-size contribution for a node.
fn measure_intrinsic_width(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    constraint: AvailableSpace,
) -> IntrinsicSizeResult {
    tree.measure_child_size_with_metadata(
        node_id,
        ChildLayoutInput::new(
            Size { width: None, height: inputs.known_dimensions.height },
            inputs.parent_size,
            inputs.parent_writing_mode,
            Size { width: constraint, height: inputs.available_space.height },
            SizingMode::ContentSize,
            inputs.vertical_margins_are_collapsible,
        ),
        RequestedAxis::Horizontal,
    )
}

/// One resolved intrinsic width together with cache dependency metadata.
#[derive(Clone, Copy, Debug, Default)]
struct IntrinsicWidthValue {
    /// Resolved border-box width, or `None` when the value is not intrinsic.
    value: Option<f32>,
    /// Whether measuring the value observed a block-constraint dependency.
    depends_on_block_constraints: bool,
}

/// Resolve a horizontal sizing value that may depend on the box's intrinsic
/// content contributions.
///
/// `available_width` is the border-box space left after horizontal margins.
/// Returned values are border-box sizes, matching `LayoutInput::known_dimensions`.
fn resolve_intrinsic_width_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    value: Dimension,
    available_width: AvailableSpace,
) -> IntrinsicWidthValue {
    if value.is_stretch() {
        return IntrinsicWidthValue { value: available_width.into_option(), depends_on_block_constraints: false };
    }
    if !value.is_intrinsic() {
        return IntrinsicWidthValue::default();
    }

    if value.is_min_content() {
        let measured = measure_intrinsic_width(tree, node_id, inputs, AvailableSpace::MinContent);
        return IntrinsicWidthValue {
            value: Some(measured.size.width),
            depends_on_block_constraints: measured.depends_on_block_constraints,
        };
    }

    let max_content = measure_intrinsic_width(tree, node_id, inputs, AvailableSpace::MaxContent);
    if value.is_max_content() {
        return IntrinsicWidthValue {
            value: Some(max_content.size.width),
            depends_on_block_constraints: max_content.depends_on_block_constraints,
        };
    }

    let min_content = measure_intrinsic_width(tree, node_id, inputs, AvailableSpace::MinContent);
    IntrinsicWidthValue {
        value: Some(match available_width {
            AvailableSpace::MinContent => min_content.size.width,
            AvailableSpace::MaxContent => max_content.size.width,
            AvailableSpace::Definite(limit) => limit.clamp(min_content.size.width, max_content.size.width),
        }),
        depends_on_block_constraints: min_content.depends_on_block_constraints
            || max_content.depends_on_block_constraints,
    }
}

/// Intrinsic components of the preferred, minimum, and maximum inline sizes.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain only the
/// values that required intrinsic content measurement (or `stretch`), including
/// their preferred-ratio transfers. They remain candidates until used sizing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntrinsicWidthConstraints {
    /// Intrinsic component of `width`.
    pub preferred: Option<f32>,
    /// Intrinsic component of `min-width`.
    pub min: Option<f32>,
    /// Intrinsic component of `max-width`.
    pub max: Option<f32>,
    /// Whether any measured contribution changes with the containing block's
    /// block-size.
    pub depends_on_block_constraints: bool,
    /// Whether the preferred width was transferred from the block axis.
    pub applied_aspect_ratio: bool,
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
    let preferred = resolve_intrinsic_width_value(tree, node_id, inputs, preferred, available_width);
    let min = resolve_intrinsic_width_value(tree, node_id, inputs, min, available_width);
    let max = resolve_intrinsic_width_value(tree, node_id, inputs, max, available_width);
    IntrinsicWidthConstraints {
        preferred: preferred.value,
        min: min.value,
        max: max.value,
        depends_on_block_constraints: preferred.depends_on_block_constraints
            || min.depends_on_block_constraints
            || max.depends_on_block_constraints,
        applied_aspect_ratio: false,
    }
}

/// Compatibility name for the result of shared used-width resolution.
pub type ResolvedIntrinsicWidthInputs = super::used_size::ResolvedWidthInputs;

/// Compatibility entry point retaining intrinsic measurement provenance.
pub fn resolve_intrinsic_width_inputs_with_provenance(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
) -> ResolvedIntrinsicWidthInputs {
    super::used_size::resolve_used_width_inputs(tree, node_id, inputs)
}

/// Measure preferred/minimum/maximum width candidates without assigning a
/// final width or clamping raw content contributions to authored limits.
///
/// All values are border-box sizes. Numeric and percentage constraints remain
/// the responsibility of the shared used-size stage. Recursive content probes
/// use `ContentSize`, keeping the original intrinsic contribution available.
pub fn measure_intrinsic_width_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
) -> IntrinsicWidthConstraints {
    if inputs.sizing_mode != SizingMode::InherentSize {
        return IntrinsicWidthConstraints::default();
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
        let raw_min_size = style.min_size().map(crate::Dimension::from);
        let raw_max_size = style.max_size().map(crate::Dimension::from);
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

    IntrinsicWidthConstraints {
        preferred,
        min: min_size,
        max: max_size,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints,
        applied_aspect_ratio,
    }
}
