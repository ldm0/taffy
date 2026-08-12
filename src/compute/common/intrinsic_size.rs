//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use crate::geometry::{AbsoluteAxis, LogicalSize, Size, WritingMode};
use crate::style::{AvailableSpace, CoreStyle, Dimension, Overflow};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, RequestedAxis,
    SizingMode,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, ResolvedAspectRatio};

use super::aspect_ratio::{
    apply_preferred_aspect_ratio, resolve_size_constraints, ResolvedAxisConstraints, ResolvedSizeConstraints,
    SizeConstraintInput, TransferredSizesMode,
};
use super::stretch::resolve_stretch_size_constraints;
use super::used_size::{resolve_inline_auto_size_preference, resolve_used_size, InlineAutoSizeInput};

/// Substitute a contained intrinsic border-box size for intrinsic sizing
/// keywords, then reapply the normal minimum-wins clamp.
///
/// Length/percentage resolution deliberately leaves intrinsic keywords
/// unresolved. Size containment makes their min-content and max-content
/// contributions equal, so formatting contexts can complete that shared step
/// without recursively measuring descendants.
pub(crate) fn apply_contained_intrinsic_size_constraints(
    mut resolved: ResolvedSizeConstraints,
    raw_size: Size<Dimension>,
    raw_min_size: Size<Dimension>,
    raw_max_size: Size<Dimension>,
    contained_outer_size: Size<Option<f32>>,
) -> ResolvedSizeConstraints {
    resolved.size = Size {
        width: resolved.size.width.or(raw_size.width.is_intrinsic().then_some(contained_outer_size.width).flatten()),
        height: resolved.size.height.or(raw_size
            .height
            .is_intrinsic()
            .then_some(contained_outer_size.height)
            .flatten()),
    };
    let late_min_size = Size {
        width: raw_min_size.width.is_intrinsic().then_some(contained_outer_size.width).flatten(),
        height: raw_min_size.height.is_intrinsic().then_some(contained_outer_size.height).flatten(),
    };
    let late_max_size = Size {
        width: raw_max_size.width.is_intrinsic().then_some(contained_outer_size.width).flatten(),
        height: raw_max_size.height.is_intrinsic().then_some(contained_outer_size.height).flatten(),
    };
    resolved.apply_late_authored_constraints(late_min_size, late_max_size);
    resolved.size = resolved.used_preferred_size();
    resolved
}

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
            inputs.block_margins_are_collapsible,
        ),
        RequestedAxis::from(axis),
    )
}

/// One resolved intrinsic axis value together with cache dependency metadata.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicAxisValue {
    /// Resolved border-box size, or `None` when the value is not intrinsic.
    pub value: Option<f32>,
    /// Whether measuring the value observed a block-constraint dependency.
    pub depends_on_block_constraints: bool,
}

/// A measured aspect-ratio automatic minimum and its cache dependency.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AutomaticMinimum {
    /// Content-derived border-box minimum, if the rule applies.
    pub value: Option<f32>,
    /// Whether measuring it observed a containing-block block constraint.
    pub depends_on_block_constraints: bool,
}

/// Measure CSS Sizing's aspect-ratio automatic minimum at the shared child
/// sizing seam.
///
/// The returned value is intentionally not clamped here. The source-preserving
/// aspect-ratio resolver applies authored maximums before transferred
/// constraints, so block, flex, and grid all consume the same ordering.
pub(crate) fn measure_aspect_ratio_automatic_minimum(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    axis: AbsoluteAxis,
    padding_border: Size<f32>,
    resolved: ResolvedSizeConstraints,
) -> AutomaticMinimum {
    let known_size = match axis {
        AbsoluteAxis::Horizontal => inputs.known_dimensions.width,
        AbsoluteAxis::Vertical => inputs.known_dimensions.height,
    };
    let ratio_was_applied = match axis {
        AbsoluteAxis::Horizontal => resolved.aspect_ratio_applied.width,
        AbsoluteAxis::Vertical => resolved.aspect_ratio_applied.height,
    };
    if inputs.sizing_mode != SizingMode::InherentSize
        || !inputs.axis.contains(axis)
        || known_size.is_some()
        || !ratio_was_applied
    {
        return AutomaticMinimum::default();
    }

    let (preferred, minimum, is_scroll_container) = {
        let style = tree.get_core_container_style(node_id);
        let preferred = match axis {
            AbsoluteAxis::Horizontal => style.size().width,
            AbsoluteAxis::Vertical => style.size().height,
        };
        let minimum = match axis {
            AbsoluteAxis::Horizontal => style.min_size().width,
            AbsoluteAxis::Vertical => style.min_size().height,
        };
        let overflow = style.overflow();
        (preferred, minimum, overflow.x.is_scroll_container() || overflow.y.is_scroll_container())
    };
    if is_scroll_container || !minimum.is_auto() || !(preferred.is_auto() || preferred.is_intrinsic()) {
        return AutomaticMinimum::default();
    }

    let contained_outer_size = tree.get_size_containment(node_id).resolve_explicit_outer_size(padding_border);
    let contained_minimum = match axis {
        AbsoluteAxis::Horizontal => contained_outer_size.width,
        AbsoluteAxis::Vertical => contained_outer_size.height,
    };
    if let Some(value) = contained_minimum {
        return AutomaticMinimum { value: Some(value), depends_on_block_constraints: false };
    }

    let measured = measure_intrinsic_axis(tree, node_id, inputs, AvailableSpace::MinContent, axis);
    AutomaticMinimum {
        value: Some(match axis {
            AbsoluteAxis::Horizontal => measured.size.width,
            AbsoluteAxis::Vertical => measured.size.height,
        }),
        depends_on_block_constraints: measured.depends_on_block_constraints,
    }
}

