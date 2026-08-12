use crate::geometry::{
    AbsoluteAxis, AbstractAxis, InBothAbstractAxis, Line, LogicalBoxStrut, LogicalOffset, LogicalSize,
    LogicalStaticPosition, Point, Rect, Size, StaticPositionEdge, WritingDirection,
};
use crate::style::{
    AlignItemsKeyword, AlignSelf, AlignmentSafety, AvailableSpace, BoxGenerationMode, BoxSizing, CoreStyle, Overflow,
    Position,
};
use crate::tree::{
    AutoSizeBehavior, ChildLayoutInput, Layout, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId,
    OutOfFlowContainingBlock, RequestedAxis, RunMode, SizingMode, SizingPurpose,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};

use super::aspect_ratio::apply_preferred_aspect_ratio;
#[cfg(feature = "content_size")]
use super::content_size::compute_content_size_contribution;
use super::intrinsic_size::{
    resolve_content_based_block_size_constraints, resolve_node_size_constraints, BlockSizeProperties,
    ContentBasedBlockSize, NodeSizeConstraintInput,
};

/// One out-of-flow candidate after its original formatting context has chosen
/// the size-independent static-position anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OutOfFlowItem {
    /// Positioned node to lay out.
    pub node: NodeId,
    /// Source-order paint index within the numeric containing block.
    pub order: u32,
    /// Candidate expressed in the containing numeric container's border-box
    /// logical coordinate space.
    pub static_position: LogicalStaticPosition,
}

/// Result retained by a containing block after laying out one out-of-flow
/// descendant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct OutOfFlowLayoutOutput {
    /// Scrollable-overflow contribution relative to the containing area.
    pub content_size: Size<f32>,
}

/// Which edge of an inset-modified containing block remains fixed while its
/// weaker edge absorbs free space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsetBias {
    /// Preserve the logical start edge and move the end edge.
    Start,
    /// Preserve the logical end edge and move the start edge.
    End,
    /// Move both logical edges by equal amounts.
    Equal,
}

impl InsetBias {
    /// Return the bias for the opposite logical edge.
    #[inline(always)]
    const fn opposite(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
            Self::Equal => Self::Equal,
        }
    }

    /// Convert static-position edge metadata into its equivalent inset bias.
    #[inline(always)]
    const fn from_static_edge(edge: StaticPositionEdge) -> Self {
        match edge {
            StaticPositionEdge::Start => Self::Start,
            StaticPositionEdge::Center => Self::Equal,
            StaticPositionEdge::End => Self::End,
        }
    }
}

/// Self-alignment projected into the positioned box's logical axes.
///
/// The values themselves remain CSS container-relative values. The paired
/// start bias records which side of each child axis corresponds to the actual
/// containing block's logical start side. `physical_low` records which child
/// edge is physically left/top, for the physical `left`/`right` keywords.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalAlignment {
    /// Container align/justify values projected onto the child's logical axes.
    values: InBothAbstractAxis<Option<AlignSelf>>,
    /// Child edge corresponding to the containing block's start edge per axis.
    container_start: InBothAbstractAxis<InsetBias>,
    /// Child edge corresponding to the physical low (left/top) edge per axis.
    physical_low: InBothAbstractAxis<InsetBias>,
}

impl LogicalAlignment {
    /// Project container-relative self-alignment into the positioned child's
    /// logical coordinate space.
    #[inline]
    fn new(
        align_self: Option<AlignSelf>,
        justify_self: Option<AlignSelf>,
        container_writing_direction: WritingDirection,
        self_writing_direction: WritingDirection,
    ) -> Self {
        let is_parallel = !self_writing_direction.mode.is_orthogonal_to(container_writing_direction.mode);
        let values = if is_parallel {
            InBothAbstractAxis { inline: justify_self, block: align_self }
        } else {
            InBothAbstractAxis { inline: align_self, block: justify_self }
        };
        let start_bias = |axis: AbstractAxis| {
            let physical_axis = axis.to_absolute(self_writing_direction.mode);
            let starts_match =
                self_writing_direction.mode.is_axis_flow_reversed(physical_axis, self_writing_direction.direction)
                    == container_writing_direction
                        .mode
                        .is_axis_flow_reversed(physical_axis, container_writing_direction.direction);
            if starts_match {
                InsetBias::Start
            } else {
                InsetBias::End
            }
        };
        let physical_low_bias = |axis: AbstractAxis| {
            let physical_axis = axis.to_absolute(self_writing_direction.mode);
            if self_writing_direction.mode.is_axis_flow_reversed(physical_axis, self_writing_direction.direction) {
                InsetBias::End
            } else {
                InsetBias::Start
            }
        };
        Self {
            values,
            container_start: InBothAbstractAxis {
                inline: start_bias(AbstractAxis::Inline),
                block: start_bias(AbstractAxis::Block),
            },
            physical_low: InBothAbstractAxis {
                inline: physical_low_bias(AbstractAxis::Inline),
                block: physical_low_bias(AbstractAxis::Block),
            },
        }
    }

