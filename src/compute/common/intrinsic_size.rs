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
            inputs.definite_dimensions.width = None;
            inputs.available_space.width = constraint;
        }
        AbsoluteAxis::Vertical => {
            inputs.known_dimensions.height = None;
            inputs.definite_dimensions.height = None;
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
    /// Whether this value came from the opposite axis through the preferred
    /// aspect ratio rather than from raw intrinsic content measurement.
    pub applied_aspect_ratio: bool,
}

/// A `SizeType::Content` contribution synthesized from the opposite axis.
///
/// This wrapper prevents a raw intrinsic measurement from being passed into
/// the ratio slot merely because both values happen to carry the same numeric
/// and cache metadata.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RatioDependentContentContribution(IntrinsicAxisValue);

/// Resolve the `SizeType::Content` contribution supplied by a definite
/// opposite-axis preferred size and a preferred aspect ratio.
///
/// Inputs are already-resolved border-box constraints. Clearing the queried
/// axis before transfer ensures an authored preferred size in that axis does
/// not masquerade as the content contribution used by intrinsic keywords.
pub(crate) fn resolve_ratio_dependent_content_contribution(
    preferred_size: Size<Option<f32>>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    aspect_ratio: Option<ResolvedAspectRatio>,
    padding_border: Size<f32>,
    axis: AbsoluteAxis,
    depends_on_block_constraints: bool,
) -> RatioDependentContentContribution {
    let mut ratio_source = preferred_size.maybe_clamp(min_size, max_size);
    match axis {
        AbsoluteAxis::Horizontal => ratio_source.width = None,
        AbsoluteAxis::Vertical => ratio_source.height = None,
    }
    let value = ratio_source
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
        .get_abs(axis);
    RatioDependentContentContribution(IntrinsicAxisValue {
        value,
        depends_on_block_constraints: value.is_some() && depends_on_block_constraints,
        applied_aspect_ratio: value.is_some(),
    })
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
    ratio_content_contribution: RatioDependentContentContribution,
) -> IntrinsicAxisValue {
    if value.is_stretch() {
        return IntrinsicAxisValue {
            value: available_space.into_option(),
            depends_on_block_constraints: false,
            applied_aspect_ratio: false,
        };
    }
    if !value.is_intrinsic() {
        return IntrinsicAxisValue::default();
    }
    if ratio_content_contribution.0.value.is_some() {
        return ratio_content_contribution.0;
    }

    if value.is_min_content() {
        let measured = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
        return IntrinsicAxisValue {
            value: Some(measured.size.get_abs(axis)),
            depends_on_block_constraints: measured.depends_on_block_constraints,
            applied_aspect_ratio: false,
        };
    }

    let max_content = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MaxContent, axis);
    if value.is_max_content() {
        return IntrinsicAxisValue {
            value: Some(max_content.size.get_abs(axis)),
            depends_on_block_constraints: max_content.depends_on_block_constraints,
            applied_aspect_ratio: false,
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
        applied_aspect_ratio: false,
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
    resolve_intrinsic_axis_size(tree, node_id, inputs, value, available_space, axis)
}

/// Resolve one intrinsic sizing property at a formatting-context-owned axis.
///
/// This lower-level entry point is used when an algorithm needs the property
/// value itself, rather than a preferred size already clamped by min/max
/// constraints. The result is a border-box size.
pub(crate) fn resolve_intrinsic_axis_size(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    value: Dimension,
    available_space: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicAxisValue {
    let inputs = resolve_intrinsic_measurement_input(tree, node_id, inputs);
    resolve_intrinsic_axis_value(
        tree,
        node_id,
        inputs,
        value,
        available_space,
        axis,
        RatioDependentContentContribution::default(),
    )
}

/// Intrinsic components of preferred, minimum and maximum sizes in one axis.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain only the
/// values that required intrinsic content measurement (or `stretch`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicSizeConstraints {
    /// Intrinsic component of the preferred size.
    pub preferred: IntrinsicAxisValue,
    /// Intrinsic component of the minimum size.
    pub min: IntrinsicAxisValue,
    /// Intrinsic component of the maximum size.
    pub max: IntrinsicAxisValue,
}

impl IntrinsicSizeConstraints {
    /// Whether any resolved property observed a block-constraint dependency.
    #[inline(always)]
    pub(crate) fn depends_on_block_constraints(self) -> bool {
        self.preferred.depends_on_block_constraints
            || self.min.depends_on_block_constraints
            || self.max.depends_on_block_constraints
    }
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
    /// Ratio-dependent `SizeType::Content` result for intrinsic keywords.
    pub ratio_content_contribution: RatioDependentContentContribution,
}

/// Authored physical-width constraints resolved by a formatting context.
///
/// Grouping these values makes the content-contribution source part of the
/// sizing request, rather than an optional positional add-on that individual
/// block, flex, grid, or absolute paths can accidentally omit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntrinsicWidthInput {
    /// Preferred width property.
    pub preferred: Dimension,
    /// Minimum width property.
    pub min: Dimension,
    /// Maximum width property.
    pub max: Dimension,
    /// Available border-box width after horizontal margins.
    pub available_space: AvailableSpace,
    /// Ratio-dependent `SizeType::Content` result for width keywords.
    pub ratio_content_contribution: RatioDependentContentContribution,
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
    let mut definite_logical_size = writing_mode.to_logical(child_input.definite_dimensions);
    let outer_inline_size = known_logical_size.inline_size;
    known_logical_size.block_size = None;
    definite_logical_size.block_size = None;
    child_input.known_dimensions = writing_mode.to_physical(known_logical_size);
    child_input.definite_dimensions = writing_mode.to_physical(definite_logical_size);
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
    width_input: IntrinsicWidthInput,
) -> IntrinsicSizeConstraints {
    let child_input = ChildLayoutInput::new(
        inputs.known_dimensions,
        inputs.parent_size,
        inputs.parent_writing_mode,
        inputs.available_space,
        SizingMode::ContentSize,
        inputs.vertical_margins_are_collapsible,
    )
    .with_definite_dimensions(inputs.definite_dimensions)
    .with_inline_auto_behavior(inputs.inline_auto_behavior)
    .with_block_auto_behavior(inputs.block_auto_behavior)
    .with_orthogonal_fallback(inputs.orthogonal_fallback);
    resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        child_input,
        IntrinsicAxisInput {
            preferred: width_input.preferred,
            min: width_input.min,
            max: width_input.max,
            available_space: width_input.available_space,
            axis: AbsoluteAxis::Horizontal,
            ratio_content_contribution: width_input.ratio_content_contribution,
        },
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
    let inputs = resolve_intrinsic_measurement_input(tree, node_id, inputs);
    let IntrinsicAxisInput { preferred, min, max, available_space, axis, ratio_content_contribution } = axis_input;
    IntrinsicSizeConstraints {
        preferred: resolve_intrinsic_axis_value(
            tree,
            node_id,
            inputs,
            preferred,
            available_space,
            axis,
            ratio_content_contribution,
        ),
        min: resolve_intrinsic_axis_value(
            tree,
            node_id,
            inputs,
            min,
            available_space,
            axis,
            ratio_content_contribution,
        ),
        max: resolve_intrinsic_axis_value(
            tree,
            node_id,
            inputs,
            max,
            available_space,
            axis,
            ratio_content_contribution,
        ),
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

/// Initial border-box geometry available while measuring an intrinsic width.
///
/// `known_dimensions` controls the node's own measurement. The narrower
/// `definite_dimensions` subset is the only geometry descendants may use to
/// resolve percentages. Keeping both values together mirrors Blink's initial
/// fragment geometry plus percentage-resolution constraint space, and avoids
/// turning a merely known parent-imposed size into a definite CSS size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct IntrinsicMeasurementGeometry {
    /// Used border-box dimensions fixed while content is measured.
    known_dimensions: Size<Option<f32>>,
    /// Definite subset exposed to descendant percentage resolution.
    definite_dimensions: Size<Option<f32>>,
}

impl IntrinsicMeasurementGeometry {
    /// Combine parent-imposed geometry with directly resolved authored sizes.
    ///
    /// A parent-known axis overrides the authored preferred size. Its
    /// definiteness must therefore come from the parent as well; otherwise a
    /// directly authored length could incorrectly bless an overridden used
    /// size as a descendant percentage basis.
    #[inline(always)]
    fn resolve(
        parent_known: Size<Option<f32>>,
        parent_definite: Size<Option<f32>>,
        own_definite: Size<Option<f32>>,
    ) -> Self {
        let known_dimensions = parent_known.or(own_definite);
        let definite_source = Size {
            width: if parent_known.width.is_some() {
                parent_definite.width
            } else {
                parent_definite.width.or(own_definite.width)
            },
            height: if parent_known.height.is_some() {
                parent_definite.height
            } else {
                parent_definite.height.or(own_definite.height)
            },
        };
        let definite_dimensions = Size {
            width: definite_source.width.and(known_dimensions.width),
            height: definite_source.height.and(known_dimensions.height),
        };
        Self { known_dimensions, definite_dimensions }
    }
}

/// Build the child-owned initial geometry for an intrinsic contribution.
///
/// Formatting contexts own available space, alignment and any dimensions they
/// fix explicitly. The measured node still owns direct authored sizing in the
/// perpendicular axis. Resolve that geometry once at the intrinsic sizing
/// boundary instead of requiring block, flex and grid to reconstruct it at
/// each call site.
fn resolve_intrinsic_measurement_input(
    tree: &impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut inputs: ChildLayoutInput,
) -> ChildLayoutInput {
    let percentage_basis = inputs.parent_writing_mode.to_logical(inputs.parent_size).inline_size;
    let own_definite_dimensions = {
        let style = tree.get_core_container_style(node_id);
        let padding = style.padding().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let box_sizing_adjustment =
            if style.box_sizing() == BoxSizing::ContentBox { (padding + border).sum_axes() } else { Size::ZERO };
        let resolve = |size: Size<Dimension>| {
            size.maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_add(box_sizing_adjustment)
        };
        resolve(style.size()).maybe_clamp(resolve(style.min_size()), resolve(style.max_size()))
    };
    let geometry = IntrinsicMeasurementGeometry::resolve(
        inputs.known_dimensions,
        inputs.definite_dimensions,
        own_definite_dimensions,
    );
    inputs.known_dimensions = geometry.known_dimensions;
    inputs.definite_dimensions = geometry.definite_dimensions;
    inputs
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
        resolved_size,
        resolved_min_size,
        resolved_max_size,
        aspect_ratio,
        padding_border_size,
        ratio_depends_on_block_constraints,
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
        let resolve = |raw: Size<Dimension>| {
            raw.maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_add(box_sizing_adjustment)
        };
        let ratio_depends_on_block_constraints = aspect_ratio.is_some()
            && [raw_size.height, raw_min_size.height, raw_max_size.height]
                .into_iter()
                .any(|value| value.may_have_percentage_dependence() || value.is_stretch());
        (
            raw_size,
            raw_min_size,
            raw_max_size,
            margin,
            resolve(raw_size),
            resolve(raw_min_size),
            resolve(raw_max_size),
            aspect_ratio,
            padding_border_size,
            ratio_depends_on_block_constraints,
        )
    };
    let available_width = inputs.available_space.width.maybe_sub(margin.horizontal_axis_sum());
    let ratio_content_contribution = resolve_ratio_dependent_content_contribution(
        resolved_size,
        resolved_min_size,
        resolved_max_size,
        aspect_ratio,
        padding_border_size,
        AbsoluteAxis::Horizontal,
        ratio_depends_on_block_constraints,
    );

    let intrinsic = resolve_intrinsic_width_constraints(
        tree,
        node_id,
        inputs,
        IntrinsicWidthInput {
            preferred: raw_size.width,
            min: raw_min_size.width,
            max: raw_max_size.width,
            available_space: available_width,
            ratio_content_contribution,
        },
    );
    let applied_aspect_ratio = inputs.known_dimensions.width.is_none() && intrinsic.preferred.applied_aspect_ratio;

    inputs.known_dimensions.width = inputs
        .known_dimensions
        .width
        .or(intrinsic.preferred.value)
        .maybe_clamp(intrinsic.min.value, intrinsic.max.value);
    ResolvedIntrinsicWidthInputs {
        inputs,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints(),
        applied_aspect_ratio,
    }
}

