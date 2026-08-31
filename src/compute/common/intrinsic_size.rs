//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use super::aspect_ratio::ResolvedAxisConstraints;
use crate::geometry::{AbsoluteAxis, LogicalSize, Size, WritingMode};
use crate::style::{AvailableSpace, CoreStyle, Dimension};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, RequestedAxis,
    SizingMode,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, ResolvedAspectRatio};

/// Measure one intrinsic contribution for a node in a physical axis.
pub(crate) fn measure_intrinsic_axis(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut inputs: ChildLayoutInput,
    constraint: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicSizeResult {
    match axis {
        AbsoluteAxis::Horizontal => {
            inputs.known_dimensions.width = None;
            inputs.available_space.width = constraint;
        }
        AbsoluteAxis::Vertical => {
            inputs.known_dimensions.height = None;
            inputs.available_space.height = constraint;
        }
    }
    inputs.sizing_mode = SizingMode::ContentSize;
    tree.measure_child_size_with_metadata(node_id, inputs, axis.into())
}

/// One resolved intrinsic axis value together with cache dependency metadata.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicAxisValue {
    /// Resolved border-box size, or `None` when the value is not intrinsic.
    pub value: Option<f32>,
    /// Whether measuring the value observed a block-constraint dependency.
    pub depends_on_block_constraints: bool,
}