    /// Resolve the requested position to the child edge that remains fixed.
    #[inline(always)]
    fn inset_bias(self, axis: AbstractAxis) -> InsetBias {
        let alignment = self.values.get(axis);
        let container_start = self.container_start.get(axis);
        match alignment.map(AlignSelf::keyword) {
            None
            | Some(AlignItemsKeyword::Normal)
            | Some(AlignItemsKeyword::Start)
            | Some(AlignItemsKeyword::FlexStart)
            | Some(AlignItemsKeyword::Baseline)
            | Some(AlignItemsKeyword::Stretch) => container_start,
            Some(AlignItemsKeyword::End) | Some(AlignItemsKeyword::FlexEnd) | Some(AlignItemsKeyword::LastBaseline) => {
                container_start.opposite()
            }
            Some(AlignItemsKeyword::SelfStart) => InsetBias::Start,
            Some(AlignItemsKeyword::SelfEnd) => InsetBias::End,
            Some(AlignItemsKeyword::Left) => self.physical_low.get(axis),
            Some(AlignItemsKeyword::Right) => self.physical_low.get(axis).opposite(),
            Some(AlignItemsKeyword::Center) => InsetBias::Equal,
        }
    }

    /// Return the containing-block start bias for authored safe alignment.
    #[inline(always)]
    fn safe_bias(self, axis: AbstractAxis) -> Option<InsetBias> {
        self.values
            .get(axis)
            .filter(|alignment| alignment.safety == AlignmentSafety::Safe)
            .map(|_| self.container_start.get(axis))
    }

    /// Return the containing-block start bias for omitted overflow alignment.
    #[inline(always)]
    fn default_bias(self, axis: AbstractAxis) -> Option<InsetBias> {
        self.values
            .get(axis)
            .filter(|alignment| alignment.safety == AlignmentSafety::Default)
            .map(|_| self.container_start.get(axis))
    }

    /// Whether this axis has non-normal alignment with the default overflow
    /// modifier.
    #[inline(always)]
    fn has_default_overflow(self, axis: AbstractAxis) -> bool {
        self.values
            .get(axis)
            .is_some_and(|alignment| alignment.safety == AlignmentSafety::Default && !alignment.is_normal())
    }

    /// Whether this axis explicitly requests stretch alignment.
    #[inline(always)]
    fn is_stretch(self, axis: AbstractAxis) -> bool {
        self.values.get(axis).is_some_and(|alignment| alignment.keyword() == AlignItemsKeyword::Stretch)
    }

    /// Whether this axis uses the initial `normal` alignment behavior.
    #[inline(always)]
    fn is_normal(self, axis: AbstractAxis) -> bool {
        match self.values.get(axis) {
            None => true,
            Some(alignment) => alignment.is_normal(),
        }
    }
}

/// One logical axis of an inset-modified containing block, including the
/// metadata needed for sizing, margin resolution, and final alignment.
#[derive(Clone, Copy, Debug, PartialEq)]
struct InsetModifiedAxis {
    /// Distance from the available area's logical start edge.
    start: f32,
    /// Distance from the available area's logical end edge.
    end: f32,
    /// Whether either authored inset was auto.
    has_auto_inset: bool,
    /// Edge retained while ordinary free space is consumed.
    inset_bias: InsetBias,
    /// Safe-overflow fallback edge, when authored.
    safe_inset_bias: Option<InsetBias>,
    /// Edge prioritized by the default-overflow adjustment.
    default_inset_bias: Option<InsetBias>,
    /// Whether default-overflow containment applies after alignment.
    has_default_alignment_overflow: bool,
}

impl InsetModifiedAxis {
    /// Return the non-negative space between the two inset edges.
    #[inline(always)]
    fn size(self, available_size: f32) -> f32 {
        available_size - self.start - self.end
    }
}