#[cfg(test)]
mod intrinsic_measurement_geometry_tests {
    use super::*;

    #[test]
    fn authored_geometry_is_definite_when_the_parent_does_not_override_it() {
        let geometry =
            IntrinsicMeasurementGeometry::resolve(Size::NONE, Size::NONE, Size { width: None, height: Some(100.0) });

        assert_eq!(geometry.known_dimensions, Size { width: None, height: Some(100.0) });
        assert_eq!(geometry.definite_dimensions, Size { width: None, height: Some(100.0) });
    }

    #[test]
    fn parent_override_exclusively_owns_definiteness() {
        let indefinite_override = IntrinsicMeasurementGeometry::resolve(
            Size { width: None, height: Some(80.0) },
            Size::NONE,
            Size { width: None, height: Some(100.0) },
        );
        assert_eq!(indefinite_override.known_dimensions.height, Some(80.0));
        assert_eq!(indefinite_override.definite_dimensions.height, None);

        let definite_override = IntrinsicMeasurementGeometry::resolve(
            Size { width: None, height: Some(80.0) },
            Size { width: None, height: Some(80.0) },
            Size { width: None, height: Some(100.0) },
        );
        assert_eq!(definite_override.known_dimensions.height, Some(80.0));
        assert_eq!(definite_override.definite_dimensions.height, Some(80.0));
    }

    #[test]
    fn definite_geometry_is_always_a_subset_of_known_geometry() {
        let geometry =
            IntrinsicMeasurementGeometry::resolve(Size::NONE, Size { width: Some(75.0), height: None }, Size::NONE);

        assert_eq!(geometry.known_dimensions, Size::NONE);
        assert_eq!(geometry.definite_dimensions, Size::NONE);
    }
}