/// The sizing property whose intrinsic value is being resolved.
///
/// CSS gives cyclic percentages in preferred, minimum, and maximum sizes
/// different initial values. Keeping that role explicit mirrors the property
/// mode carried by browser constraint-space implementations and prevents the
/// three callers from growing independent fallback rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntrinsicSizeRole {
    /// The preferred `width`/`height` property.
    Preferred,
    /// The minimum size property.
    Minimum,
    /// The maximum size property.
    Maximum,
}

/// Context needed to resolve the argument of `fit-content()`.
#[derive(Clone, Copy, Debug, Default)]
struct FitContentContext {
    /// Containing-block size for the percentage component of the argument.
    percentage_basis: Option<f32>,
    /// Amount needed to convert the selected sizing box to a border box.
    box_sizing_adjustment: f32,
}

/// Resolve decoration and percentage context for a parameterized
/// `fit-content()` value.
fn fit_content_context(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    axis: AbsoluteAxis,
    percentage_basis: Option<f32>,
    is_required: bool,
) -> FitContentContext {
    if !is_required {
        return FitContentContext::default();
    }

    let decoration_percentage_basis =
        inputs.constraint_space(tree.get_writing_mode(node_id)).margin_padding_percentage_basis();
    let (padding, border, box_sizing) = {
        let style = tree.get_core_container_style(node_id);
        (style.padding(), style.border(), style.box_sizing())
    };
    let padding = padding.resolve_or_zero(decoration_percentage_basis, |value, basis| tree.calc(value, basis));
    let border = border.resolve_or_zero(decoration_percentage_basis, |value, basis| tree.calc(value, basis));
    let box_sizing_adjustment =
        if box_sizing == BoxSizing::ContentBox { (padding + border).sum_axes().get_abs(axis) } else { 0.0 };
    FitContentContext { percentage_basis, box_sizing_adjustment }
}

/// Pass-local inputs for resolving one intrinsic sizing property.
#[derive(Clone, Copy, Debug)]
struct IntrinsicValueInput {
    /// Layout constraints supplied by the owning formatting context.
    layout: LayoutInput,
    /// Available border-box space after margins in the selected axis.
    available_space: AvailableSpace,
    /// Physical axis of the sizing property.
    axis: AbsoluteAxis,
    /// Preferred, minimum, or maximum property semantics.
    role: IntrinsicSizeRole,
    /// Resolution data shared by parameterized fit-content values.
    fit_content: FitContentContext,
}

/// Resolve one sizing value that may depend on the box's intrinsic content
/// contributions.
///
/// `available_size` is the border-box space left after margins in `axis`.
/// Returned values are border-box sizes, matching `LayoutInput::known_dimensions`.
fn resolve_intrinsic_axis_value(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    value: Dimension,
    value_input: IntrinsicValueInput,
) -> IntrinsicAxisValue {
    let IntrinsicValueInput { layout, available_space, axis, role, fit_content } = value_input;
    if !value.is_intrinsic() {
        return IntrinsicAxisValue::default();
    }

    if value.is_min_content() {
        let measured = measure_intrinsic_axis(tree, node_id, layout, AvailableSpace::MinContent, axis);
        return IntrinsicAxisValue {
            value: Some(measured.size.get_abs(axis)),
            depends_on_block_constraints: measured.depends_on_block_constraints,
        };
    }

    let max_content = measure_intrinsic_axis(tree, node_id, layout, AvailableSpace::MaxContent, axis);
    if value.is_max_content() {
        return IntrinsicAxisValue {
            value: Some(max_content.size.get_abs(axis)),
            depends_on_block_constraints: max_content.depends_on_block_constraints,
        };
    }

    let min_content = measure_intrinsic_axis(tree, node_id, layout, AvailableSpace::MinContent, axis);
    let min_content_size = min_content.size.get_abs(axis);
    let max_content_size = max_content.size.get_abs(axis);

    let limit = if value.is_fit_content_keyword() {
        match available_space {
            AvailableSpace::MinContent => min_content_size,
            AvailableSpace::MaxContent => max_content_size,
            AvailableSpace::Definite(limit) => limit,
        }
    } else {
        let percentage_basis = match (fit_content.percentage_basis, role) {
            (basis @ Some(_), _) => basis,
            // A cyclic percentage in min-size resolves against zero.
            (None, IntrinsicSizeRole::Minimum) => Some(0.0),
            (None, _) => None,
        };
        match value.resolve_fit_content_limit(percentage_basis, |value, basis| tree.calc(value, basis)) {
            Some(limit) => limit + fit_content.box_sizing_adjustment,
            // Cyclic preferred sizes use their initial `auto` value when
            // contributing an intrinsic size.
            None if role == IntrinsicSizeRole::Preferred => match available_space {
                AvailableSpace::MinContent => min_content_size,
                AvailableSpace::MaxContent => max_content_size,
                AvailableSpace::Definite(limit) => limit,
            },
            // A cyclic max-size has an initial value of none, so its
            // fit-content clamp is the max-content contribution.
            None => max_content_size,
        }
    };
    IntrinsicAxisValue {
        value: Some(limit.clamp(min_content_size, max_content_size)),
        depends_on_block_constraints: min_content.depends_on_block_constraints
            || max_content.depends_on_block_constraints,
    }
}

