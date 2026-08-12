//! Resolution of intrinsic inline-size keywords.
//!
//! `Dimension::min_content()`, `max_content()`, and `fit_content()` cannot be
//! reduced by the ordinary length/percentage resolver: their used value comes
//! from content-size layout of the same box. Keep that recursion at the tree
//! seam so every formatting context uses the same pass-local cache and no
//! retained intrinsic-size state is required.

use crate::geometry::{AbsoluteAxis, LogicalSize, Size, WritingMode};
use crate::style::{AvailableSpace, CoreStyle, Dimension};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, RequestedAxis,
    SizingMode,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, ResolvedAspectRatio};

use super::aspect_ratio::{
    resolve_size_constraints, ResolvedAxisConstraints, ResolvedSizeConstraints, SizeConstraintInput,
    TransferredSizesMode,
};
use super::stretch::resolve_stretch_size_constraints;

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
    resolved.size = resolved.size.maybe_clamp(resolved.min_size, resolved.max_size);
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
struct IntrinsicAxisValue {
    /// Resolved border-box size, or `None` when the value is not intrinsic.
    value: Option<f32>,
    /// Whether measuring the value observed a block-constraint dependency.
    depends_on_block_constraints: bool,
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

/// Intrinsic components of preferred, minimum and maximum sizes in one axis.
///
/// Numeric and percentage components are resolved by the formatting-context
/// algorithm that owns their containing block. These fields contain only the
/// values that required intrinsic content measurement. Available-size values
/// such as `stretch` are resolved separately by the containing formatting
/// context.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IntrinsicSizeConstraints {
    /// Intrinsic component of the preferred size.
    pub preferred: Option<f32>,
    /// Intrinsic component of the minimum size.
    pub min: Option<f32>,
    /// Intrinsic component of the maximum size.
    pub max: Option<f32>,
    /// Content-based automatic minimum before the authored maximum clamps it.
    automatic_min: Option<f32>,
    /// Whether any measured contribution changes with the containing block's
    /// block-size.
    pub depends_on_block_constraints: bool,
}

impl IntrinsicSizeConstraints {
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

    /// Merge intrinsic components into already-resolved physical preferred,
    /// minimum, and maximum sizes along `writing_mode`'s block axis.
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