/// Inset-modified containing block expressed in the positioned child's
/// logical coordinate space.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalInsetModifiedContainingBlock {
    /// Child logical inline-axis constraints.
    inline: InsetModifiedAxis,
    /// Child logical block-axis constraints.
    block: InsetModifiedAxis,
}

impl LogicalInsetModifiedContainingBlock {
    /// Return the inline/block size between the inset edges.
    #[inline(always)]
    fn size(self, available_size: LogicalSize<f32>) -> LogicalSize<f32> {
        LogicalSize {
            inline_size: self.inline.size(available_size.inline_size),
            block_size: self.block.size(available_size.block_size),
        }
    }
}

/// Move the weak edge or both edges by `amount` according to `bias`.
#[inline(always)]
fn resize_inset_modified_axis(axis: &mut InsetModifiedAxis, bias: InsetBias, amount: f32) {
    match bias {
        InsetBias::Start => axis.end += amount,
        InsetBias::End => axis.start += amount,
        InsetBias::Equal => {
            axis.start += amount / 2.0;
            axis.end += amount / 2.0;
        }
    }
}

/// Build and clamp one axis of the inset-modified containing block from
/// resolved insets, the static-position anchor, and self-alignment.
#[allow(clippy::too_many_arguments)]
#[inline]
fn compute_inset_modified_axis(
    available_size: f32,
    inset: Line<Option<f32>>,
    static_offset: f32,
    static_edge: StaticPositionEdge,
    axis: AbstractAxis,
    static_align_self_axis: AbstractAxis,
    alignment: LogicalAlignment,
) -> InsetModifiedAxis {
    let has_auto_inset = inset.start.is_none() || inset.end.is_none();
    let current_safe_bias = alignment.safe_bias(axis);
    let alternate_safe_bias = alignment.safe_bias(axis.other());
    let mut result = match (inset.start, inset.end) {
        (None, None) => {
            let static_bias = InsetBias::from_static_edge(static_edge);
            let (start, end) = match static_bias {
                InsetBias::Start => (static_offset, 0.0),
                InsetBias::Equal => {
                    let half_size = static_offset.min(available_size - static_offset);
                    (static_offset - half_size, available_size - static_offset - half_size)
                }
                InsetBias::End => (0.0, available_size - static_offset),
            };
            let static_axis_uses_current_alignment = static_align_self_axis == AbstractAxis::Block;
            let safe_inset_bias =
                if static_axis_uses_current_alignment { current_safe_bias } else { alternate_safe_bias };
            InsetModifiedAxis {
                start,
                end,
                has_auto_inset,
                inset_bias: static_bias,
                safe_inset_bias,
                default_inset_bias: None,
                has_default_alignment_overflow: false,
            }
        }
        (start, end) => {
            let inset_bias = match (start, end) {
                (Some(_), None) => InsetBias::Start,
                (None, Some(_)) => InsetBias::End,
                (Some(_), Some(_)) => alignment.inset_bias(axis),
                (None, None) => unreachable!(),
            };
            let both_insets_are_definite = start.is_some() && end.is_some();
            InsetModifiedAxis {
                start: start.unwrap_or(0.0),
                end: end.unwrap_or(0.0),
                has_auto_inset,
                inset_bias,
                safe_inset_bias: both_insets_are_definite.then_some(current_safe_bias).flatten(),
                default_inset_bias: both_insets_are_definite.then(|| alignment.default_bias(axis)).flatten(),
                has_default_alignment_overflow: both_insets_are_definite && alignment.has_default_overflow(axis),
            }
        }
    };
    let size = result.size(available_size);
    if size < 0.0 {
        let clamp_bias = result.default_inset_bias.unwrap_or(result.inset_bias);
        resize_inset_modified_axis(&mut result, clamp_bias, size);
    }
    result
}

/// Build the complete logical inset-modified containing block for an
/// out-of-flow positioned box.
#[inline]
fn compute_inset_modified_containing_block(
    available_size: LogicalSize<f32>,
    inset: LogicalBoxStrut<Option<f32>>,
    static_position: LogicalStaticPosition,
    alignment: LogicalAlignment,
) -> LogicalInsetModifiedContainingBlock {
    LogicalInsetModifiedContainingBlock {
        inline: compute_inset_modified_axis(
            available_size.inline_size,
            Line { start: inset.inline_start, end: inset.inline_end },
            static_position.offset.inline_offset,
            static_position.inline_edge,
            AbstractAxis::Inline,
            static_position.align_self_axis,
            alignment,
        ),
        block: compute_inset_modified_axis(
            available_size.block_size,
            Line { start: inset.block_start, end: inset.block_end },
            static_position.offset.block_offset,
            static_position.block_edge,
            AbstractAxis::Block,
            static_position.align_self_axis,
            alignment,
        ),
    }
}