/// Resolve a preferred intrinsic sizing value for a formatting-context-owned
/// axis.
///
/// Flex basis resolution uses the flex container's main-axis percentage basis,
/// which can intentionally differ from the parent-size state supplied to the
/// child's content probe. Keeping that basis explicit lets the shared
/// min/max/fit-content resolver retain CSS's cyclic-percentage behavior without
/// making the child contribution itself definite.
pub(crate) fn resolve_intrinsic_preferred_axis_size(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    value: Dimension,
    available_space: AvailableSpace,
    axis: AbsoluteAxis,
    percentage_basis: Option<f32>,
) -> IntrinsicAxisValue {
    let fit_content =
        fit_content_context(tree, node_id, inputs, axis, percentage_basis, value.is_fit_content_function());
    resolve_intrinsic_axis_value(
        tree,
        node_id,
        value,
        IntrinsicValueInput { layout: inputs, available_space, axis, role: IntrinsicSizeRole::Preferred, fit_content },
    )
}

/// Content-derived components of preferred, minimum and maximum sizes in one
/// axis.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain values
/// resolved from intrinsic keywords, preferred aspect-ratio transfer, or the
/// content-based automatic minimum. Available-size values such as `stretch`
/// are resolved separately by the containing formatting context.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ContentBasedSizeConstraints {
    /// Content-derived component of the preferred size.
    pub preferred: Option<f32>,
    /// Content-derived component of the minimum size.
    pub min: Option<f32>,
    /// Content-derived component of the maximum size.
    pub max: Option<f32>,
    /// Content-based automatic minimum before the authored maximum clamps it.
    automatic_min: Option<f32>,
    /// Whether any measured contribution changes with the containing block's
    /// block-size.
    pub depends_on_block_constraints: bool,
}

impl ContentBasedSizeConstraints {
    /// Merge content-derived components with source-preserving constraints.
    ///
    /// The shared axis resolver applies the aspect-ratio automatic minimum
    /// before transferred constraints, matching CSS Sizing's observable
    /// ordering while keeping formatting-context code source agnostic.
    #[inline(always)]
    pub(crate) fn resolve_against(self, preferred: Option<f32>, constraints: ResolvedAxisConstraints) -> Self {
        let (min, max) = constraints.resolve(self.min, self.max, self.automatic_min);
        Self {
            preferred: preferred.or(self.preferred),
            min,
            max,
            automatic_min: None,
            depends_on_block_constraints: self.depends_on_block_constraints,
        }
    }

    /// Merge content-derived components into already-resolved physical
    /// preferred, minimum, and maximum sizes along `writing_mode`'s block
    /// axis.
    ///
    /// Existing numeric or aspect-ratio-transferred values retain precedence;
    /// the minimum-wins clamp is applied after projecting back to physical
    /// axes. This is shared by block, flex, and grid out-of-flow sizing.
    pub(crate) fn apply_to_block_axis(
        self,
        writing_mode: WritingMode,
        constraints: ResolvedAxisConstraints,
        minimum_border_box_size: Size<f32>,
        size: &mut Size<Option<f32>>,
        min_size: &mut Size<Option<f32>>,
        max_size: &mut Size<Option<f32>>,
    ) {
        let mut logical_size = writing_mode.to_logical(*size);
        let mut logical_min_size = writing_mode.to_logical(*min_size);
        let mut logical_max_size = writing_mode.to_logical(*max_size);
        let resolved = self.resolve_against(logical_size.block_size, constraints);
        logical_size.block_size = resolved.preferred;
        let logical_minimum_border_box_size = writing_mode.to_logical(minimum_border_box_size).block_size;
        logical_min_size.block_size =
            resolved.min.or(Some(logical_minimum_border_box_size)).maybe_max(logical_minimum_border_box_size);
        logical_max_size.block_size = resolved.max;
        logical_size.block_size = logical_size
            .block_size
            .maybe_clamp(logical_min_size.block_size, logical_max_size.block_size)
            .maybe_max(logical_minimum_border_box_size);
        *min_size = writing_mode.to_physical(logical_min_size);
        *max_size = writing_mode.to_physical(logical_max_size);
        *size = writing_mode.to_physical(logical_size);
    }
}