/// Resolve a sizing value that may depend on the box's intrinsic content
/// contributions in one physical axis.
///
/// `available_space` is the border-box space left after margins in `axis`.
/// Returned values are border-box sizes, matching `LayoutInput::known_dimensions`.
fn resolve_intrinsic_axis_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    value: Dimension,
    available_space: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicAxisValue {
    if value.is_stretch() {
        return IntrinsicAxisValue { value: available_space.into_option(), depends_on_block_constraints: false };
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
    let min_content_size = min_content.size.get_abs(axis);
    let max_content_size = max_content.size.get_abs(axis);
    IntrinsicAxisValue {
        value: Some(match available_space {
            AvailableSpace::MinContent => min_content_size,
            AvailableSpace::MaxContent => max_content_size,
            AvailableSpace::Definite(limit) => limit.clamp(min_content_size, max_content_size),
        }),
        depends_on_block_constraints: min_content.depends_on_block_constraints
            || max_content.depends_on_block_constraints,
    }
}

/// Resolve an intrinsic preferred size at a formatting-context-owned axis.
///
/// Flex basis resolution supplies its own available main size, which can
/// intentionally differ from the child's cross-axis constraint. Keeping both
/// inputs explicit preserves min-/max-/fit-content semantics in parallel and
/// orthogonal writing modes.
pub(crate) fn resolve_intrinsic_preferred_axis_size(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    value: Dimension,
    available_space: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicAxisValue {
    debug_assert_eq!(axis, tree.get_writing_mode(node_id).inline_axis());
    resolve_intrinsic_axis_value(tree, node_id, inputs, value, available_space, axis)
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

/// A min-intrinsic automatic minimum activated by a ratio-derived preferred
/// size in one axis.
///
/// Keeping the source-preserving constraints inside this type makes it
/// impossible for a formatting context to apply the minimum with the wrong
/// authored/transferred maximum ordering.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RatioDependentAutomaticMinimum {
    /// Authored and transferred min/max sources retained until the intrinsic
    /// contribution is known.
    constraint_sources: ResolvedAxisConstraints,
}

impl RatioDependentAutomaticMinimum {
    /// Capture the automatic-minimum state while authored style and ratio
    /// provenance are both available.
    #[inline(always)]
    pub(crate) fn new(
        constraint_sources: ResolvedAxisConstraints,
        preferred_size_from_ratio: bool,
        authored_min_size: Dimension,
        is_scroll_container: bool,
        is_replaced: bool,
    ) -> Option<Self> {
        (preferred_size_from_ratio && authored_min_size.is_auto() && !is_scroll_container && !is_replaced)
            .then_some(Self { constraint_sources })
    }

    /// Merge a measured min-intrinsic contribution with authored and
    /// ratio-transferred constraints in CSS sizing order.
    #[inline(always)]
    pub(crate) fn resolve(self, min_intrinsic_size: f32) -> (Option<f32>, Option<f32>) {
        self.constraint_sources.resolve(None, None, Some(min_intrinsic_size))
    }
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

/// Content-derived constraints for one logical block axis.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicBlockSizeConstraints {
    /// Content-derived preferred block size.
    pub preferred: Option<f32>,
    /// Content-derived minimum block size.
    pub min: Option<f32>,
    /// Content-derived maximum block size.
    pub max: Option<f32>,
    /// Ratio-dependent automatic minimum before the authored maximum caps it.
    automatic_min: Option<f32>,
    /// Whether measuring the contribution observed a block constraint.
    pub depends_on_block_constraints: bool,
}

impl IntrinsicBlockSizeConstraints {
    /// Merge content-derived values with already-resolved authored values.
    #[inline(always)]
    pub(crate) fn resolve_against(self, preferred: Option<f32>, constraint_sources: ResolvedAxisConstraints) -> Self {
        let (min, max) = constraint_sources.resolve(self.min, self.max, self.automatic_min);
        Self {
            preferred: preferred.or(self.preferred),
            min,
            max,
            automatic_min: None,
            depends_on_block_constraints: self.depends_on_block_constraints,
        }
    }
}

/// Authored preferred, minimum, and maximum sizes on a logical block axis.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlockSizeProperties {
    /// Authored preferred logical block-size.
    preferred: Dimension,
    /// Authored minimum logical block-size.
    min: Dimension,
    /// Authored maximum logical block-size.
    max: Dimension,
}

impl BlockSizeProperties {
    /// Construct a logical block-axis property triplet.
    #[inline(always)]
    pub(crate) const fn new(preferred: Dimension, min: Dimension, max: Dimension) -> Self {
        Self { preferred, min, max }
    }

    /// Whether a provisional ratio-derived block size must remain content-sized.
    #[inline(always)]
    pub(crate) fn preferred_is_content_based(self, auto_size_is_content_based: bool) -> bool {
        self.preferred.is_intrinsic() || (self.preferred.is_auto() && auto_size_is_content_based)
    }

    #[inline(always)]
    /// Whether any authored block-size property requires intrinsic measurement.
    fn uses_intrinsic_size(self) -> bool {
        self.preferred.is_intrinsic() || self.min.is_intrinsic() || self.max.is_intrinsic()
    }

    #[inline(always)]
    /// Whether aspect-ratio sizing contributes the automatic content minimum.
    fn applies_automatic_minimum(
        self,
        has_preferred_aspect_ratio: bool,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
        is_replaced: bool,
    ) -> bool {
        has_preferred_aspect_ratio
            && !is_scroll_container
            && !is_replaced
            && self.min.is_auto()
            && self.preferred_is_content_based(auto_size_is_content_based)
    }

    #[inline(always)]
    /// Resolve authored intrinsic values and the ratio-dependent automatic minimum.
    fn resolve(
        self,
        intrinsic_border_box_size: f32,
        ratio_block_size: Option<f32>,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
        is_replaced: bool,
    ) -> IntrinsicBlockSizeConstraints {
        let content_block_size = ratio_block_size.unwrap_or(intrinsic_border_box_size);
        let resolve_explicit = |value: Dimension| value.is_intrinsic().then_some(content_block_size);
        IntrinsicBlockSizeConstraints {
            preferred: resolve_explicit(self.preferred).or_else(|| {
                (self.preferred.is_auto() && auto_size_is_content_based).then_some(ratio_block_size).flatten()
            }),
            min: resolve_explicit(self.min),
            max: resolve_explicit(self.max),
            automatic_min: self
                .applies_automatic_minimum(
                    ratio_block_size.is_some(),
                    auto_size_is_content_based,
                    is_scroll_container,
                    is_replaced,
                )
                .then_some(intrinsic_border_box_size),
            depends_on_block_constraints: false,
        }
    }
}

/// Shared state for resolving a content-based logical block size.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentBasedBlockSize {
    /// Authored logical block-axis properties.
    properties: BlockSizeProperties,
    /// Used preferred ratio, including its sizing-box semantics.
    aspect_ratio: Option<ResolvedAspectRatio>,
    /// Physical padding-and-border sums used for ratio box conversion.
    padding_border: Size<f32>,
    /// Whether an authored automatic block size derives from content here.
    auto_size_is_content_based: bool,
    /// Whether overflow suppresses the ratio-dependent automatic minimum.
    is_scroll_container: bool,
    /// Whether replaced sizing bypasses the non-replaced automatic minimum.
    is_replaced: bool,
}