/// Compute the space available to an out-of-flow box from a flow-relative
/// static-position candidate.
///
/// The candidate's start/center/end bias changes shrink-to-fit space before
/// the box's used size is known: start grows toward end, end grows toward
/// start, and center grows equally until it reaches the nearest edge.
#[inline]
#[cfg(test)]
pub(crate) fn logical_inset_modified_containing_block_size(
    containing_block_size: Size<f32>,
    inset: Rect<Option<f32>>,
    static_position: LogicalStaticPosition,
    writing_direction: WritingDirection,
) -> Size<f32> {
    let logical_size = writing_direction.mode.to_logical(containing_block_size);
    let logical_inset = writing_direction.to_logical_box_strut(inset);
    let alignment = LogicalAlignment::new(None, None, writing_direction, writing_direction);
    let imcb = compute_inset_modified_containing_block(logical_size, logical_inset, static_position, alignment);
    writing_direction.mode.to_physical(imcb.size(logical_size))
}

/// Resolve the fit-content inline size used by an automatically sized
/// absolutely positioned box.
///
/// CSS 2 defines this as `min(max(min-content, available), max-content)`. A single
/// measurement with definite available space is insufficient: nested block and flex
/// containers may return their max-content contribution while they are being measured.
#[inline]
pub(crate) fn fit_content_inline_size(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    mut inputs: ChildLayoutInput,
    available_inline_size: f32,
    inline_axis: AbsoluteAxis,
) -> f32 {
    match inline_axis {
        AbsoluteAxis::Horizontal => inputs.available_space.width = AvailableSpace::MinContent,
        AbsoluteAxis::Vertical => inputs.available_space.height = AvailableSpace::MinContent,
    }
    let min_content = tree.measure_child_size(node, inputs, inline_axis);
    match inline_axis {
        AbsoluteAxis::Horizontal => inputs.available_space.width = AvailableSpace::MaxContent,
        AbsoluteAxis::Vertical => inputs.available_space.height = AvailableSpace::MaxContent,
    }
    let max_content = tree.measure_child_size(node, inputs, inline_axis);

    available_inline_size.max(0.0).max(min_content).min(max_content)
}

/// Resolve auto margins in one axis of an absolutely positioned box.
///
/// Auto margins only participate when both insets in the axis are definite.
/// Negative free space in the containing block's inline direction is assigned
/// to the non-dominant side, while free space in its block direction is shared
/// equally. The selected axis and margins are expressed in the positioned
/// box's writing direction.
#[inline]
fn resolve_absolute_axis_margins(
    margin: Line<Option<f32>>,
    inset_modified_size: f32,
    box_size: f32,
    has_auto_inset: bool,
    is_containing_block_block_direction: bool,
    start_is_dominant: bool,
) -> (Line<f32>, bool) {
    if has_auto_inset || (margin.start.is_some() && margin.end.is_some()) {
        return (Line { start: margin.start.unwrap_or(0.0), end: margin.end.unwrap_or(0.0) }, false);
    }

    let free_space = inset_modified_size - box_size - margin.start.unwrap_or(0.0) - margin.end.unwrap_or(0.0);

    let resolved = match (margin.start, margin.end) {
        (Some(_), Some(_)) => unreachable!(),
        (None, Some(end)) => Line { start: free_space, end },
        (Some(start), None) => Line { start, end: free_space },
        (None, None) if free_space > 0.0 || is_containing_block_block_direction => {
            let start = free_space / 2.0;
            Line { start, end: free_space - start }
        }
        (None, None) if start_is_dominant => Line { start: 0.0, end: free_space },
        (None, None) => Line { start: free_space, end: 0.0 },
    };
    (resolved, true)
}