/// Authored preferred, minimum, and maximum sizes projected onto the logical
/// block axis.
///
/// Keeping the triplet together gives block, flex, grid, and out-of-flow
/// layout one ownership boundary for content-derived block sizes, analogous to
/// Blink's `ResolveBlockLengthInternal`/`BlockSizeFunctionRef` pair.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlockSizeProperties {
    /// Authored preferred block size.
    preferred: Dimension,
    /// Authored minimum block size.
    min: Dimension,
    /// Authored maximum block size.
    max: Dimension,
}

impl BlockSizeProperties {
    /// Construct a logical block-axis property triplet.
    #[inline(always)]
    pub(crate) const fn new(preferred: Dimension, min: Dimension, max: Dimension) -> Self {
        Self { preferred, min, max }
    }

    /// Whether any property needs the formatting context's intrinsic block
    /// size.
    #[inline(always)]
    fn uses_intrinsic_size(self) -> bool {
        self.preferred.is_intrinsic() || self.min.is_intrinsic() || self.max.is_intrinsic()
    }

    /// Whether an automatic preferred block size is transferred through the
    /// preferred aspect ratio after the formatting context has established
    /// its final inline size.
    #[inline(always)]
    fn resolves_auto_size_from_ratio(self, has_preferred_aspect_ratio: bool, auto_size_is_content_based: bool) -> bool {
        self.preferred.is_auto() && has_preferred_aspect_ratio && auto_size_is_content_based
    }

    #[inline(always)]
    /// Resolve authored content-based constraints and their automatic minimum.
    fn resolve_content_based_constraints(
        self,
        intrinsic_border_box_size: f32,
        ratio_block_size: Option<f32>,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
    ) -> ContentBasedSizeConstraints {
        let content_block_size = ratio_block_size.unwrap_or(intrinsic_border_box_size);
        let resolve_explicit = |value: Dimension| value.is_intrinsic().then_some(content_block_size);
        let resolves_auto_size_from_ratio =
            self.resolves_auto_size_from_ratio(ratio_block_size.is_some(), auto_size_is_content_based);
        let automatic_minimum = self
            .applies_automatic_minimum(ratio_block_size.is_some(), auto_size_is_content_based, is_scroll_container)
            .then_some(intrinsic_border_box_size);
        ContentBasedSizeConstraints {
            preferred: resolve_explicit(self.preferred)
                .or_else(|| resolves_auto_size_from_ratio.then_some(ratio_block_size).flatten()),
            min: resolve_explicit(self.min),
            max: resolve_explicit(self.max),
            automatic_min: automatic_minimum,
            depends_on_block_constraints: false,
        }
    }

    #[inline(always)]
    /// Whether CSS Sizing's ratio-dependent automatic minimum applies.
    fn applies_automatic_minimum(
        self,
        has_preferred_aspect_ratio: bool,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
    ) -> bool {
        has_preferred_aspect_ratio
            && !is_scroll_container
            && self.min.is_auto()
            && (self.preferred.is_intrinsic() || (self.preferred.is_auto() && auto_size_is_content_based))
    }
}

/// State needed to resolve a formatting context's content-based block size.
///
/// Formatting algorithms produce the real intrinsic border-box size; this
/// value owns the shared CSS sizing rules that combine it with a preferred
/// aspect ratio and the automatic minimum. Keeping that state together avoids
/// block, flex, grid, and out-of-flow layout growing separate resolution
/// orders.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentBasedBlockSize {
    /// Authored logical block-axis sizing properties.
    properties: BlockSizeProperties,
    /// Used preferred aspect ratio and its sizing box.
    aspect_ratio: ResolvedAspectRatio,
    /// Physical padding-and-border sums for ratio box conversion.
    padding_border: Size<f32>,
    /// Whether this constraint space resolves an authored auto block size from content.
    auto_size_is_content_based: bool,
    /// Whether overflow establishes a scroll container that disables the automatic minimum.
    is_scroll_container: bool,
    /// Contained intrinsic border-box size selected by the formatting-context boundary.
    intrinsic_border_box_override: Option<f32>,
    /// Source-preserving constraints for the logical block axis.
    resolved_constraints: ResolvedAxisConstraints,
}

