//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use super::aspect_ratio::{
    apply_preferred_aspect_ratio, resolve_size_constraints, ResolvedAxisConstraints, ResolvedSizeConstraints,
    SizeConstraintInput, TransferredSizesMode,
};
use super::used_size::{
    resolve_inline_auto_size, resolve_stretch_axis_value, resolve_used_size, stretch_border_box_available_space,
    SizeConstraintRole, StretchSizeProperties,
};

/// Substitute a contained intrinsic border-box size for authored intrinsic
/// sizing keywords, then reapply the source-ordered minimum/maximum clamp.
///
/// The logical inline path may already have measured the same contribution;
/// assignment rather than fallback is deliberate so containment remains the
/// authoritative content source in either physical axis.
pub(crate) fn apply_contained_intrinsic_size_constraints(
    mut resolved: ResolvedSizeConstraints,
    raw_size: Size<Dimension>,
    raw_min_size: Size<Dimension>,
    raw_max_size: Size<Dimension>,
    contained_outer_size: Size<Option<f32>>,
) -> ResolvedSizeConstraints {
    if raw_size.width.is_intrinsic() && contained_outer_size.width.is_some() {
        resolved.size.width = contained_outer_size.width;
        resolved.aspect_ratio_applied.width = false;
    }
    if raw_size.height.is_intrinsic() && contained_outer_size.height.is_some() {
        resolved.size.height = contained_outer_size.height;
        resolved.aspect_ratio_applied.height = false;
    }
    let late_min_size = Size {
        width: raw_min_size.width.is_intrinsic().then_some(contained_outer_size.width).flatten(),
        height: raw_min_size.height.is_intrinsic().then_some(contained_outer_size.height).flatten(),
    };
    let late_max_size = Size {
        width: raw_max_size.width.is_intrinsic().then_some(contained_outer_size.width).flatten(),
        height: raw_max_size.height.is_intrinsic().then_some(contained_outer_size.height).flatten(),
    };
    resolved.apply_late_authored_constraints(late_min_size, late_max_size);
    resolved.size = resolved.size.maybe_clamp(resolved.min_size, resolved.max_size);
    resolved
}
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
#[derive(Clone, Copy)]
struct IntrinsicAxisValueInput {
    /// Authored sizing value to resolve.
    value: Dimension,
    /// Margin-adjusted border-box opportunity in the selected axis.
    available_space: AvailableSpace,
    /// Physical axis selected by the owning formatting context.
    axis: AbsoluteAxis,
    /// Ratio-derived substitute for intrinsic content measurement.
    ratio_content_contribution: RatioDependentContentContribution,
    /// Whether the value is a preferred, minimum, or maximum size.
    role: SizeConstraintRole,
}