/// Align one margin box inside its inset-modified containing block and return
/// the border-box logical-start coordinate.
#[inline]
fn resolve_aligned_axis_start(
    available_size: f32,
    imcb: InsetModifiedAxis,
    margin: Line<f32>,
    box_size: f32,
    auto_margins_applied: bool,
) -> f32 {
    if auto_margins_applied {
        return imcb.start + margin.start;
    }

    let margin_box_size = margin.start + box_size + margin.end;
    let mut aligned = imcb;
    let mut free_space = imcb.size(available_size) - margin_box_size;
    let mut bias = imcb.inset_bias;
    let apply_safe_bias = imcb.safe_inset_bias.is_some() && free_space < 0.0;
    if apply_safe_bias {
        free_space = 0.0;
        bias = imcb.safe_inset_bias.unwrap();
    }
    resize_inset_modified_axis(&mut aligned, bias, free_space);

    if imcb.has_default_alignment_overflow && !apply_safe_bias {
        let use_imcb = margin_box_size <= imcb.size(available_size);
        let safe_start = if use_imcb { imcb.start } else { imcb.start.min(0.0) };
        let safe_end = if use_imcb { imcb.end } else { imcb.end.min(0.0) };
        let adjust_start = |axis: &mut InsetModifiedAxis| {
            if axis.start < safe_start {
                axis.end += axis.start - safe_start;
                axis.start = safe_start;
            }
        };
        let adjust_end = |axis: &mut InsetModifiedAxis| {
            if axis.end < safe_end {
                axis.start += axis.end - safe_end;
                axis.end = safe_end;
            }
        };
        match imcb.default_inset_bias.unwrap_or(InsetBias::Start) {
            InsetBias::Start => {
                adjust_end(&mut aligned);
                adjust_start(&mut aligned);
            }
            InsetBias::End => {
                adjust_start(&mut aligned);
                adjust_end(&mut aligned);
            }
            InsetBias::Equal => {
                adjust_start(&mut aligned);
                adjust_end(&mut aligned);
            }
        }
    }

    aligned.start + margin.start
}