impl ContentBasedBlockSize {
    /// Construct the resolver at the formatting-context boundary.
    #[inline(always)]
    pub(crate) const fn new(
        properties: BlockSizeProperties,
        aspect_ratio: ResolvedAspectRatio,
        padding_border: Size<f32>,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
        intrinsic_border_box_override: Option<f32>,
    ) -> Self {
        Self {
            properties,
            aspect_ratio,
            padding_border,
            auto_size_is_content_based,
            is_scroll_container,
            intrinsic_border_box_override,
            resolved_constraints: ResolvedAxisConstraints::NONE,
        }
    }

    /// Attach the initially resolved block-axis constraint sources.
    #[inline(always)]
    pub(crate) const fn with_resolved_constraints(mut self, resolved_constraints: ResolvedAxisConstraints) -> Self {
        self.resolved_constraints = resolved_constraints;
        self
    }

    /// Return the constraints consumed after intrinsic content measurement.
    #[inline(always)]
    pub(crate) const fn resolved_constraints(self) -> ResolvedAxisConstraints {
        self.resolved_constraints
    }

    /// Replace the formatting context's intrinsic block-size contribution.
    ///
    /// Grid discovers its no-children track size after constructing its track
    /// collection, whereas ordinary formatting contexts know their zero-based
    /// override at construction time.
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
        let has_preferred_aspect_ratio = self.aspect_ratio.ratio.is_some();
        self.properties.uses_intrinsic_size()
            || self
                .properties
                .resolves_auto_size_from_ratio(has_preferred_aspect_ratio, self.auto_size_is_content_based)
            || self.properties.applies_automatic_minimum(
                has_preferred_aspect_ratio,
                self.auto_size_is_content_based,
                self.is_scroll_container,
            )
    }

    /// Whether this pass needs the formatting context's real intrinsic block
    /// size.
    ///
    /// A preferred aspect ratio can synthesize the content-based preferred
    /// size, but the automatic minimum still uses the real intrinsic size.
    /// This mirrors Blink's separate `SizeType::kContent` and
    /// `SizeType::kIntrinsic` callbacks in `ComputeBlockSizeForFragment`.
    #[inline(always)]
    pub(crate) fn requires_intrinsic_measurement(self) -> bool {
        let has_preferred_aspect_ratio = self.aspect_ratio.ratio.is_some();
        self.intrinsic_border_box_override.is_none()
            && (self.properties.uses_intrinsic_size()
                || self.properties.applies_automatic_minimum(
                    has_preferred_aspect_ratio,
                    self.auto_size_is_content_based,
                    self.is_scroll_container,
                ))
    }

    /// Resolve preferred/minimum/maximum content-based block constraints from
    /// the formatting context's real intrinsic border-box size.
    #[inline(always)]
    pub(crate) fn resolve(
        self,
        writing_mode: WritingMode,
        outer_inline_size: Option<f32>,
        intrinsic_border_box_size: f32,
    ) -> ContentBasedSizeConstraints {
        let ratio_block_size =
            resolve_aspect_ratio_block_size(writing_mode, outer_inline_size, self.aspect_ratio, self.padding_border);
        self.properties.resolve_content_based_constraints(
            self.intrinsic_border_box_override.unwrap_or(intrinsic_border_box_size),
            ratio_block_size,
            self.auto_size_is_content_based,
            self.is_scroll_container,
        )
    }
}

/// Resolve the physical preferred aspect ratio from a known logical inline
/// border-box size back onto the logical block axis.
///
/// `ResolvedAspectRatio` owns its sizing box, so content-box ratios account
/// for padding and border before returning a border-box block size.
#[inline(always)]
fn resolve_aspect_ratio_block_size(
    writing_mode: WritingMode,
    outer_inline_size: Option<f32>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Option<f32> {
    let physical_size = writing_mode.to_physical(LogicalSize { inline_size: outer_inline_size, block_size: None });
    let ratio_size =
        physical_size.maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border);
    writing_mode.to_logical(ratio_size).block_size
}

