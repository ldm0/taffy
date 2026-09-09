//! Resolution of intrinsic sizing keywords in a formatting context's axes.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use super::aspect_ratio::{
    resolve_size_constraints, ResolvedAxisConstraints, ResolvedSizeConstraints, SizeConstraintInput,
    TransferredSizesMode,
};
use crate::geometry::{AbsoluteAxis, LogicalSize, Size, WritingMode};
use crate::style::{AvailableSpace, CoreStyle, Dimension};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, RequestedAxis,
    SizingMode,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, ResolvedAspectRatio};

/// Measure intrinsic content in a formatting context's selected physical axis.
fn measure_intrinsic_axis(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    constraint: AvailableSpace,
    axis: AbsoluteAxis,
) -> IntrinsicSizeResult {
    let (known_dimensions, available_space) = match axis {
        AbsoluteAxis::Horizontal => (
            Size { width: None, height: inputs.known_dimensions.height },
            Size { width: constraint, height: inputs.available_space.height },
        ),
        AbsoluteAxis::Vertical => (
            Size { width: inputs.known_dimensions.width, height: None },
            Size { width: inputs.available_space.width, height: constraint },
        ),
    };
    tree.measure_child_size_with_metadata(
        node_id,
        ChildLayoutInput { known_dimensions, available_space, sizing_mode: SizingMode::ContentSize, ..inputs },
        RequestedAxis::from(axis),
    )
}

/// Complete a ratio-dependent inline constraint before its preferred size is
/// published as an exact child dimension. The ratio supplies the preferred
/// extent, not the real min-content contribution (CSS Sizing 4 section 4.3).
///
/// Callers retain ownership of fixed layout inputs. This operation applies to
/// provisional style-derived sizes only, never to a flexed or stretched size
/// already assigned by a parent formatting context.
/// Returns whether measuring the minimum observed a block-constraint dependency.
pub(crate) fn resolve_ratio_dependent_inline_minimum(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    constraints: &mut ResolvedSizeConstraints,
) -> bool {
    let writing_mode = tree.get_writing_mode(node_id);
    let axis = writing_mode.inline_axis();
    if !constraints.aspect_ratio_applied.get_abs(axis) {
        return false;
    }
    let style = tree.get_core_container_style(node_id);
    let preferred = style.size().get_abs(axis);
    let overflow = style.overflow();
    if style.is_compressible_replaced()
        || overflow.x.is_scroll_container()
        || overflow.y.is_scroll_container()
        || !style.min_size().get_abs(axis).is_auto()
        || !(preferred.is_auto() || preferred.is_intrinsic())
    {
        return false;
    }
    drop(style);

    let measured = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
    constraints.apply_automatic_minimum(axis, measured.size.get_abs(axis));
    measured.depends_on_block_constraints
}

/// One resolved intrinsic extent together with cache dependency metadata.
#[derive(Clone, Copy, Debug, Default)]
struct IntrinsicAxisValue {
    /// Resolved border-box extent, or `None` when the value is not intrinsic.
    value: Option<f32>,
    /// Whether measuring the value observed a block-constraint dependency.
    depends_on_block_constraints: bool,
}

/// Resolve a sizing value that may depend on the box's intrinsic
/// content contributions.
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
    let extent = |size: Size<f32>| size.get_abs(axis);
    if value.is_stretch() {
        return IntrinsicAxisValue { value: available_space.into_option(), depends_on_block_constraints: false };
    }
    if !value.is_intrinsic() {
        return IntrinsicAxisValue::default();
    }

    if value.is_min_content() {
        let measured = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
        return IntrinsicAxisValue {
            value: Some(extent(measured.size)),
            depends_on_block_constraints: measured.depends_on_block_constraints,
        };
    }

    let max_content = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MaxContent, axis);
    if value.is_max_content() {
        return IntrinsicAxisValue {
            value: Some(extent(max_content.size)),
            depends_on_block_constraints: max_content.depends_on_block_constraints,
        };
    }

    let min_content = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
    IntrinsicAxisValue {
        value: Some(match available_space {
            AvailableSpace::MinContent => extent(min_content.size),
            AvailableSpace::MaxContent => extent(max_content.size),
            AvailableSpace::Definite(limit) => limit.clamp(extent(min_content.size), extent(max_content.size)),
        }),
        depends_on_block_constraints: min_content.depends_on_block_constraints
            || max_content.depends_on_block_constraints,
    }
}

/// Intrinsic components of the preferred, minimum, and maximum sizes in one axis.
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

/// Authored sizing properties and available space for one physical axis.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntrinsicAxisInput {
    /// Authored preferred size in the requested axis.
    pub preferred: Dimension,
    /// Authored minimum size in the requested axis.
    pub min: Dimension,
    /// Authored maximum size in the requested axis.
    pub max: Dimension,
    /// Available border-box space after margins in the requested axis.
    pub available_space: AvailableSpace,
    /// Physical measurement axis at the tree boundary.
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
    ) -> bool {
        has_preferred_aspect_ratio
            && !is_scroll_container
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
                .applies_automatic_minimum(ratio_block_size.is_some(), auto_size_is_content_based, is_scroll_container)
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
    ) -> Self {
        Self { properties, aspect_ratio, padding_border, auto_size_is_content_based, is_scroll_container }
    }

    /// Whether the real intrinsic block contribution is required.
    #[inline(always)]
    pub(crate) fn requires_intrinsic_measurement(self) -> bool {
        self.properties.uses_intrinsic_size()
            || self.properties.applies_automatic_minimum(
                self.aspect_ratio.is_some(),
                self.auto_size_is_content_based,
                self.is_scroll_container,
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
    let child_inputs = ChildLayoutInput::new(
        inputs.known_dimensions,
        inputs.parent_size,
        inputs.parent_writing_mode,
        inputs.available_space,
        inputs.sizing_mode,
        inputs.block_margins_are_collapsible,
    )
    .with_block_auto_behavior(inputs.block_auto_behavior);
    resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        child_inputs,
        IntrinsicAxisInput { preferred, min, max, available_space: available_width, axis: AbsoluteAxis::Horizontal },
    )
}