/// Resolve one intrinsic or stretch sizing property at a formatting-context
/// boundary.
fn resolve_intrinsic_axis_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    value_input: IntrinsicAxisValueInput,
) -> IntrinsicAxisValue {
    let IntrinsicAxisValueInput { value, available_space, axis, ratio_content_contribution, role } = value_input;
    if value.is_stretch() {
        return IntrinsicAxisValue {
            value: resolve_stretch_axis_value(value, role, available_space, 0.0),
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
    // CSS intrinsic sizes are an ordered pair. Negative margins and
    // compatibility algorithms can make their raw sums cross, in which case
    // max-content encompasses min-content. Spell out the fit-content formula
    // instead of relying on `f32::clamp`, whose precondition would turn this
    // valid CSS case into a panic.
    let max_content_size = max_content.size.get_abs(axis).max(min_content_size);
    IntrinsicAxisValue {
        value: Some(match available_space {
            AvailableSpace::MinContent => min_content_size,
            AvailableSpace::MaxContent => max_content_size,
            AvailableSpace::Definite(limit) => limit.max(min_content_size).min(max_content_size),
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

/// Initial geometry available while measuring an intrinsic contribution.
///
/// A parent-owned fixed dimension and a directly authored dimension may have
/// the same numeric value but different definiteness. Keeping both projections
/// prevents an overridden child size from becoming a percentage basis.
#[derive(Clone, Copy, Debug, PartialEq)]
struct IntrinsicMeasurementGeometry {
    /// Physical dimensions known to the intrinsic measurement operation.
    known_dimensions: Size<Option<f32>>,
    /// Subset of known dimensions that are definite for descendant sizing.
    definite_dimensions: Size<Option<f32>>,
}

impl IntrinsicMeasurementGeometry {
    /// Merge parent-owned overrides with node-owned authored geometry.
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

/// Add the node's directly authored perpendicular-axis geometry to an
/// intrinsic content probe without changing the parent-owned constraint space.
fn resolve_intrinsic_measurement_input(
    tree: &impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut inputs: ChildLayoutInput,
) -> ChildLayoutInput {
    let percentage_basis = inputs.parent_writing_mode.to_logical(inputs.parent_size).inline_size;
    let (own_definite_dimensions, own_min_size, own_max_size) = {
        let style = tree.get_core_container_style(node_id);
        let padding = style.padding().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let box_sizing_adjustment =
            if style.box_sizing() == BoxSizing::ContentBox { (padding + border).sum_axes() } else { Size::ZERO };
        let resolve = |size: Size<Dimension>| {
            size.maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_add(box_sizing_adjustment)
        };
        let min_size = resolve(style.min_size());
        let max_size = resolve(style.max_size());
        (resolve(style.size()).maybe_clamp(min_size, max_size), min_size, max_size)
    };
    let geometry = IntrinsicMeasurementGeometry::resolve(
        inputs.known_dimensions,
        inputs.definite_dimensions,
        own_definite_dimensions,
    );
    inputs.known_dimensions = geometry.known_dimensions;
    inputs.definite_dimensions = geometry.definite_dimensions;
    // The queried axis is replaced by its min/max-content constraint in
    // `measure_intrinsic_axis`. On the perpendicular axis, however, authored
    // min/max constraints still bound the layout opportunity. In particular,
    // a wrapped column flexbox uses `max-block-size` as its line length while
    // its inline contribution is being measured.
    inputs.available_space = inputs.available_space.maybe_min(own_max_size).maybe_max(own_min_size);
    inputs
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
        IntrinsicAxisValueInput {
            value,
            available_space,
            axis,
            ratio_content_contribution: RatioDependentContentContribution::default(),
            role: SizeConstraintRole::Preferred,
        },
    )
}

/// Intrinsic components of preferred, minimum and maximum sizes in one axis.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain values
/// that ordinary length-percentage resolution could not reduce: intrinsic
/// values measure content, while `stretch` delegates to the shared used-size
/// resolver.
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
    /// Authored or formatting-context-selected preferred size source.
    pub preferred: IntrinsicPreferredSize,
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

/// Provenance of the preferred size supplied to intrinsic-axis resolution.
///
/// An automatic shrink-to-fit size is deliberately not rewritten into an
/// authored `fit-content` value. Intrinsic contribution requests leave
/// `inline-size: auto` authored and let the owning formatting algorithm compute
/// the requested contribution. During final layout, this variant selects the
/// fit-content formula even when the containing constraint is min-content or
/// max-content. Blink retains the same distinction through
/// `ConstraintSpace::InlineAutoBehavior` and the separate `auto_length` passed
/// to `ResolveMainInlineLength`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum IntrinsicPreferredSize {
    /// The preferred size comes directly from the computed style.
    Authored(Dimension),
    /// `inline-size:auto` is shrink-to-fit in this constraint space.
    AutomaticFitContent,
}

impl IntrinsicPreferredSize {
    /// Preserve an automatic fit-content decision made by the containing
    /// formatting context while leaving all other values authored.
    #[inline(always)]
    fn for_node_inline_size(
        authored: Dimension,
        sizing_purpose: crate::tree::SizingPurpose,
        auto_behavior: crate::AutoSizeBehavior,
        automatic_resolution: AutomaticInlineSizeResolution,
    ) -> Self {
        if authored.is_auto()
            && sizing_purpose == crate::tree::SizingPurpose::Layout
            && auto_behavior == crate::AutoSizeBehavior::FitContent
            && automatic_resolution == AutomaticInlineSizeResolution::FitContent
        {
            Self::AutomaticFitContent
        } else {
            Self::Authored(authored)
        }
    }
}

/// Selects which layer resolves an automatic inline size during final layout.
///
/// Most formatting algorithms can use the shared fit-content measurement.
/// Grid defers because auto-repeat track counts are resolved from its final
/// ratio-constrained size and may require an algorithm-owned rerun.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutomaticInlineSizeResolution {
    /// Resolve `inline-size: auto` through the shared fit-content path.
    FitContent,
    /// Leave the automatic inline size to the formatting algorithm.
    DeferToFormattingContext,
}

/// Resolve the preferred intrinsic value without erasing whether fit-content
/// came from style or from the current formatting context.
fn resolve_intrinsic_preferred_axis_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: ChildLayoutInput,
    preferred: IntrinsicPreferredSize,
    available_space: AvailableSpace,
    axis: AbsoluteAxis,
    ratio_content_contribution: RatioDependentContentContribution,
) -> IntrinsicAxisValue {
    let value = match preferred {
        IntrinsicPreferredSize::Authored(value) => value,
        IntrinsicPreferredSize::AutomaticFitContent => Dimension::fit_content(),
    };
    resolve_intrinsic_axis_value(
        tree,
        node_id,
        inputs,
        IntrinsicAxisValueInput {
            value,
            available_space,
            axis,
            ratio_content_contribution,
            role: SizeConstraintRole::Preferred,
        },
    )
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
pub(crate) struct ContentBasedSizeConstraints {
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

impl ContentBasedSizeConstraints {
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

    /// Merge content-derived values into the logical block axis after the
    /// formatting context has established its final inline size.
    pub(crate) fn apply_to_block_axis(
        self,
        writing_mode: WritingMode,
        constraint_sources: ResolvedAxisConstraints,
        minimum_border_box_size: Size<f32>,
        size: &mut Size<Option<f32>>,
        min_size: &mut Size<Option<f32>>,
        max_size: &mut Size<Option<f32>>,
    ) {
        let mut logical_size = writing_mode.to_logical(*size);
        let mut logical_min_size = writing_mode.to_logical(*min_size);
        let mut logical_max_size = writing_mode.to_logical(*max_size);
        let resolved = self.resolve_against(logical_size.block_size, constraint_sources);
        let minimum_border_box_size = writing_mode.to_logical(minimum_border_box_size).block_size;

        logical_size.block_size = resolved.preferred;
        logical_min_size.block_size = resolved.min.or(Some(minimum_border_box_size)).maybe_max(minimum_border_box_size);
        logical_max_size.block_size = resolved.max;
        logical_size.block_size = logical_size
            .block_size
            .maybe_clamp(logical_min_size.block_size, logical_max_size.block_size)
            .maybe_max(minimum_border_box_size);

        *size = writing_mode.to_physical(logical_size);
        *min_size = writing_mode.to_physical(logical_min_size);
        *max_size = writing_mode.to_physical(logical_max_size);
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

    /// Whether an automatic preferred block size is transferred through the
    /// preferred aspect ratio after the formatting context establishes its
    /// final inline size.
    #[inline(always)]
    fn resolves_auto_size_from_ratio(self, has_preferred_aspect_ratio: bool, auto_size_is_content_based: bool) -> bool {
        self.preferred.is_auto() && has_preferred_aspect_ratio && auto_size_is_content_based
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
    ) -> ContentBasedSizeConstraints {
        let content_block_size = ratio_block_size.unwrap_or(intrinsic_border_box_size);
        let resolve_explicit = |value: Dimension| value.is_intrinsic().then_some(content_block_size);
        let resolves_auto_size_from_ratio =
            self.resolves_auto_size_from_ratio(ratio_block_size.is_some(), auto_size_is_content_based);
        ContentBasedSizeConstraints {
            preferred: resolve_explicit(self.preferred)
                .or_else(|| resolves_auto_size_from_ratio.then_some(ratio_block_size).flatten()),
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
    /// Formatting-context-selected intrinsic border-box substitute.
    ///
    /// Size containment normally replaces descendant contributions before
    /// this resolver runs. Grid is the exception: its substitute is derived
    /// from tracks sized without items, so it only becomes available after
    /// track initialization.
    intrinsic_border_box_override: Option<f32>,
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
        Self {
            properties,
            aspect_ratio,
            padding_border,
            auto_size_is_content_based,
            is_scroll_container,
            is_replaced,
            intrinsic_border_box_override: None,
        }
    }

    /// Replace the real intrinsic block contribution at the formatting
    /// context boundary.
    #[inline(always)]
    pub(crate) const fn with_intrinsic_border_box_override(
        mut self,
        intrinsic_border_box_override: Option<f32>,
    ) -> Self {
        self.intrinsic_border_box_override = intrinsic_border_box_override;
        self
    }

    /// Whether content-based block-axis properties need to be resolved.
    #[inline(always)]
    pub(crate) fn requires_resolution(self) -> bool {
        let has_preferred_aspect_ratio = self.aspect_ratio.is_some();
        self.properties.uses_intrinsic_size()
            || self
                .properties
                .resolves_auto_size_from_ratio(has_preferred_aspect_ratio, self.auto_size_is_content_based)
            || self.properties.applies_automatic_minimum(
                has_preferred_aspect_ratio,
                self.auto_size_is_content_based,
                self.is_scroll_container,
                self.is_replaced,
            )
    }

    /// Whether the automatic preferred block size becomes definite through
    /// the preferred ratio after the formatting context resolves inline size.
    #[inline(always)]
    pub(crate) fn resolves_auto_size_from_ratio(self) -> bool {
        self.properties.resolves_auto_size_from_ratio(self.aspect_ratio.is_some(), self.auto_size_is_content_based)
    }

    /// Whether the real intrinsic block contribution is required.
    #[inline(always)]
    pub(crate) fn requires_intrinsic_measurement(self) -> bool {
        self.intrinsic_border_box_override.is_none()
            && (self.properties.uses_intrinsic_size()
                || self.properties.applies_automatic_minimum(
                    self.aspect_ratio.is_some(),
                    self.auto_size_is_content_based,
                    self.is_scroll_container,
                    self.is_replaced,
                ))
    }

    /// Resolve the content-derived block-axis constraints.
    #[inline(always)]
    pub(crate) fn resolve(
        self,
        writing_mode: WritingMode,
        outer_inline_size: Option<f32>,
        intrinsic_border_box_size: f32,
    ) -> ContentBasedSizeConstraints {
        let physical_size = writing_mode.to_physical(LogicalSize { inline_size: outer_inline_size, block_size: None });
        let ratio_size = physical_size.maybe_apply_aspect_ratio_with_box_sizing(
            self.aspect_ratio,
            BoxSizing::BorderBox,
            self.padding_border,
        );
        let ratio_block_size = writing_mode.to_logical(ratio_size).block_size;
        self.properties.resolve(
            self.intrinsic_border_box_override.unwrap_or(intrinsic_border_box_size),
            ratio_block_size,
            self.auto_size_is_content_based,
            self.is_scroll_container,
            self.is_replaced,
        )
    }
}

/// Resolve content-based block-size properties after the caller establishes
/// the box's inline-size constraint.
///
/// Intrinsic keywords and the automatic minimum measure real content. A
/// ratio-only automatic size can be resolved directly from the final inline
/// size without recursively measuring the node.
pub(crate) fn resolve_content_based_block_size_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut child_input: ChildLayoutInput,
    resolver: ContentBasedBlockSize,
) -> ContentBasedSizeConstraints {
    if !resolver.requires_resolution() {
        return ContentBasedSizeConstraints::default();
    }

    let writing_mode = tree.get_writing_mode(node_id);
    let mut known_logical_size = writing_mode.to_logical(child_input.known_dimensions);
    let mut definite_logical_size = writing_mode.to_logical(child_input.definite_dimensions);
    let outer_inline_size = known_logical_size.inline_size;
    if !resolver.requires_intrinsic_measurement() {
        return resolver.resolve(writing_mode, outer_inline_size, 0.0);
    }
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
    .with_ignored_margins_for_stretch(inputs.ignored_margins_for_stretch)
    .with_orthogonal_fallback(inputs.orthogonal_fallback);
    resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        child_input,
        IntrinsicAxisInput {
            preferred: IntrinsicPreferredSize::Authored(width_input.preferred),
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
        preferred: resolve_intrinsic_preferred_axis_value(
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
            IntrinsicAxisValueInput {
                value: min,
                available_space,
                axis,
                ratio_content_contribution,
                role: SizeConstraintRole::Minimum,
            },
        ),
        max: resolve_intrinsic_axis_value(
            tree,
            node_id,
            inputs,
            IntrinsicAxisValueInput {
                value: max,
                available_space,
                axis,
                ratio_content_contribution,
                role: SizeConstraintRole::Maximum,
            },
        ),
    }
}

/// Inputs needed to resolve the node-owned preferred and limiting sizes.
///
/// Formatting algorithms resolve their own decoration before entering the
/// shared sizing boundary. Parent-owned fixed geometry remains exclusively in
/// [`LayoutInput::known_dimensions`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSizeConstraintInput {
    /// Authored physical preferred size.
    pub raw_size: Size<Dimension>,
    /// Authored physical minimum size.
    pub raw_min_size: Size<Dimension>,
    /// Authored physical maximum size.
    pub raw_max_size: Size<Dimension>,
    /// Content-box adjustment applied to resolved authored lengths.
    pub box_sizing_adjustment: Size<f32>,
    /// Physical padding-and-border sums.
    pub padding_border_size: Size<f32>,
    /// Used preferred aspect ratio.
    pub aspect_ratio: Option<ResolvedAspectRatio>,
    /// Formatting-context-selected size-containment substitute, including
    /// decoration.
    pub contained_outer_size: Size<Option<f32>>,
    /// Owner of automatic inline-size resolution for this formatting context.
    pub automatic_inline_size_resolution: AutomaticInlineSizeResolution,
}

/// Child-owned initial geometry derived from style and a constraint space.
///
/// This is deliberately separate from [`LayoutInput::known_dimensions`]. A
/// known dimension is an exact used size fixed by the parent formatting
/// context; these values are the node's own preferred/minimum/maximum sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedNodeSizing {
    /// Preferred physical border-box size after intrinsic keywords, automatic
    /// inline sizing and preferred-ratio transfer.
    pub preferred_size: Size<Option<f32>>,
    /// Used physical minimum constraints.
    pub min_size: Size<Option<f32>>,
    /// Used physical maximum constraints.
    pub max_size: Size<Option<f32>>,
    /// Initial physical border-box geometry after parent-fixed dimensions and
    /// child-owned sizing have been combined.
    pub outer_size: Size<Option<f32>>,
    /// Used physical dimensions that are definite percentage bases for
    /// descendants.
    pub definite_size: Size<Option<f32>>,
    /// Whether resolving the logical inline axis measured content dependent
    /// on the containing block's block constraint.
    pub depends_on_block_constraints: bool,
    /// Whether the logical inline size was synthesized through the preferred
    /// aspect ratio.
    pub applied_aspect_ratio: bool,
    /// Source-preserving constraints used by later formatting-context sizing.
    pub(crate) constraints: ResolvedSizeConstraints,
}

impl ResolvedNodeSizing {
    /// Empty child-owned sizing state for a content-only measurement.
    const NONE: Self = Self {
        preferred_size: Size::NONE,
        min_size: Size::NONE,
        max_size: Size::NONE,
        outer_size: Size::NONE,
        definite_size: Size::NONE,
        depends_on_block_constraints: false,
        applied_aspect_ratio: false,
        constraints: ResolvedSizeConstraints::NONE,
    };
}

/// Keep the final used value only on axes with a definite sizing source.
#[inline(always)]
fn used_definite_size(
    used_size: Size<Option<f32>>,
    known_size: Size<Option<f32>>,
    parent_definite_size: Size<Option<f32>>,
    own_definite_size: Size<Option<f32>>,
) -> Size<Option<f32>> {
    let definite_source = Size {
        width: if known_size.width.is_some() {
            parent_definite_size.width
        } else {
            parent_definite_size.width.or(own_definite_size.width)
        },
        height: if known_size.height.is_some() {
            parent_definite_size.height
        } else {
            parent_definite_size.height.or(own_definite_size.height)
        },
    };
    Size { width: definite_source.width.and(used_size.width), height: definite_source.height.and(used_size.height) }
}

/// Resolve a node's initial sizing geometry without mutating its parent-owned
/// constraint space.
///
/// This is the Taffy counterpart of Blink's initial-fragment geometry
/// boundary: intrinsic keywords and authored sizes stay child-owned, while
/// `known_dimensions` remains an exact size imposed by the parent.
pub(crate) fn resolve_node_size_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    sizing: NodeSizeConstraintInput,
) -> ResolvedNodeSizing {
    if inputs.sizing_mode == SizingMode::ContentSize {
        return ResolvedNodeSizing {
            preferred_size: sizing.contained_outer_size,
            outer_size: inputs.known_dimensions.or(sizing.contained_outer_size),
            definite_size: inputs.definite_dimensions,
            ..ResolvedNodeSizing::NONE
        };
    }

    let NodeSizeConstraintInput {
        raw_size,
        raw_min_size,
        raw_max_size,
        box_sizing_adjustment,
        padding_border_size,
        aspect_ratio,
        contained_outer_size,
        automatic_inline_size_resolution,
    } = sizing;
    let writing_mode = tree.get_writing_mode(node_id);
    let inline_axis = writing_mode.inline_axis();
    let size_is_auto = raw_size.map(Dimension::is_auto);
    let mut direct_size = raw_size
        .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
        .maybe_add(box_sizing_adjustment);
    let mut direct_min_size = raw_min_size
        .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
        .maybe_add(box_sizing_adjustment);
    let mut direct_max_size = raw_max_size
        .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
        .maybe_add(box_sizing_adjustment);

    // Explicit stretch resolves against the margin-box opportunity supplied
    // by the containing formatting context. Only the margin sides named by
    // the constraint space are omitted; ordinary auto sizing below continues
    // to subtract every margin.
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let margin = tree
        .get_core_container_style(node_id)
        .margin()
        .resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
    let stretch_available_space =
        stretch_border_box_available_space(inputs.available_space, margin, inputs.ignored_margins_for_stretch);
    let stretch = StretchSizeProperties::new(raw_size, raw_min_size, raw_max_size)
        .resolve(stretch_available_space, padding_border_size);
    direct_size = direct_size.or(stretch.preferred);
    direct_min_size = direct_min_size.or(stretch.min);
    direct_max_size = direct_max_size.or(stretch.max);

    let logical_raw_size = writing_mode.to_logical(raw_size);
    let logical_raw_min_size = writing_mode.to_logical(raw_min_size);
    let logical_raw_max_size = writing_mode.to_logical(raw_max_size);
    let logical_direct_size = writing_mode.to_logical(direct_size);
    let logical_direct_min_size = writing_mode.to_logical(direct_min_size);
    let logical_direct_max_size = writing_mode.to_logical(direct_max_size);
    let opposite_axis_depends_on_parent =
        [logical_raw_size.block_size, logical_raw_min_size.block_size, logical_raw_max_size.block_size]
            .into_iter()
            .any(|value| value.may_have_percentage_dependence() || value.is_stretch());
    let ratio_content_contribution = resolve_ratio_dependent_content_contribution(
        direct_size,
        direct_min_size,
        direct_max_size,
        aspect_ratio,
        padding_border_size,
        inline_axis,
        opposite_axis_depends_on_parent && aspect_ratio.is_some(),
    );
    // Like Blink's ConstraintSpace, LayoutInput carries the margin-box
    // opportunity. Node-owned sizing removes its own non-auto margins before
    // resolving stretch and fit-content. Keeping this at the node boundary
    // gives leaf and container algorithms the same contract while allowing a
    // parent formatting context to position the resulting margin box.
    let available_space = inputs.available_space.maybe_sub(margin.sum_axes());
    let available_inline_size = available_space.get_abs(inline_axis);
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
    .with_ignored_margins_for_stretch(inputs.ignored_margins_for_stretch)
    .with_orthogonal_fallback(inputs.orthogonal_fallback);
    let intrinsic = resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        child_input,
        IntrinsicAxisInput {
            preferred: IntrinsicPreferredSize::for_node_inline_size(
                logical_raw_size.inline_size,
                inputs.sizing_purpose,
                inputs.inline_auto_behavior,
                automatic_inline_size_resolution,
            ),
            min: logical_raw_min_size.inline_size,
            max: logical_raw_max_size.inline_size,
            available_space: available_inline_size,
            axis: inline_axis,
            ratio_content_contribution,
        },
    );
    let mut logical_preferred_size = logical_direct_size;
    logical_preferred_size.inline_size = logical_preferred_size.inline_size.or(intrinsic.preferred.value);
    let mut logical_min_size = logical_direct_min_size;
    logical_min_size.inline_size = logical_min_size.inline_size.or(intrinsic.min.value);
    let mut logical_max_size = logical_direct_max_size;
    logical_max_size.inline_size = logical_max_size.inline_size.or(intrinsic.max.value);
    direct_min_size = writing_mode.to_physical(logical_min_size);
    direct_max_size = writing_mode.to_physical(logical_max_size);

    let mut resolved = resolve_size_constraints(SizeConstraintInput {
        size: writing_mode.to_physical(logical_preferred_size),
        min_size: direct_min_size,
        max_size: direct_max_size,
        size_is_auto,
        writing_mode,
        inline_auto_behavior: inputs.inline_auto_behavior,
        block_auto_behavior: inputs.block_auto_behavior,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border: padding_border_size,
    });
    if intrinsic.preferred.applied_aspect_ratio {
        match inline_axis {
            AbsoluteAxis::Horizontal => resolved.aspect_ratio_applied.width = true,
            AbsoluteAxis::Vertical => resolved.aspect_ratio_applied.height = true,
        }
    }
    let resolved = apply_contained_intrinsic_size_constraints(
        resolved,
        raw_size,
        raw_min_size,
        raw_max_size,
        contained_outer_size,
    );

    let min_max_definite_size = resolved.min_size.zip_map(resolved.max_size, |min, max| match (min, max) {
        (Some(min), Some(max)) if max <= min => Some(min),
        _ => None,
    });
    let preferred_size = resolve_inline_auto_size(
        min_max_definite_size.or(resolved.size.maybe_clamp(resolved.min_size, resolved.max_size)),
        size_is_auto,
        writing_mode,
        inputs.inline_auto_behavior,
        available_space,
    )
    .or(contained_outer_size.maybe_clamp(resolved.min_size, resolved.max_size));
    let size_before_fixed_ratio = resolve_used_size(
        inputs.known_dimensions,
        preferred_size,
        resolved.min_size,
        resolved.max_size,
        padding_border_size,
    );
    let size_after_fixed_ratio = apply_preferred_aspect_ratio(
        size_before_fixed_ratio,
        size_is_auto,
        writing_mode,
        inputs.inline_auto_behavior,
        inputs.block_auto_behavior,
        aspect_ratio,
        padding_border_size,
    );
    let outer_size = resolve_used_size(
        inputs.known_dimensions,
        size_after_fixed_ratio,
        resolved.min_size,
        resolved.max_size,
        padding_border_size,
    );

    let direct_constraints = resolve_size_constraints(SizeConstraintInput {
        size: direct_size,
        min_size: direct_min_size,
        max_size: direct_max_size,
        size_is_auto,
        writing_mode,
        inline_auto_behavior: inputs.inline_auto_behavior,
        block_auto_behavior: inputs.block_auto_behavior,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border: padding_border_size,
    });
    let own_definite_size = resolve_inline_auto_size(
        direct_constraints.size.maybe_clamp(direct_constraints.min_size, direct_constraints.max_size),
        size_is_auto,
        writing_mode,
        inputs.inline_auto_behavior,
        available_space,
    );
    let definite_size =
        used_definite_size(outer_size, inputs.known_dimensions, inputs.definite_dimensions, own_definite_size);
    let fixed_ratio_applied =
        size_before_fixed_ratio.get_abs(inline_axis).is_none() && size_after_fixed_ratio.get_abs(inline_axis).is_some();
    let applied_aspect_ratio = inputs.known_dimensions.get_abs(inline_axis).is_none()
        && min_max_definite_size.get_abs(inline_axis).is_none()
        && (resolved.aspect_ratio_applied.get_abs(inline_axis) || fixed_ratio_applied);

    ResolvedNodeSizing {
        preferred_size,
        min_size: resolved.min_size,
        max_size: resolved.max_size,
        outer_size,
        definite_size,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints(),
        applied_aspect_ratio,
        constraints: resolved,
    }
}

/// Resolve leaf sizing directly from its style projection.
///
/// Custom tree adapters call this after a cache miss, so the raw parent
/// constraint space remains the cache key.
pub fn resolve_leaf_node_sizing(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
) -> ResolvedNodeSizing {
    let writing_mode = tree.get_writing_mode(node_id);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let sizing = {
        let aspect_ratio = tree.get_resolved_aspect_ratio(node_id);
        let size_containment = tree.get_size_containment(node_id);
        let scrollbar_insets = tree.get_scrollbar_insets(node_id);
        let style = tree.get_core_container_style(node_id);
        let padding = style.padding().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let padding_border_size = (padding + border).sum_axes();
        NodeSizeConstraintInput {
            raw_size: style.size(),
            raw_min_size: style.min_size(),
            raw_max_size: style.max_size(),
            box_sizing_adjustment: if style.box_sizing() == BoxSizing::ContentBox {
                padding_border_size
            } else {
                Size::ZERO
            },
            padding_border_size,
            aspect_ratio,
            contained_outer_size: size_containment
                .resolve_outer_size(Size::ZERO, padding_border_size + scrollbar_insets.sum_axes()),
            automatic_inline_size_resolution: AutomaticInlineSizeResolution::FitContent,
        }
    };
    resolve_node_size_constraints(tree, node_id, inputs, sizing)
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