/// Resolve content-based block-size properties after the caller has
/// established the box's inline-size constraint.
///
/// This is primarily used by out-of-flow sizing algorithms, which must decide
/// whether opposing insets stretch an automatic block size. Passing the
/// resolved inline size into a content-size probe preserves wrapping when an
/// intrinsic keyword or automatic minimum needs real content. Ratio-only
/// resolution bypasses that measurement.
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
    let outer_inline_size = known_logical_size.inline_size;
    if !resolver.requires_intrinsic_measurement() {
        return resolver.resolve(writing_mode, outer_inline_size, 0.0);
    }
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
) -> ContentBasedSizeConstraints {
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
) -> ContentBasedSizeConstraints {
    let IntrinsicAxisInput { preferred, min, max, available_space, axis } = axis_input;
    let has_fit_content_function =
        preferred.is_fit_content_function() || min.is_fit_content_function() || max.is_fit_content_function();
    let fit_content =
        fit_content_context(tree, node_id, inputs, axis, inputs.parent_size.get_abs(axis), has_fit_content_function);
    let preferred = resolve_intrinsic_axis_value(
        tree,
        node_id,
        preferred,
        IntrinsicValueInput { layout: inputs, available_space, axis, role: IntrinsicSizeRole::Preferred, fit_content },
    );
    let min = resolve_intrinsic_axis_value(
        tree,
        node_id,
        min,
        IntrinsicValueInput { layout: inputs, available_space, axis, role: IntrinsicSizeRole::Minimum, fit_content },
    );
    let max = resolve_intrinsic_axis_value(
        tree,
        node_id,
        max,
        IntrinsicValueInput { layout: inputs, available_space, axis, role: IntrinsicSizeRole::Maximum, fit_content },
    );
    ContentBasedSizeConstraints {
        preferred: preferred.value,
        min: min.value,
        max: max.value,
        automatic_min: None,
        depends_on_block_constraints: preferred.depends_on_block_constraints
            || min.depends_on_block_constraints
            || max.depends_on_block_constraints,
    }
}

/// Inputs needed to resolve the node's initial preferred and limiting sizes.
///
/// Formatting algorithms already own decoration and containment resolution,
/// so this context keeps the shared sizing operation independent of any one
/// display mode.
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
    pub aspect_ratio: ResolvedAspectRatio,
    /// Formatting-context-selected size-containment substitute, including
    /// decoration.
    pub contained_outer_size: Size<Option<f32>>,
}

impl NodeSizeConstraintInput {
    /// Retain only authored constraints outside the axes whose content
    /// contribution is being measured.
    fn for_content_contribution(mut self, requested_axis: RequestedAxis) -> Self {
        if requested_axis.contains(AbsoluteAxis::Horizontal) {
            self.raw_size.width = Dimension::auto();
            self.raw_min_size.width = Dimension::auto();
            self.raw_max_size.width = Dimension::auto();
        }
        if requested_axis.contains(AbsoluteAxis::Vertical) {
            self.raw_size.height = Dimension::auto();
            self.raw_min_size.height = Dimension::auto();
            self.raw_max_size.height = Dimension::auto();
        }
        self.aspect_ratio = self.aspect_ratio.disabled();
        self
    }
}

/// Child-owned initial geometry derived from style and a constraint space.
///
/// This is deliberately separate from [`LayoutInput::known_dimensions`]. A
/// known dimension is a fixed size owned by the parent formatting context;
/// these values are the node's own preferred/minimum/maximum sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedNodeSizing {
    /// Preferred physical border-box size after intrinsic keywords, stretch,
    /// containment and preferred-ratio transfer.
    pub preferred_size: Size<Option<f32>>,
    /// Used physical minimum constraints.
    pub min_size: Size<Option<f32>>,
    /// Used physical maximum constraints.
    pub max_size: Size<Option<f32>>,
    /// Initial physical border-box geometry after parent-fixed dimensions and
    /// child-owned sizing have been combined.
    pub outer_size: Size<Option<f32>>,
    /// Used physical border-box dimensions that are definite for descendants.
    ///
    /// Unlike `outer_size`, this excludes content-derived and intrinsic-keyword
    /// sizes even after they have been reduced to numeric used values.
    pub definite_size: Size<Option<f32>>,
    /// Whether resolving the logical inline axis measured content dependent
    /// on the containing block's block constraint.
    pub depends_on_block_constraints: bool,
    /// Whether the logical inline size was synthesized through the preferred
    /// aspect ratio.
    pub applied_aspect_ratio: bool,
    /// Source-preserving constraints used by block-size resolution.
    pub(crate) constraints: ResolvedSizeConstraints,
}

/// Direct authored-size resolution and the subset whose source is definite.
///
/// Intrinsic keywords and size-containment substitutes can produce numeric
/// preferred sizes later in the pipeline, but they must not become percentage
/// bases merely because their used value has been measured.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DirectNodeSizeResolution {
    /// Preferred and limiting used-value constraints after containment.
    constraints: ResolvedSizeConstraints,
    /// Preferred axes resolved without content measurement or containment.
    definite_preferred_size: Size<Option<f32>>,
}

