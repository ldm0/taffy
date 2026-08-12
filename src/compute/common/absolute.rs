use crate::geometry::{
    AbsoluteAxis, Line, LogicalBoxStrut, LogicalOffset, LogicalSize, LogicalStaticPosition, Point, Rect, Size,
    StaticPositionEdge, WritingDirection,
};
use crate::style::{AvailableSpace, BoxGenerationMode, BoxSizing, CoreStyle, Overflow, Position};
use crate::tree::{
    AutoSizeBehavior, ChildLayoutInput, Layout, LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId,
    OutOfFlowContainingBlock, RequestedAxis, RunMode, SizingMode, SizingPurpose,
};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};

use super::aspect_ratio::apply_preferred_aspect_ratio;
#[cfg(feature = "content_size")]
use super::content_size::compute_content_size_contribution;
use super::intrinsic_size::{
    measure_intrinsic_block_size_constraints, resolve_node_size_constraints, BlockSizeProperties,
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

/// Compute the space available to an out-of-flow box from a flow-relative
/// static-position candidate.
///
/// The candidate's start/center/end bias changes shrink-to-fit space before
/// the box's used size is known: start grows toward end, end grows toward
/// start, and center grows equally until it reaches the nearest edge.
#[inline]
pub(crate) fn logical_inset_modified_containing_block_size(
    containing_block_size: Size<f32>,
    inset: Rect<Option<f32>>,
    static_position: LogicalStaticPosition,
    writing_direction: WritingDirection,
) -> Size<f32> {
    #[inline(always)]
    fn axis_size(
        containing_size: f32,
        start: Option<f32>,
        end: Option<f32>,
        static_offset: f32,
        static_edge: StaticPositionEdge,
    ) -> f32 {
        let inset_modified_size = match (start, end) {
            (Some(start), Some(end)) => containing_size - start - end,
            (Some(start), None) => containing_size - start,
            (None, Some(end)) => containing_size - end,
            (None, None) => match static_edge {
                StaticPositionEdge::Start => containing_size - static_offset,
                StaticPositionEdge::Center => 2.0 * static_offset.min(containing_size - static_offset),
                StaticPositionEdge::End => static_offset,
            },
        };
        inset_modified_size.max(0.0)
    }

    let logical_size = writing_direction.mode.to_logical(containing_block_size);
    let logical_inset = writing_direction.to_logical_box_strut(inset);
    writing_direction.mode.to_physical(LogicalSize {
        inline_size: axis_size(
            logical_size.inline_size,
            logical_inset.inline_start,
            logical_inset.inline_end,
            static_position.offset.inline_offset,
            static_position.inline_edge,
        ),
        block_size: axis_size(
            logical_size.block_size,
            logical_inset.block_start,
            logical_inset.block_end,
            static_position.offset.block_offset,
            static_position.block_edge,
        ),
    })
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
    inset: Line<Option<f32>>,
    area_size: f32,
    box_size: f32,
    is_containing_block_block_direction: bool,
    start_is_dominant: bool,
) -> Line<f32> {
    if inset.start.is_none() || inset.end.is_none() {
        return Line { start: margin.start.unwrap_or(0.0), end: margin.end.unwrap_or(0.0) };
    }

    let free_space = area_size
        - inset.start.unwrap()
        - inset.end.unwrap()
        - box_size
        - margin.start.unwrap_or(0.0)
        - margin.end.unwrap_or(0.0);

    match (margin.start, margin.end) {
        (Some(start), Some(end)) => Line { start, end },
        (None, Some(end)) => Line { start: free_space, end },
        (Some(start), None) => Line { start, end: free_space },
        (None, None) if free_space > 0.0 || is_containing_block_block_direction => {
            let start = free_space / 2.0;
            Line { start, end: free_space - start }
        }
        (None, None) if start_is_dominant => Line { start: 0.0, end: free_space },
        (None, None) => Line { start: free_space, end: 0.0 },
    }
}

/// Resolve one physical low-axis location from physical insets and margins.
///
/// When both insets are definite, the containing block's logical start side
/// is the dominant equation edge. This applies to either physical axis in
/// vertical writing modes.
#[inline]
fn resolve_absolute_axis_location(
    inset: Line<Option<f32>>,
    area_size: f32,
    box_size: f32,
    margin: Line<f32>,
    static_location: f32,
    low_side_is_dominant: bool,
) -> f32 {
    match (inset.start, inset.end) {
        (Some(_), Some(high)) if !low_side_is_dominant => area_size - box_size - high - margin.end,
        (Some(low), _) => low + margin.start,
        (None, Some(high)) => area_size - box_size - high - margin.end,
        (None, None) => static_location,
    }
}

/// Resolve a static-position anchor to the border-box origin after used size
/// and margins are known.
#[inline]
fn resolve_static_position_location(
    static_position: LogicalStaticPosition,
    writing_direction: WritingDirection,
    containing_outer_size: Size<f32>,
    box_size: Size<f32>,
    margin: Rect<f32>,
) -> Point<f32> {
    #[inline(always)]
    fn axis_start(anchor: f32, edge: StaticPositionEdge, size: f32, margin_start: f32, margin_end: f32) -> f32 {
        match edge {
            StaticPositionEdge::Start => anchor + margin_start,
            StaticPositionEdge::Center => anchor + (margin_start - margin_end - size) / 2.0,
            StaticPositionEdge::End => anchor - size - margin_end,
        }
    }

    let logical_size = writing_direction.mode.to_logical(box_size);
    let logical_margin = writing_direction.to_logical_box_strut(margin);
    let logical_location = LogicalOffset {
        inline_offset: axis_start(
            static_position.offset.inline_offset,
            static_position.inline_edge,
            logical_size.inline_size,
            logical_margin.inline_start,
            logical_margin.inline_end,
        ),
        block_offset: axis_start(
            static_position.offset.block_offset,
            static_position.block_edge,
            logical_size.block_size,
            logical_margin.block_start,
            logical_margin.block_end,
        ),
    };
    writing_direction.converter(containing_outer_size).to_physical_point(logical_location, box_size)
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
    let block_auto_behavior = match child_writing_mode.block_axis() {
        AbsoluteAxis::Horizontal if left.is_some() && right.is_some() => AutoSizeBehavior::StretchExplicit,
        AbsoluteAxis::Vertical if top.is_some() && bottom.is_some() => AutoSizeBehavior::StretchExplicit,
        _ => AutoSizeBehavior::FitContent,
    };
    let inline_auto_behavior = match child_writing_mode.inline_axis() {
        AbsoluteAxis::Horizontal if left.is_some() && right.is_some() => AutoSizeBehavior::StretchExplicit,
        AbsoluteAxis::Vertical if top.is_some() && bottom.is_some() => AutoSizeBehavior::StretchExplicit,
        _ => AutoSizeBehavior::FitContent,
    };

    let raw_size = child_style.size();
    let raw_min_size = child_style.min_size();
    let raw_max_size = child_style.max_size();
    drop(child_style);

    let mut physical_static_position = item.static_position.to_physical(writing_direction, outer_size);
    physical_static_position.offset.x -= area_offset.x;
    physical_static_position.offset.y -= area_offset.y;
    let static_position_in_area = physical_static_position.to_logical(child_writing_direction, area_size);
    let inset_modified_containing_block = logical_inset_modified_containing_block_size(
        area_size,
        Rect { left, right, top, bottom },
        static_position_in_area,
        child_writing_direction,
    );
    let inset_modified_size =
        (inset_modified_containing_block - margin.map(|value| value.unwrap_or(0.0)).sum_axes()).f32_max(Size::ZERO);
    let available_width = inset_modified_size.width;
    let available_height = inset_modified_size.height;
    let inline_axis = child_writing_mode.inline_axis();
    let block_axis = child_writing_mode.block_axis();
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
    let intrinsic_block_constraints = measure_intrinsic_block_size_constraints(
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
    intrinsic_block_constraints.apply_to_block_axis(
        child_writing_mode,
        block_axis_constraints,
        padding_border_sum,
        &mut known_dimensions,
        &mut min_size,
        &mut max_size,
    );

    if known_dimensions.get_abs(block_axis).is_none() && block_auto_behavior == AutoSizeBehavior::StretchExplicit {
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
    let logical_inset = child_writing_direction.to_logical_box_strut(Rect { left, right, top, bottom });
    let logical_area_size = child_writing_mode.to_logical(area_size);
    let logical_box_size = child_writing_mode.to_logical(final_size);
    let containing_start_sides =
        child_writing_direction.to_logical_box_strut(writing_direction.to_physical_box_strut(LogicalBoxStrut {
            inline_start: true,
            inline_end: false,
            block_start: true,
            block_end: false,
        }));
    let is_orthogonal = child_writing_mode.is_orthogonal_to(writing_mode);
    let inline_margin = resolve_absolute_axis_margins(
        Line { start: logical_margin.inline_start, end: logical_margin.inline_end },
        Line { start: logical_inset.inline_start, end: logical_inset.inline_end },
        logical_area_size.inline_size,
        logical_box_size.inline_size,
        is_orthogonal,
        containing_start_sides.inline_start,
    );
    let block_margin = resolve_absolute_axis_margins(
        Line { start: logical_margin.block_start, end: logical_margin.block_end },
        Line { start: logical_inset.block_start, end: logical_inset.block_end },
        logical_area_size.block_size,
        logical_box_size.block_size,
        !is_orthogonal,
        containing_start_sides.block_start,
    );
    let resolved_margin = child_writing_direction.to_physical_box_strut(LogicalBoxStrut {
        inline_start: inline_margin.start,
        inline_end: inline_margin.end,
        block_start: block_margin.start,
        block_end: block_margin.end,
    });
    let static_location_in_area = resolve_static_position_location(
        static_position_in_area,
        child_writing_direction,
        area_size,
        final_size,
        resolved_margin,
    );
    let horizontal_low_is_dominant =
        !writing_mode.is_axis_flow_reversed(AbsoluteAxis::Horizontal, writing_direction.direction);
    let vertical_low_is_dominant =
        !writing_mode.is_axis_flow_reversed(AbsoluteAxis::Vertical, writing_direction.direction);
    let x = resolve_absolute_axis_location(
        Line { start: left, end: right },
        area_width,
        final_size.width,
        Line { start: resolved_margin.left, end: resolved_margin.right },
        static_location_in_area.x,
        horizontal_low_is_dominant,
    ) + area_offset.x;
    let y = resolve_absolute_axis_location(
        Line { start: top, end: bottom },
        area_height,
        final_size.height,
        Line { start: resolved_margin.top, end: resolved_margin.bottom },
        static_location_in_area.y,
        vertical_low_is_dominant,
    ) + area_offset.y;
    let location = Point { x, y };
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