/// Size and place one absolutely positioned box in its actual containing
/// block.
///
/// Formatting contexts are responsible only for producing
/// [`OutOfFlowItem::static_position`]. This resolver owns percentage and inset
/// resolution, the inset-modified containing block, intrinsic sizing, aspect
/// ratio transfer, auto margins, and the final physical offset. Keeping those
/// operations together prevents block, flex and grid from evolving subtly
/// different positioned-layout semantics.
pub(crate) fn layout_out_of_flow_item(
    tree: &mut impl LayoutPartialTree,
    item: OutOfFlowItem,
    containing_block: OutOfFlowContainingBlock,
) -> Option<OutOfFlowLayoutOutput> {
    let OutOfFlowContainingBlock { outer_size, area_offset, area_size, writing_direction } = containing_block;
    let writing_mode = writing_direction.mode;
    let area_width = area_size.width;
    let area_height = area_size.height;
    let percentage_basis = writing_mode.to_logical(area_size).inline_size;
    let aspect_ratio = tree.get_resolved_aspect_ratio(item.node);
    let child_writing_mode = tree.get_writing_mode(item.node);
    let child_style = tree.get_core_container_style(item.node);

    if child_style.box_generation_mode() == BoxGenerationMode::None || child_style.position() != Position::Absolute {
        return None;
    }

    let overflow = child_style.overflow();
    let child_direction = child_style.direction();
    let child_writing_direction = WritingDirection::new(child_writing_mode, child_direction);
    let align_self = child_style.align_self();
    let justify_self = child_style.justify_self();
    let scrollbar_width = child_style.scrollbar_width();
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let scrollbar_gutter = overflow.transpose().map(|overflow| match overflow {
        Overflow::Scroll => scrollbar_width,
        _ => 0.0,
    });
    let margin =
        child_style.margin().map(|value| value.resolve_to_option(percentage_basis, |val, basis| tree.calc(val, basis)));
    let padding = child_style.padding().resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis));
    let border = child_style.border().resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis));
    let padding_border_sum = (padding + border).sum_axes();
    let box_sizing_adjustment =
        if child_style.box_sizing() == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };

    let left = child_style.inset().left.maybe_resolve(area_width, |val, basis| tree.calc(val, basis));
    let right = child_style.inset().right.maybe_resolve(area_width, |val, basis| tree.calc(val, basis));
    let top = child_style.inset().top.maybe_resolve(area_height, |val, basis| tree.calc(val, basis));
    let bottom = child_style.inset().bottom.maybe_resolve(area_height, |val, basis| tree.calc(val, basis));
    let raw_size = child_style.size();
    let raw_min_size = child_style.min_size();
    let raw_max_size = child_style.max_size();
    let is_compressible_replaced = child_style.is_compressible_replaced();
    drop(child_style);

    let mut physical_static_position = item.static_position.to_physical(writing_direction, outer_size);
    physical_static_position.offset.x -= area_offset.x;
    physical_static_position.offset.y -= area_offset.y;
    let static_position_in_area = physical_static_position.to_logical(child_writing_direction, area_size);
    let logical_area_size = child_writing_mode.to_logical(area_size);
    let logical_inset = child_writing_direction.to_logical_box_strut(Rect { left, right, top, bottom });
    let alignment = LogicalAlignment::new(align_self, justify_self, writing_direction, child_writing_direction);
    let inset_modified_containing_block =
        compute_inset_modified_containing_block(logical_area_size, logical_inset, static_position_in_area, alignment);
    let logical_inset_modified_size = inset_modified_containing_block.size(logical_area_size);
    let physical_inset_modified_size = child_writing_mode.to_physical(logical_inset_modified_size);
    let inset_modified_size =
        (physical_inset_modified_size - margin.map(|value| value.unwrap_or(0.0)).sum_axes()).f32_max(Size::ZERO);
    let available_width = inset_modified_size.width;
    let available_height = inset_modified_size.height;
    let inline_axis = child_writing_mode.inline_axis();
    let block_axis = child_writing_mode.block_axis();
    let auto_behavior = |axis: AbstractAxis, has_auto_inset: bool| {
        if has_auto_inset {
            AutoSizeBehavior::FitContent
        } else if alignment.is_stretch(axis) {
            AutoSizeBehavior::StretchExplicit
        } else if alignment.is_normal(axis) {
            if is_compressible_replaced {
                AutoSizeBehavior::FitContent
            } else {
                AutoSizeBehavior::StretchImplicit
            }
        } else {
            AutoSizeBehavior::FitContent
        }
    };
    let inline_auto_behavior =
        auto_behavior(AbstractAxis::Inline, inset_modified_containing_block.inline.has_auto_inset);
    let block_auto_behavior = auto_behavior(AbstractAxis::Block, inset_modified_containing_block.block.has_auto_inset);
    let sizing_inputs = LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: SizingMode::InherentSize,
        sizing_purpose: SizingPurpose::IntrinsicContribution,
        axis: RequestedAxis::from(inline_axis),
        inline_auto_behavior,
        block_auto_behavior,
        known_dimensions: Size::NONE,
        definite_dimensions: Size::NONE,
        parent_size: area_size.map(Some),
        parent_writing_mode: writing_mode,
        available_space: Size {
            width: AvailableSpace::Definite(available_width),
            height: AvailableSpace::Definite(available_height),
        },
        block_margins_are_collapsible: Line::FALSE,
    };
    let node_sizing = resolve_node_size_constraints(
        tree,
        item.node,
        sizing_inputs,
        NodeSizeConstraintInput {
            raw_size,
            raw_min_size,
            raw_max_size,
            box_sizing_adjustment,
            padding_border_size: padding_border_sum,
            aspect_ratio,
            contained_outer_size: tree.get_size_containment(item.node).resolve_outer_size(
                Size::ZERO,
                padding_border_sum + Size { width: scrollbar_gutter.x, height: scrollbar_gutter.y },
            ),
        },
    );
    let block_axis_constraints = node_sizing.constraints.block_axis_constraints(child_writing_mode);
    let mut min_size = node_sizing.min_size.or(padding_border_sum.map(Some)).maybe_max(padding_border_sum);
    let mut max_size = node_sizing.max_size;
    let mut known_dimensions = node_sizing.outer_size.maybe_clamp(min_size, max_size);

    if known_dimensions.get_abs(inline_axis).is_none() {
        let available_inline_size = inset_modified_size.get_abs(inline_axis);
        let fitted_inline_size = fit_content_inline_size(
            tree,
            item.node,
            ChildLayoutInput::new(
                known_dimensions,
                area_size.map(Some),
                writing_mode,
                Size {
                    width: AvailableSpace::Definite(available_width),
                    height: AvailableSpace::Definite(available_height.maybe_clamp(min_size.height, max_size.height)),
                },
                SizingMode::ContentSize,
                Line::FALSE,
            ),
            available_inline_size,
            inline_axis,
        );
        match inline_axis {
            AbsoluteAxis::Horizontal => known_dimensions.width = Some(fitted_inline_size),
            AbsoluteAxis::Vertical => known_dimensions.height = Some(fitted_inline_size),
        }
        known_dimensions = apply_preferred_aspect_ratio(
            known_dimensions,
            raw_size.map(|dimension| dimension.is_auto()),
            child_writing_mode,
            inline_auto_behavior,
            block_auto_behavior,
            aspect_ratio,
            padding_border_sum,
        )
        .maybe_clamp(min_size, max_size);
    }

    let raw_logical_size = child_writing_mode.to_logical(raw_size);
    let raw_logical_min_size = child_writing_mode.to_logical(raw_min_size);
    let raw_logical_max_size = child_writing_mode.to_logical(raw_max_size);
    let content_based_block_size = ContentBasedBlockSize::new(
        BlockSizeProperties::new(
            raw_logical_size.block_size,
            raw_logical_min_size.block_size,
            raw_logical_max_size.block_size,
        ),
        aspect_ratio,
        padding_border_sum,
        block_auto_behavior.is_content_based(aspect_ratio.ratio.is_some()),
        is_scroll_container,
        None,
    );
    let content_based_block_constraints = resolve_content_based_block_size_constraints(
        tree,
        item.node,
        ChildLayoutInput::new(
            known_dimensions,
            area_size.map(Some),
            writing_mode,
            Size {
                width: AvailableSpace::Definite(available_width),
                height: AvailableSpace::Definite(available_height),
            },
            SizingMode::ContentSize,
            Line::FALSE,
        )
        .with_block_auto_behavior(block_auto_behavior),
        content_based_block_size,
    );
    content_based_block_constraints.apply_to_block_axis(
        child_writing_mode,
        block_axis_constraints,
        padding_border_sum,
        &mut known_dimensions,
        &mut min_size,
        &mut max_size,
    );

    if known_dimensions.get_abs(block_axis).is_none()
        && !block_auto_behavior.is_content_based(aspect_ratio.ratio.is_some())
    {
        let stretched_block_size = inset_modified_size.get_abs(block_axis);
        match block_axis {
            AbsoluteAxis::Horizontal => known_dimensions.width = Some(stretched_block_size),
            AbsoluteAxis::Vertical => known_dimensions.height = Some(stretched_block_size),
        }
        known_dimensions = apply_preferred_aspect_ratio(
            known_dimensions,
            raw_size.map(|dimension| dimension.is_auto()),
            child_writing_mode,
            inline_auto_behavior,
            block_auto_behavior,
            aspect_ratio,
            padding_border_sum,
        )
        .maybe_clamp(min_size, max_size);
    }

    let child_available_space = Size {
        width: AvailableSpace::Definite(available_width.maybe_clamp(min_size.width, max_size.width)),
        height: AvailableSpace::Definite(available_height.maybe_clamp(min_size.height, max_size.height)),
    };
    let measured_size = tree.measure_child_size_both(
        item.node,
        ChildLayoutInput::new(
            known_dimensions,
            area_size.map(Some),
            writing_mode,
            child_available_space,
            SizingMode::ContentSize,
            Line::FALSE,
        )
        .with_block_auto_behavior(block_auto_behavior),
    );
    let final_size = known_dimensions.unwrap_or(measured_size).maybe_clamp(min_size, max_size);
    let layout_output = tree.compute_child_layout(
        item.node,
        LayoutInput {
            known_dimensions: final_size.map(Some),
            definite_dimensions: known_dimensions,
            parent_size: area_size.map(Some),
            parent_writing_mode: writing_mode,
            available_space: child_available_space,
            sizing_mode: SizingMode::ContentSize,
            sizing_purpose: SizingPurpose::Layout,
            axis: RequestedAxis::Both,
            inline_auto_behavior,
            block_auto_behavior,
            run_mode: RunMode::PerformLayout,
            block_margins_are_collapsible: Line::FALSE,
        },
    );

    let logical_margin = child_writing_direction.to_logical_box_strut(margin);
    let logical_box_size = child_writing_mode.to_logical(final_size);
    let is_orthogonal = child_writing_mode.is_orthogonal_to(writing_mode);
    let (inline_margin, inline_auto_margins_applied) = resolve_absolute_axis_margins(
        Line { start: logical_margin.inline_start, end: logical_margin.inline_end },
        logical_inset_modified_size.inline_size,
        logical_box_size.inline_size,
        inset_modified_containing_block.inline.has_auto_inset,
        is_orthogonal,
        alignment.container_start.inline == InsetBias::Start,
    );
    let (block_margin, block_auto_margins_applied) = resolve_absolute_axis_margins(
        Line { start: logical_margin.block_start, end: logical_margin.block_end },
        logical_inset_modified_size.block_size,
        logical_box_size.block_size,
        inset_modified_containing_block.block.has_auto_inset,
        !is_orthogonal,
        alignment.container_start.block == InsetBias::Start,
    );
    let resolved_margin = child_writing_direction.to_physical_box_strut(LogicalBoxStrut {
        inline_start: inline_margin.start,
        inline_end: inline_margin.end,
        block_start: block_margin.start,
        block_end: block_margin.end,
    });
    let logical_location = LogicalOffset {
        inline_offset: resolve_aligned_axis_start(
            logical_area_size.inline_size,
            inset_modified_containing_block.inline,
            inline_margin,
            logical_box_size.inline_size,
            inline_auto_margins_applied,
        ),
        block_offset: resolve_aligned_axis_start(
            logical_area_size.block_size,
            inset_modified_containing_block.block,
            block_margin,
            logical_box_size.block_size,
            block_auto_margins_applied,
        ),
    };
    let location_in_area = child_writing_direction.converter(area_size).to_physical_point(logical_location, final_size);
    let location = Point { x: location_in_area.x + area_offset.x, y: location_in_area.y + area_offset.y };
    let scrollbar_size = Size {
        width: if overflow.y == Overflow::Scroll { scrollbar_width } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { scrollbar_width } else { 0.0 },
    };
    tree.set_unrounded_layout(
        item.node,
        &Layout {
            order: item.order,
            size: final_size,
            #[cfg(feature = "content_size")]
            content_size: layout_output.content_size,
            scrollbar_size,
            location,
            padding,
            border,
            margin: resolved_margin,
        },
    );

    #[cfg(feature = "content_size")]
    let content_size = compute_content_size_contribution(
        Point { x: location.x - area_offset.x, y: location.y - area_offset.y },
        final_size,
        layout_output.content_size,
        overflow,
    );
    #[cfg(not(feature = "content_size"))]
    let content_size = Size::ZERO;
    Some(OutOfFlowLayoutOutput { content_size })
}