/// Resolve authored numeric, available-space and containment constraints
/// before intrinsic keywords are merged by the caller.
fn resolve_direct_node_size_constraints(
    tree: &impl LayoutPartialTree,
    inputs: LayoutInput,
    writing_mode: WritingMode,
    sizing: NodeSizeConstraintInput,
    transferred_sizes_mode: TransferredSizesMode,
) -> DirectNodeSizeResolution {
    let NodeSizeConstraintInput {
        raw_size,
        raw_min_size,
        raw_max_size,
        box_sizing_adjustment,
        padding_border_size,
        aspect_ratio,
        contained_outer_size,
    } = sizing;
    let stretch = resolve_stretch_size_constraints(
        raw_size,
        raw_min_size,
        raw_max_size,
        inputs.available_space.into_options(),
        padding_border_size,
    );
    let direct_preferred_size = raw_size
        .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
        .maybe_add(box_sizing_adjustment)
        .or(stretch.preferred);
    let resolved = resolve_size_constraints(SizeConstraintInput {
        size: direct_preferred_size,
        min_size: raw_min_size
            .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
            .maybe_add(box_sizing_adjustment)
            .or(stretch.min),
        max_size: raw_max_size
            .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
            .maybe_add(box_sizing_adjustment)
            .or(stretch.max),
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode,
        inline_auto_behavior: inputs.inline_auto_behavior,
        block_auto_behavior: inputs.block_auto_behavior,
        transferred_sizes_mode,
        aspect_ratio,
        padding_border: padding_border_size,
    });
    let definite_preferred_size = resolved.size;
    let constraints = apply_contained_intrinsic_size_constraints(
        resolved,
        raw_size,
        raw_min_size,
        raw_max_size,
        contained_outer_size,
    );
    DirectNodeSizeResolution { constraints, definite_preferred_size }
}