/// Resolve intrinsic preferred/min/max contributions in the requested axis.
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

/// Resolve intrinsic inline-size/min-inline-size/max-inline-size values before a node's
/// formatting-context algorithm consumes `known_dimensions`.
///
/// This is public for custom [`LayoutPartialTree`] implementations that
/// dispatch Taffy's low-level algorithms themselves. It is a pure, pass-local
/// sizing step: recursive measurements use `SizingMode::ContentSize` and the
/// existing tree cache.
pub fn resolve_intrinsic_inline_inputs(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
) -> LayoutInput {
    resolve_intrinsic_inline_inputs_with_provenance(tree, node_id, inputs).inputs
}

/// Resolved input for an intrinsic sizing operation and the provenance needed
/// by the node sizing boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedIntrinsicInlineInputs {
    /// Layout input with intrinsic inline-size keywords resolved into a known
    /// border-box inline size.
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
/// known inline dimension does not erase the dependency that produced it.
pub fn resolve_intrinsic_inline_inputs_with_provenance(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut inputs: LayoutInput,
) -> ResolvedIntrinsicInlineInputs {
    let writing_mode = tree.get_writing_mode(node_id);
    let unchanged =
        ResolvedIntrinsicInlineInputs { inputs, depends_on_block_constraints: false, applied_aspect_ratio: false };
    // A parent-assigned size is already final, including flexible sizing and
    // intrinsic limits. Only resolve styles while this axis remains unknown.
    if inputs.sizing_mode != SizingMode::InherentSize
        || writing_mode.to_logical(inputs.known_dimensions).inline_size.is_some()
    {
        return unchanged;
    }

    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let (raw_size, raw_min_size, raw_max_size, margin, mut constraints) = {
        let aspect_ratio = tree.get_resolved_aspect_ratio(node_id);
        let style = tree.get_core_container_style(node_id);
        let raw_size = style.size();
        let raw_min_size = style.min_size();
        let raw_max_size = style.max_size();
        let raw_inline_size = writing_mode.to_logical(raw_size).inline_size;
        let raw_min_inline_size = writing_mode.to_logical(raw_min_size).inline_size;
        let raw_max_inline_size = writing_mode.to_logical(raw_max_size).inline_size;
        if ![raw_inline_size, raw_min_inline_size, raw_max_inline_size]
            .into_iter()
            .any(|value| value.is_intrinsic() || value.is_stretch())
        {
            return unchanged;
        }
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
        (
            raw_inline_size,
            raw_min_inline_size,
            raw_max_inline_size,
            margin,
            SizeConstraintInput {
                size: resolve(raw_size),
                min_size: resolve(raw_min_size),
                max_size: resolve(raw_max_size),
                size_is_auto: raw_size.map(|value| value.is_auto()),
                writing_mode,
                block_auto_behavior: inputs.block_auto_behavior,
                transferred_sizes_mode: TransferredSizesMode::Normal,
                aspect_ratio,
                padding_border: padding_border_size,
            },
        )
    };
    let available_inline_size = writing_mode
        .to_logical(inputs.available_space)
        .inline_size
        .maybe_sub(writing_mode.to_logical(margin.sum_axes()).inline_size);

    let child_inputs = ChildLayoutInput::new(
        inputs.known_dimensions,
        inputs.parent_size,
        inputs.parent_writing_mode,
        inputs.available_space,
        inputs.sizing_mode,
        inputs.block_margins_are_collapsible,
    )
    .with_block_auto_behavior(inputs.block_auto_behavior);
    let intrinsic = resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        child_inputs,
        IntrinsicAxisInput {
            preferred: raw_size,
            min: raw_min_size,
            max: raw_max_size,
            available_space: available_inline_size,
            axis: writing_mode.inline_axis(),
        },
    );
    let merge_inline = |numeric: Size<Option<f32>>, measured: Option<f32>| {
        let mut logical = writing_mode.to_logical(numeric);
        logical.inline_size = logical.inline_size.or(measured);
        writing_mode.to_physical(logical)
    };
    // Intrinsic and numeric min/max are authored constraints in the same
    // sizing operation. Resolve them together before publishing a fixed size.
    constraints.min_size = merge_inline(constraints.min_size, intrinsic.min);
    constraints.max_size = merge_inline(constraints.max_size, intrinsic.max);
    let mut resolved = resolve_size_constraints(constraints);
    let ratio_dependency = resolve_ratio_dependent_inline_minimum(tree, node_id, child_inputs, &mut resolved);
    let resolved_size = writing_mode.to_logical(resolved.size);
    let preferred = resolved_size.inline_size.or(intrinsic.preferred);
    let min_size = writing_mode.to_logical(resolved.min_size).inline_size;
    let max_size = writing_mode.to_logical(resolved.max_size).inline_size;
    let padding_border = writing_mode.to_logical(constraints.padding_border).inline_size;
    inputs.known_dimensions =
        merge_inline(inputs.known_dimensions, preferred.maybe_clamp(min_size, max_size).maybe_max(padding_border));
    ResolvedIntrinsicInlineInputs {
        inputs,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints || ratio_dependency,
        applied_aspect_ratio: writing_mode.to_logical(resolved.aspect_ratio_applied).inline_size,
    }
}