impl ContentBasedBlockSize {
    /// Construct the resolver at a formatting-context boundary.
    #[inline(always)]
    pub(crate) const fn new(
        properties: BlockSizeProperties,
        aspect_ratio: Option<ResolvedAspectRatio>,
        padding_border: Size<f32>,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
        is_replaced: bool,
    ) -> Self {
        Self { properties, aspect_ratio, padding_border, auto_size_is_content_based, is_scroll_container, is_replaced }
    }

    /// Whether the real intrinsic block contribution is required.
    #[inline(always)]
    pub(crate) fn requires_intrinsic_measurement(self) -> bool {
        self.properties.uses_intrinsic_size()
            || self.properties.applies_automatic_minimum(
                self.aspect_ratio.is_some(),
                self.auto_size_is_content_based,
                self.is_scroll_container,
                self.is_replaced,
            )
    }

    /// Resolve the content-derived block-axis constraints.
    #[inline(always)]
    pub(crate) fn resolve(
        self,
        writing_mode: WritingMode,
        outer_inline_size: Option<f32>,
        intrinsic_border_box_size: f32,
    ) -> IntrinsicBlockSizeConstraints {
        let physical_size = writing_mode.to_physical(LogicalSize { inline_size: outer_inline_size, block_size: None });
        let ratio_size = physical_size.maybe_apply_aspect_ratio_with_box_sizing(
            self.aspect_ratio,
            BoxSizing::BorderBox,
            self.padding_border,
        );
        let ratio_block_size = writing_mode.to_logical(ratio_size).block_size;
        self.properties.resolve(
            intrinsic_border_box_size,
            ratio_block_size,
            self.auto_size_is_content_based,
            self.is_scroll_container,
            self.is_replaced,
        )
    }
}

/// Measure a node's real intrinsic block contribution after its inline size is known.
pub(crate) fn measure_content_based_block_size(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut child_input: ChildLayoutInput,
    resolver: ContentBasedBlockSize,
) -> IntrinsicBlockSizeConstraints {
    if !resolver.requires_intrinsic_measurement() {
        return IntrinsicBlockSizeConstraints::default();
    }

    let writing_mode = tree.get_writing_mode(node_id);
    let mut known_logical_size = writing_mode.to_logical(child_input.known_dimensions);
    let outer_inline_size = known_logical_size.inline_size;
    known_logical_size.block_size = None;
    child_input.known_dimensions = writing_mode.to_physical(known_logical_size);
    child_input.sizing_mode = SizingMode::ContentSize;
    let measured =
        tree.measure_child_size_with_metadata(node_id, child_input, RequestedAxis::from(writing_mode.block_axis()));
    let mut constraints =
        resolver.resolve(writing_mode, outer_inline_size, writing_mode.to_logical(measured.size).block_size);
    constraints.depends_on_block_constraints = measured.depends_on_block_constraints;
    constraints
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
    let child_input = ChildLayoutInput::new(
        inputs.known_dimensions,
        inputs.parent_size,
        inputs.parent_writing_mode,
        inputs.available_space,
        SizingMode::ContentSize,
        inputs.vertical_margins_are_collapsible,
    )
    .with_inline_auto_behavior(inputs.inline_auto_behavior)
    .with_block_auto_behavior(inputs.block_auto_behavior)
    .with_orthogonal_fallback(inputs.orthogonal_fallback);
    resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        child_input,
        IntrinsicAxisInput { preferred, min, max, available_space: available_width, axis: AbsoluteAxis::Horizontal },
    )
}

/// Resolve preferred/minimum/maximum intrinsic sizing values along one
/// physical axis. Formatting contexts select the axis by projecting their
/// logical inline axis through their writing mode.
pub(crate) fn resolve_intrinsic_axis_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
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