/// Keep the final used value only on axes with a definite sizing source.
#[inline(always)]
fn used_definite_size(
    used_size: Size<Option<f32>>,
    known_size: Size<Option<f32>>,
    parent_definite_size: Size<Option<f32>>,
    own_definite_size: Size<Option<f32>>,
) -> Size<Option<f32>> {
    // A parent-fixed used axis replaces the child's authored preferred size.
    // Its definiteness therefore belongs exclusively to the parent formatting
    // context; an authored length cannot make the overridden target definite.
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

/// Resolve a node's initial sizing geometry without changing its constraint
/// space.
///
/// This is Taffy's counterpart to Blink's initial fragment geometry sizing:
/// parent-fixed dimensions remain in `known_dimensions`, while authored and
/// intrinsic sizes are returned as child-owned data.
pub(crate) fn resolve_node_size_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    inputs: LayoutInput,
    sizing: NodeSizeConstraintInput,
) -> ResolvedNodeSizing {
    let writing_mode = tree.get_writing_mode(node_id);
    if inputs.sizing_mode == SizingMode::ContentSize {
        // A content contribution ignores authored sizing in the requested
        // axis, but the perpendicular axis still belongs to the box's
        // constraint space. This is observable for formatting contexts whose
        // intrinsic contribution depends on that axis, such as a wrapped
        // column flex container whose number of columns is set by its height.
        //
        // Resolve only direct length/percentage/available-space constraints
        // here. Intrinsic values in the requested axis would recurse back into
        // the contribution being measured, while preferred-ratio transfer is
        // owned by the formatting algorithm performing that measurement.
        let contribution_sizing = sizing.for_content_contribution(inputs.axis);
        let direct = resolve_direct_node_size_constraints(
            tree,
            inputs,
            writing_mode,
            contribution_sizing,
            TransferredSizesMode::Ignore,
        );
        let resolved = direct.constraints;
        let preferred_size = resolved
            .used_preferred_size()
            .or(sizing.contained_outer_size.maybe_clamp(resolved.min_size, resolved.max_size));
        let outer_size = resolve_used_size(
            inputs.known_dimensions,
            preferred_size,
            resolved.min_size,
            resolved.max_size,
            sizing.padding_border_size,
        );
        let definite_size = used_definite_size(
            outer_size,
            inputs.known_dimensions,
            inputs.definite_dimensions,
            direct.definite_preferred_size,
        );
        return ResolvedNodeSizing {
            preferred_size,
            min_size: resolved.min_size,
            max_size: resolved.max_size,
            outer_size,
            definite_size,
            depends_on_block_constraints: false,
            applied_aspect_ratio: false,
            constraints: resolved,
        };
    }
    let NodeSizeConstraintInput {
        raw_size,
        raw_min_size,
        raw_max_size,
        padding_border_size,
        aspect_ratio,
        contained_outer_size,
        ..
    } = sizing;
    let inline_axis = writing_mode.inline_axis();
    // `available_space` is the border-box space offered by the containing
    // formatting context. Parent algorithms have already applied their own
    // margin, alignment, track, line and inset rules at that boundary.
    let available_inline_size = inputs.available_space.get_abs(inline_axis);
    let logical_raw_size = writing_mode.to_logical(raw_size);
    let logical_raw_min_size = writing_mode.to_logical(raw_min_size);
    let logical_raw_max_size = writing_mode.to_logical(raw_max_size);
    let intrinsic = resolve_intrinsic_axis_constraints(
        tree,
        node_id,
        inputs,
        IntrinsicAxisInput {
            preferred: logical_raw_size.inline_size,
            min: logical_raw_min_size.inline_size,
            max: logical_raw_max_size.inline_size,
            available_space: available_inline_size,
            axis: inline_axis,
        },
    );

    let direct = resolve_direct_node_size_constraints(tree, inputs, writing_mode, sizing, TransferredSizesMode::Normal);
    let mut resolved = direct.constraints;
    let mut logical_resolved_size = writing_mode.to_logical(resolved.size);
    logical_resolved_size.inline_size = logical_resolved_size.inline_size.or(intrinsic.preferred);
    resolved.size = writing_mode.to_physical(logical_resolved_size);
    resolved.apply_late_authored_constraints(
        writing_mode.to_physical(LogicalSize { inline_size: intrinsic.min, block_size: None }),
        writing_mode.to_physical(LogicalSize { inline_size: intrinsic.max, block_size: None }),
    );

    let inline_auto_size = resolve_inline_auto_size_preference(InlineAutoSizeInput {
        preferred_size: resolved.size,
        fixed_size: inputs.known_dimensions,
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode,
        inline_behavior: inputs.inline_auto_behavior,
        block_behavior: inputs.block_auto_behavior,
        available_space: inputs.available_space,
        min_size: resolved.min_size,
        max_size: resolved.max_size,
        minimum_border_box_size: padding_border_size,
        aspect_ratio,
    });
    resolved.size = inline_auto_size.size;
    resolved.aspect_ratio_applied.width |= inline_auto_size.aspect_ratio_applied.width;
    resolved.aspect_ratio_applied.height |= inline_auto_size.aspect_ratio_applied.height;

    let automatic_minimum =
        measure_aspect_ratio_automatic_minimum(tree, node_id, inputs, inline_axis, padding_border_size, resolved);
    resolved.apply_automatic_minimum(inline_axis, automatic_minimum.value);
    let own_definite_size = resolve_inline_auto_size_preference(InlineAutoSizeInput {
        preferred_size: direct.definite_preferred_size,
        fixed_size: inputs.definite_dimensions,
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode,
        inline_behavior: inputs.inline_auto_behavior,
        block_behavior: inputs.block_auto_behavior,
        available_space: inputs.available_space,
        min_size: resolved.min_size,
        max_size: resolved.max_size,
        minimum_border_box_size: padding_border_size,
        aspect_ratio,
    })
    .size;
    let preferred_size =
        resolved.used_preferred_size().or(contained_outer_size.maybe_clamp(resolved.min_size, resolved.max_size));
    let min_max_definite_size = resolved.min_size.zip_map(resolved.max_size, |min, max| match (min, max) {
        (Some(min), Some(max)) if max <= min => Some(min),
        _ => None,
    });
    let size_before_fixed_ratio = resolve_used_size(
        inputs.known_dimensions,
        min_max_definite_size.or(preferred_size),
        resolved.min_size,
        resolved.max_size,
        padding_border_size,
    );
    let size_after_fixed_ratio = apply_preferred_aspect_ratio(
        size_before_fixed_ratio,
        raw_size.map(|dimension| dimension.is_auto()),
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
    let definite_size =
        used_definite_size(outer_size, inputs.known_dimensions, inputs.definite_dimensions, own_definite_size);
    let applied_aspect_ratio = inputs.known_dimensions.get_abs(inline_axis).is_none()
        && min_max_definite_size.get_abs(inline_axis).is_none()
        && (resolved.aspect_ratio_applied.get_abs(inline_axis)
            || (size_before_fixed_ratio.get_abs(inline_axis).is_none()
                && size_after_fixed_ratio.get_abs(inline_axis).is_some()));

    ResolvedNodeSizing {
        preferred_size,
        min_size: resolved.min_size,
        max_size: resolved.max_size,
        outer_size,
        definite_size,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints
            || automatic_minimum.depends_on_block_constraints,
        applied_aspect_ratio,
        constraints: resolved,
    }
}

/// Resolve leaf sizing directly from its style projection.
///
/// Custom tree adapters use this at their cache-miss dispatch boundary before
/// invoking the low-level leaf algorithm.
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
        let style = tree.get_core_container_style(node_id);
        let padding = style.padding().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let padding_border_size = (padding + border).sum_axes();
        let scrollbar_gutter = style.overflow().transpose().map(|overflow| match overflow {
            Overflow::Scroll => style.scrollbar_width(),
            _ => 0.0,
        });
        let content_box_inset_size =
            padding_border_size + Size { width: scrollbar_gutter.x, height: scrollbar_gutter.y };
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
            contained_outer_size: size_containment.resolve_outer_size(Size::ZERO, content_box_inset_size),
        }
    };
    resolve_node_size_constraints(tree, node_id, inputs, sizing)
}