    #[inline(always)]
    /// Resolve authored content-based constraints and their automatic minimum.
    fn resolve_content_based_constraints(
        self,
        intrinsic_border_box_size: f32,
        ratio_block_size: Option<f32>,
        auto_size_is_content_based: bool,
        is_scroll_container: bool,
    ) -> IntrinsicSizeConstraints {
        let content_block_size = ratio_block_size.unwrap_or(intrinsic_border_box_size);
        let resolve_explicit = |value: Dimension| value.is_intrinsic().then_some(content_block_size);
        let automatic_minimum = self
            .applies_automatic_minimum(ratio_block_size.is_some(), auto_size_is_content_based, is_scroll_container)
            .then_some(intrinsic_border_box_size);
        IntrinsicSizeConstraints {
            preferred: resolve_explicit(self.preferred).or_else(|| {
                (self.preferred.is_auto() && auto_size_is_content_based).then_some(ratio_block_size).flatten()
            }),
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
        self.properties.uses_intrinsic_size()
            || self.properties.applies_automatic_minimum(
                self.aspect_ratio.ratio.is_some(),
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
        self.requires_resolution() && self.intrinsic_border_box_override.is_none()
    }

    /// Resolve preferred/minimum/maximum content-based block constraints from
    /// the formatting context's real intrinsic border-box size.
    #[inline(always)]
    pub(crate) fn resolve(
        self,
        writing_mode: WritingMode,
        outer_inline_size: Option<f32>,
        intrinsic_border_box_size: f32,
    ) -> IntrinsicSizeConstraints {
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

/// Measure and resolve intrinsic block-size properties after the caller has
/// established the box's inline-size constraint.
///
/// This is primarily used by out-of-flow sizing algorithms, which must decide
/// whether opposing insets stretch an automatic block size. Passing the
/// resolved inline size into a content-size probe preserves wrapping while
/// deliberately removing any preliminary block-size stretch.
pub(crate) fn measure_intrinsic_block_size_constraints(
    tree: &mut impl LayoutPartialTree,
    node_id: crate::NodeId,
    mut child_input: ChildLayoutInput,
    resolver: ContentBasedBlockSize,
) -> IntrinsicSizeConstraints {
    if !resolver.requires_resolution() {
        return IntrinsicSizeConstraints::default();
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
    let has_fit_content_function =
        preferred.is_fit_content_function() || min.is_fit_content_function() || max.is_fit_content_function();
    let fit_content = if has_fit_content_function {
        let percentage_basis =
            inputs.constraint_space(tree.get_writing_mode(node_id)).margin_padding_percentage_basis();
        let (padding, border, box_sizing) = {
            let style = tree.get_core_container_style(node_id);
            (style.padding(), style.border(), style.box_sizing())
        };
        let padding = padding.resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let border = border.resolve_or_zero(percentage_basis, |value, basis| tree.calc(value, basis));
        let box_sizing_adjustment =
            if box_sizing == BoxSizing::ContentBox { (padding + border).sum_axes().get_abs(axis) } else { 0.0 };
        FitContentContext { percentage_basis: inputs.parent_size.get_abs(axis), box_sizing_adjustment }
    } else {
        FitContentContext::default()
    };
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
    IntrinsicSizeConstraints {
        preferred: preferred.value,
        min: min.value,
        max: max.value,
        automatic_min: None,
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

    let writing_mode = tree.get_writing_mode(node_id);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let (
        raw_size,
        raw_min_size,
        raw_max_size,
        margin,
        padding_border_size,
        box_sizing_adjustment,
        aspect_ratio,
        contained_outer_size,
    ) = {
        let aspect_ratio = tree.get_resolved_aspect_ratio(node_id);
        let size_containment = tree.get_size_containment(node_id);
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
        let contained_outer_size = size_containment.resolve_explicit_outer_size(padding_border_size);
        (
            raw_size,
            raw_min_size,
            raw_max_size,
            margin,
            padding_border_size,
            box_sizing_adjustment,
            aspect_ratio,
            contained_outer_size,
        )
    };
    let available_width = inputs.available_space.width.maybe_sub(margin.horizontal_axis_sum());
    let stretch = resolve_stretch_size_constraints(
        raw_size,
        raw_min_size,
        raw_max_size,
        Size { width: available_width.into_option(), height: None },
        padding_border_size,
    );

    let intrinsic = resolve_intrinsic_width_constraints(
        tree,
        node_id,
        inputs,
        raw_size.width,
        raw_min_size.width,
        raw_max_size.width,
        available_width,
    );

    let mut resolved = apply_contained_intrinsic_size_constraints(
        resolve_size_constraints(SizeConstraintInput {
            size: raw_size
                .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_add(box_sizing_adjustment),
            min_size: raw_min_size
                .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_add(box_sizing_adjustment),
            max_size: raw_max_size
                .maybe_resolve(inputs.parent_size, |value, basis| tree.calc(value, basis))
                .maybe_add(box_sizing_adjustment),
            size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
            writing_mode,
            block_auto_behavior: inputs.block_auto_behavior,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio,
            padding_border: padding_border_size,
        }),
        raw_size,
        raw_min_size,
        raw_max_size,
        contained_outer_size,
    );
    resolved.size.width = resolved.size.width.or(stretch.preferred.width).or(intrinsic.preferred);
    resolved.apply_late_authored_constraints(
        Size { width: stretch.min.width.or(intrinsic.min), height: None },
        Size { width: stretch.max.width.or(intrinsic.max), height: None },
    );

    let automatic_minimum = measure_aspect_ratio_automatic_minimum(
        tree,
        node_id,
        inputs,
        AbsoluteAxis::Horizontal,
        padding_border_size,
        resolved,
    );
    resolved.apply_automatic_minimum(AbsoluteAxis::Horizontal, automatic_minimum.value);
    let preferred_width = resolved.size.width.maybe_clamp(resolved.min_size.width, resolved.max_size.width);
    let applied_aspect_ratio = inputs.known_dimensions.width.is_none() && resolved.aspect_ratio_applied.width;

    inputs.known_dimensions.width = inputs.known_dimensions.width.or(preferred_width);
    ResolvedIntrinsicWidthInputs {
        inputs,
        depends_on_block_constraints: intrinsic.depends_on_block_constraints
            || automatic_minimum.depends_on_block_constraints,
        applied_aspect_ratio,
    }
}