#[cfg(test)]
mod tests {
    use super::logical_inset_modified_containing_block_size;
    use crate::{
        AbstractAxis, Direction, LogicalOffset, LogicalStaticPosition, Rect, Size, StaticPositionEdge,
        WritingDirection, WritingMode,
    };

    const AUTO_INSETS: Rect<Option<f32>> = Rect { left: None, right: None, top: None, bottom: None };

    fn candidate(inline_edge: StaticPositionEdge) -> LogicalStaticPosition {
        LogicalStaticPosition {
            offset: LogicalOffset { inline_offset: 20.0, block_offset: 30.0 },
            inline_edge,
            block_edge: StaticPositionEdge::Center,
            align_self_axis: AbstractAxis::Block,
        }
    }

    #[test]
    fn static_position_edge_controls_out_of_flow_available_space() {
        let direction = WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr);
        let containing_size = Size { width: 100.0, height: 80.0 };

        assert_eq!(
            logical_inset_modified_containing_block_size(
                containing_size,
                AUTO_INSETS,
                candidate(StaticPositionEdge::Start),
                direction,
            ),
            Size { width: 80.0, height: 60.0 },
        );
        assert_eq!(
            logical_inset_modified_containing_block_size(
                containing_size,
                AUTO_INSETS,
                candidate(StaticPositionEdge::Center),
                direction,
            ),
            Size { width: 40.0, height: 60.0 },
        );
        assert_eq!(
            logical_inset_modified_containing_block_size(
                containing_size,
                AUTO_INSETS,
                candidate(StaticPositionEdge::End),
                direction,
            ),
            Size { width: 20.0, height: 60.0 },
        );
    }

    #[test]
    fn logical_static_space_projects_through_vertical_writing_modes() {
        let result = logical_inset_modified_containing_block_size(
            Size { width: 80.0, height: 100.0 },
            AUTO_INSETS,
            candidate(StaticPositionEdge::End),
            WritingDirection::new(WritingMode::VerticalRl, Direction::Rtl),
        );
        assert_eq!(result, Size { width: 60.0, height: 20.0 },);
    }
}
