//! Alignment of tracks and final positioning of items
use super::types::GridTrack;
use crate::compute::common::alignment::{
    apply_alignment_fallback, compute_alignment_offset, resolve_self_alignment, resolve_self_alignment_safety,
};
use crate::compute::common::aspect_ratio::apply_preferred_aspect_ratio;
use crate::compute::common::baseline::{logical_block_baseline, BaselineGroup};
use crate::compute::common::intrinsic_size::{
    fit_content_inline_size, resolve_content_based_block_size_constraints, resolve_node_size_constraints,
    BlockSizeProperties, ContentBasedBlockSize, NodeSizeConstraintInput,
};
use crate::geometry::{
    AbsoluteAxis, InBothAbstractAxis, Line, LogicalBoxStrut, LogicalOffset, LogicalSize, LogicalStaticPosition, Point,
    Rect, Size, StaticPositionEdge,
};
use crate::style::{
    AlignContent, AlignItems, AlignItemsKeyword, AlignSelf, AvailableSpace, CoreStyle, GridItemStyle, Overflow,
    Position,
};
use crate::tree::{AutoSizeBehavior, ChildLayoutInput, Layout, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::sys::f32_max;
use crate::util::{MaybeMath, ResolveOrZero};

#[cfg(feature = "content_size")]
use crate::compute::common::content_size::compute_content_size_contribution;
use crate::{BoxSizing, Direction, LayoutGridContainer, WritingMode};

use super::flow::GridFlow;

/// Final block-axis geometry and baseline data for a positioned grid item.
///
/// Baselines belong to the final child layout, not to the temporary intrinsic
/// measurement used to calculate baseline shims. Keeping both sets here lets
/// the grid container propagate distinct first and last baselines to its own
/// parent formatting context.
pub(super) struct GridItemPlacement {
    /// Contribution of the positioned item to the grid's scrollable content.
    pub(super) content_size_contribution: Size<f32>,
    /// Used block-start position relative to the grid container.
    pub(super) block_offset: f32,
    /// Used border-box block-size.
    pub(super) block_size: f32,
    /// First baseline relative to the item's border box.
    pub(super) first_baseline: Option<f32>,
    /// Last baseline relative to the item's border box.
    pub(super) last_baseline: Option<f32>,
}

/// Keep the final numeric value only on axes whose sizing source is definite.
#[inline(always)]
fn definite_dimensions(size: Size<Option<f32>>, definite_axes: Size<bool>) -> Size<Option<f32>> {
    Size {
        width: if definite_axes.width { size.width } else { None },
        height: if definite_axes.height { size.height } else { None },
    }
}

/// Propagate definiteness when a preferred ratio synthesizes a previously
/// unresolved axis from an independently definite opposite axis.
#[inline(always)]
fn transfer_ratio_definiteness(before: Size<Option<f32>>, after: Size<Option<f32>>, definite_axes: &mut Size<bool>) {
    if before.width.is_none() && after.width.is_some() && definite_axes.height {
        definite_axes.width = true;
    }
    if before.height.is_none() && after.height.is_some() && definite_axes.width {
        definite_axes.height = true;
    }
}

/// Align the grid tracks within the grid according to the align-content (rows) or
/// justify-content (columns) property. This only does anything if the size of the
/// grid is not equal to the size of the grid container in the axis being aligned.
pub(super) fn align_tracks(
    grid_container_content_box_size: f32,
    padding: Line<f32>,
    border: Line<f32>,
    tracks: &mut [GridTrack],
    track_alignment_style: AlignContent,
    axis_is_reversed: bool,
) {
    let used_size: f32 = tracks.iter().map(|track| track.base_size).sum();
    let free_space = grid_container_content_box_size - used_size;
    let origin = padding.start + border.start;

    // Count the number of non-collapsed tracks (not counting gutters)
    let num_tracks = tracks.iter().skip(1).step_by(2).filter(|track| !track.is_collapsed).count();

    // Grid layout treats gaps as full tracks rather than applying them at alignment so we
    // simply pass zero here. Grid layout is never reversed.
    let gap = 0.0;
    let layout_is_reversed = false;
    let track_alignment = apply_alignment_fallback(free_space, num_tracks, track_alignment_style);
    let track_alignment = if axis_is_reversed { track_alignment.reversed() } else { track_alignment };

    // If every track is collapsed then no track receives the alignment offset below, but the
    // grid's lines should still be aligned within the container (e.g. at the inline-start edge
    // for RTL), so apply the offset to the origin instead.
    let empty_grid_offset = if num_tracks == 0 {
        compute_alignment_offset(free_space, num_tracks, gap, track_alignment, layout_is_reversed, true)
    } else {
        0.0
    };

    // Compute offsets
    let mut total_offset = origin + empty_grid_offset;
    let mut seen_non_collapsed_track = false;
    tracks.iter_mut().enumerate().for_each(|(i, track)| {
        // Odd tracks are gutters (but slices are zero-indexed, so odd tracks have even indices)
        let is_gutter = i % 2 == 0;
        let is_non_collapsed_track = !is_gutter && !track.is_collapsed;

        // Alignment offsets should be applied only to non-collapsed tracks.
        let is_first = is_non_collapsed_track && !seen_non_collapsed_track;

        let offset = if is_non_collapsed_track {
            compute_alignment_offset(free_space, num_tracks, gap, track_alignment, layout_is_reversed, is_first)
        } else {
            0.0
        };

        track.offset = total_offset + offset;
        total_offset = total_offset + offset + track.base_size;
        if is_non_collapsed_track {
            seen_non_collapsed_track = true;
        }
    });
}

/// Align and size a grid item into it's final position
#[allow(clippy::too_many_arguments)]
pub(super) fn align_and_position_item(
    tree: &mut impl LayoutGridContainer,
    node: NodeId,
    order: u32,
    grid_area: Rect<f32>,
    container_alignment_styles: InBothAbstractAxis<Option<AlignItems>>,
    baseline_shim: InBothAbstractAxis<f32>,
    baseline_group: InBothAbstractAxis<BaselineGroup>,
    baseline_fallback: InBothAbstractAxis<Option<AlignSelf>>,
    direction: Direction,
    parent_writing_mode: WritingMode,
    container_border_box_size: Size<f32>,
    container_border: Rect<f32>,
) -> GridItemPlacement {
    let grid_area_size = Size { width: grid_area.right - grid_area.left, height: grid_area.bottom - grid_area.top };
    let flow = GridFlow::new(parent_writing_mode, direction);
    let converter = flow.writing_direction().converter(container_border_box_size);
    let logical_grid_area_size = converter.to_logical_size(grid_area_size);
    let logical_grid_area_offset =
        converter.to_logical_point(Point { x: grid_area.left, y: grid_area.top }, grid_area_size);
    let percentage_basis = logical_grid_area_size.inline_size;

    let aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let item_writing_mode = tree.get_writing_mode(node);
    let style = tree.get_grid_child_style(node);

    let overflow = style.overflow();
    let scrollbar_width = style.scrollbar_width();
    let is_compressible_replaced = style.is_compressible_replaced();
    let item_direction = style.direction();
    let inline_self = GridItemStyle::justify_self(&style).map(|align| {
        align.resolve_axis_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.inline_axis(),
        )
    });
    let block_self = GridItemStyle::align_self(&style).map(|align| {
        align.resolve_axis_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.block_axis(),
        )
    });
    let logical_container_alignment = InBothAbstractAxis {
        inline: container_alignment_styles.inline.map(|align| {
            align.resolve_axis_relative(
                item_writing_mode,
                item_direction,
                parent_writing_mode,
                direction,
                parent_writing_mode.inline_axis(),
            )
        }),
        block: container_alignment_styles.block.map(|align| {
            align.resolve_axis_relative(
                item_writing_mode,
                item_direction,
                parent_writing_mode,
                direction,
                parent_writing_mode.block_axis(),
            )
        }),
    };
    let padding =
        style.padding().map(|p| p.resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis)));
    let border = style.border().map(|p| p.resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis)));
    let padding_border_size = (padding + border).sum_axes();

    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    let raw_size = style.size();
    let raw_min_size = style.min_size();
    let raw_max_size = style.max_size();

    let margin =
        style.margin().map(|margin| margin.resolve_to_option(percentage_basis, |val, basis| tree.calc(val, basis)));
    let logical_margin = flow.writing_direction().to_logical_box_strut(margin);

    drop(style);

    let logical_grid_area_minus_item_margins_size = LogicalSize {
        inline_size: logical_grid_area_size
            .inline_size
            .maybe_sub(logical_margin.inline_start)
            .maybe_sub(logical_margin.inline_end)
            - baseline_shim.inline,
        block_size: logical_grid_area_size
            .block_size
            .maybe_sub(logical_margin.block_start)
            .maybe_sub(logical_margin.block_end)
            - baseline_shim.block,
    };
    let grid_area_minus_item_margins_size = flow.to_physical_size(logical_grid_area_minus_item_margins_size);
    let item_available_size = grid_area_minus_item_margins_size;

    let mut alignment_styles = InBothAbstractAxis {
        inline: inline_self.or(logical_container_alignment.inline).unwrap_or(AlignSelf::NORMAL),
        block: block_self.or(logical_container_alignment.block).unwrap_or(AlignSelf::NORMAL),
    };
    if let Some(fallback) = baseline_fallback.inline {
        alignment_styles.inline = fallback;
    }
    if let Some(fallback) = baseline_fallback.block {
        alignment_styles.block = fallback;
    }
    let normal_auto_size =
        if is_compressible_replaced { AutoSizeBehavior::FitContent } else { AutoSizeBehavior::StretchImplicit };
    let resolved_alignment = InBothAbstractAxis {
        inline: resolve_self_alignment(alignment_styles.inline, AlignSelf::START, normal_auto_size),
        block: resolve_self_alignment(alignment_styles.block, AlignSelf::START, normal_auto_size),
    };
    alignment_styles =
        InBothAbstractAxis { inline: resolved_alignment.inline.position, block: resolved_alignment.block.position };
    let mut physical_auto_size = flow.to_physical_axes(InBothAbstractAxis {
        inline: resolved_alignment.inline.auto_size,
        block: resolved_alignment.block.auto_size,
    });
    if margin.left.is_none() || margin.right.is_none() {
        physical_auto_size.horizontal = AutoSizeBehavior::FitContent;
    }
    if margin.top.is_none() || margin.bottom.is_none() {
        physical_auto_size.vertical = AutoSizeBehavior::FitContent;
    }
    let (inline_auto_behavior, block_auto_behavior) = match item_writing_mode.inline_axis() {
        AbsoluteAxis::Horizontal => (physical_auto_size.horizontal, physical_auto_size.vertical),
        AbsoluteAxis::Vertical => (physical_auto_size.vertical, physical_auto_size.horizontal),
    };
    let sizing_inputs = ChildLayoutInput::new(
        Size::NONE,
        grid_area_size.map(Some),
        parent_writing_mode,
        item_available_size.map(AvailableSpace::Definite),
        SizingMode::InherentSize,
        Line::FALSE,
    )
    .with_inline_auto_behavior(inline_auto_behavior)
    .with_block_auto_behavior(block_auto_behavior)
    .into_layout();
    let scrollbar_size = Size {
        width: if overflow.y == Overflow::Scroll { scrollbar_width } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { scrollbar_width } else { 0.0 },
    };
    let contained_outer_size =
        tree.get_size_containment(node).resolve_explicit_outer_size(padding_border_size + scrollbar_size);
    let node_sizing = resolve_node_size_constraints(
        tree,
        node,
        sizing_inputs,
        NodeSizeConstraintInput {
            raw_size,
            raw_min_size,
            raw_max_size,
            box_sizing_adjustment,
            padding_border_size,
            aspect_ratio,
            contained_outer_size,
        },
    );
    let resolved = node_sizing.constraints;
    let block_axis_constraints = resolved.block_axis_constraints(item_writing_mode);
    let raw_logical_size = item_writing_mode.to_logical(raw_size);
    let raw_logical_min_size = item_writing_mode.to_logical(raw_min_size);
    let raw_logical_max_size = item_writing_mode.to_logical(raw_max_size);
    let mut inherent_size = resolved.size;
    let item_inline_axis = item_writing_mode.inline_axis();
    if inherent_size.get_abs(item_inline_axis).is_none()
        && raw_logical_size.inline_size.is_auto()
        && inline_auto_behavior.is_content_based(aspect_ratio.ratio.is_some())
    {
        let fitted_inline_size = fit_content_inline_size(
            tree,
            node,
            ChildLayoutInput::new(
                inherent_size,
                grid_area_size.map(Some),
                parent_writing_mode,
                item_available_size.map(AvailableSpace::Definite),
                SizingMode::ContentSize,
                Line::FALSE,
            )
            .with_definite_dimensions(node_sizing.definite_size)
            .with_inline_auto_behavior(inline_auto_behavior)
            .with_block_auto_behavior(block_auto_behavior),
            item_available_size.get_abs(item_inline_axis),
            item_inline_axis,
        )
        .maybe_clamp(resolved.min_size.get_abs(item_inline_axis), resolved.max_size.get_abs(item_inline_axis))
        .max(padding_border_size.get_abs(item_inline_axis));
        match item_inline_axis {
            AbsoluteAxis::Horizontal => inherent_size.width = Some(fitted_inline_size),
            AbsoluteAxis::Vertical => inherent_size.height = Some(fitted_inline_size),
        }
    }
    let mut min_size = resolved.min_size.or(padding_border_size.map(Some)).maybe_max(padding_border_size);
    let mut max_size = resolved.max_size;
    let mut definite_axes = node_sizing.definite_size.map(|size| size.is_some());

    let mut size_before_ratio = inherent_size;
    if size_before_ratio.width.is_none()
        && !physical_auto_size.horizontal.is_content_based(aspect_ratio.ratio.is_some())
    {
        size_before_ratio.width = Some(grid_area_minus_item_margins_size.width);
        definite_axes.width = true;
    }

    // Reapply aspect ratio after stretch and absolute position width adjustments
    let size_after_ratio = apply_preferred_aspect_ratio(
        size_before_ratio,
        raw_size.map(|dimension| dimension.is_auto()),
        item_writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        aspect_ratio,
        padding_border_size,
    );
    transfer_ratio_definiteness(size_before_ratio, size_after_ratio, &mut definite_axes);
    let Size { width, height } = size_after_ratio;

    let block_size_properties = BlockSizeProperties::new(
        raw_logical_size.block_size,
        raw_logical_min_size.block_size,
        raw_logical_max_size.block_size,
    );
    let content_based_block_size = ContentBasedBlockSize::new(
        block_size_properties,
        aspect_ratio,
        padding_border_size,
        block_auto_behavior,
        item_writing_mode.to_logical(item_available_size.map(AvailableSpace::Definite)).block_size,
        overflow.x.is_scroll_container() || overflow.y.is_scroll_container(),
        None,
    );
    let content_based_block_constraints = resolve_content_based_block_size_constraints(
        tree,
        node,
        ChildLayoutInput::new(
            Size { width, height },
            grid_area_size.map(Some),
            parent_writing_mode,
            item_available_size.map(AvailableSpace::Definite),
            SizingMode::ContentSize,
            Line::FALSE,
        )
        .with_definite_dimensions(definite_dimensions(Size { width, height }, definite_axes))
        .with_block_auto_behavior(block_auto_behavior),
        content_based_block_size,
    );
    let mut resolved_size = Size { width, height };
    content_based_block_constraints.apply_to_block_axis(
        item_writing_mode,
        block_axis_constraints,
        padding_border_size,
        &mut resolved_size,
        &mut min_size,
        &mut max_size,
    );
    let mut size_before_ratio = resolved_size;
    if size_before_ratio.height.is_none() && !physical_auto_size.vertical.is_content_based(aspect_ratio.ratio.is_some())
    {
        size_before_ratio.height = Some(grid_area_minus_item_margins_size.height);
        definite_axes.height = true;
    }
    // Reapply aspect ratio after stretch and absolute position height adjustments
    let size_after_ratio = size_before_ratio.maybe_apply_aspect_ratio_with_box_sizing(
        aspect_ratio,
        BoxSizing::BorderBox,
        padding_border_size,
    );
    transfer_ratio_definiteness(size_before_ratio, size_after_ratio, &mut definite_axes);

    // Clamp size by min and max width/height
    let size = size_after_ratio.maybe_clamp(min_size, max_size);
    let definite_size = definite_dimensions(size, definite_axes);

    let layout_output = tree.perform_child_layout(
        node,
        ChildLayoutInput::new(
            size,
            grid_area_size.map(Option::Some),
            parent_writing_mode,
            item_available_size.map(AvailableSpace::Definite),
            SizingMode::InherentSize,
            Line::FALSE,
        )
        .with_definite_dimensions(definite_size)
        .with_block_auto_behavior(block_auto_behavior),
    );

    // Resolve final size
    let Size { width, height } = size.unwrap_or(layout_output.size).maybe_clamp(min_size, max_size);

    let physical_size = Size { width, height };
    let logical_size = flow.to_logical_size(physical_size);
    let (inline_offset, inline_margin) = align_item_within_area(
        Line {
            start: logical_grid_area_offset.inline_offset,
            end: logical_grid_area_offset.inline_offset + logical_grid_area_size.inline_size,
        },
        alignment_styles.inline,
        logical_size.inline_size,
        Position::Relative,
        Line { start: None, end: None },
        Line { start: logical_margin.inline_start, end: logical_margin.inline_end },
        baseline_shim.inline,
        baseline_group.inline,
    );
    let (block_offset, block_margin) = align_item_within_area(
        Line {
            start: logical_grid_area_offset.block_offset,
            end: logical_grid_area_offset.block_offset + logical_grid_area_size.block_size,
        },
        alignment_styles.block,
        logical_size.block_size,
        Position::Relative,
        Line { start: None, end: None },
        Line { start: logical_margin.block_start, end: logical_margin.block_end },
        baseline_shim.block,
        baseline_group.block,
    );
    let logical_location = LogicalOffset { inline_offset, block_offset };
    let location = converter.to_physical_point(logical_location, physical_size);

    let resolved_margin = flow.writing_direction().to_physical_box_strut(LogicalBoxStrut {
        inline_start: inline_margin.start,
        inline_end: inline_margin.end,
        block_start: block_margin.start,
        block_end: block_margin.end,
    });

    tree.set_unrounded_layout(
        node,
        &Layout {
            order,
            location,
            size: physical_size,
            #[cfg(feature = "content_size")]
            content_size: layout_output.content_size,
            scrollbar_size,
            padding,
            border,
            margin: resolved_margin,
        },
    );

    #[cfg(feature = "content_size")]
    let contribution = {
        // Scrollable overflow is accumulated in logical coordinates so that
        // inline/block-end contributions behave identically in every writing direction.
        let logical_container_border = flow.writing_direction().to_logical_box_strut(container_border);
        let contribution_location = Point {
            x: logical_location.inline_offset - logical_container_border.inline_start,
            y: logical_location.block_offset - logical_container_border.block_start,
        };
        let logical_content_size = flow.to_logical_size(layout_output.content_size);
        let logical_overflow = flow.to_logical_size(Size { width: overflow.x, height: overflow.y });
        let logical_contribution = compute_content_size_contribution(
            contribution_location,
            Size { width: logical_size.inline_size, height: logical_size.block_size },
            Size { width: logical_content_size.inline_size, height: logical_content_size.block_size },
            Point { x: logical_overflow.inline_size, y: logical_overflow.block_size },
        );
        flow.to_physical_size(LogicalSize {
            inline_size: logical_contribution.width,
            block_size: logical_contribution.height,
        })
    };
    #[cfg(not(feature = "content_size"))]
    let contribution = Size::ZERO;

    GridItemPlacement {
        content_size_contribution: contribution,
        block_offset: logical_location.block_offset,
        block_size: logical_size.block_size,
        first_baseline: logical_block_baseline(layout_output.first_baselines, physical_size, flow.writing_direction()),
        last_baseline: logical_block_baseline(layout_output.last_baselines, physical_size, flow.writing_direction()),
    }
}

/// Build the size-independent static-position candidate for an out-of-flow
/// grid child. The actual containing block resolves the selected edge after
/// the child's used size and margins are known.
#[inline]
pub(super) fn static_position_for_grid_area(
    area_offset: LogicalOffset<f32>,
    area_size: LogicalSize<f32>,
    alignment: InBothAbstractAxis<AlignSelf>,
) -> LogicalStaticPosition {
    #[inline(always)]
    fn axis_anchor(start: f32, size: f32, alignment: AlignSelf) -> (f32, StaticPositionEdge) {
        match alignment.keyword() {
            AlignItemsKeyword::Center => (start + size / 2.0, StaticPositionEdge::Center),
            AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd | AlignItemsKeyword::LastBaseline => {
                (start + size, StaticPositionEdge::End)
            }
            AlignItemsKeyword::Start
            | AlignItemsKeyword::Normal
            | AlignItemsKeyword::FlexStart
            | AlignItemsKeyword::Baseline
            | AlignItemsKeyword::Stretch => (start, StaticPositionEdge::Start),
            // Axis-relative values are resolved against the item and container
            // writing directions before this helper is called.
            AlignItemsKeyword::SelfStart
            | AlignItemsKeyword::SelfEnd
            | AlignItemsKeyword::Left
            | AlignItemsKeyword::Right => unreachable!(),
        }
    }

    let (inline_offset, inline_edge) = axis_anchor(area_offset.inline_offset, area_size.inline_size, alignment.inline);
    let (block_offset, block_edge) = axis_anchor(area_offset.block_offset, area_size.block_size, alignment.block);
    LogicalStaticPosition {
        offset: LogicalOffset { inline_offset, block_offset },
        inline_edge,
        block_edge,
        ..LogicalStaticPosition::default()
    }
}

/// Resolve a grid formatting context's size-independent static-position
/// candidate for an out-of-flow child.
///
/// The actual containing block may be an ancestor, so this function deliberately
/// does not size or place the child. Authored grid lines only affect the
/// `grid_area` selected by the caller when this grid is also the containing
/// block.
pub(super) fn out_of_flow_static_position(
    tree: &impl LayoutGridContainer,
    node: NodeId,
    grid_area: Rect<f32>,
    container_alignment_styles: InBothAbstractAxis<Option<AlignItems>>,
    direction: Direction,
    parent_writing_mode: WritingMode,
    container_border_box_size: Size<f32>,
) -> LogicalStaticPosition {
    let flow = GridFlow::new(parent_writing_mode, direction);
    let converter = flow.writing_direction().converter(container_border_box_size);
    let grid_area_size = Size { width: grid_area.right - grid_area.left, height: grid_area.bottom - grid_area.top };
    let logical_grid_area_size = converter.to_logical_size(grid_area_size);
    let logical_grid_area_offset =
        converter.to_logical_point(Point { x: grid_area.left, y: grid_area.top }, grid_area_size);
    let item_writing_mode = tree.get_writing_mode(node);
    let style = tree.get_grid_child_style(node);
    let item_direction = style.direction();
    let inline_self = GridItemStyle::justify_self(&style).map(|align| {
        align.resolve_axis_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.inline_axis(),
        )
    });
    let block_self = GridItemStyle::align_self(&style).map(|align| {
        align.resolve_axis_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.block_axis(),
        )
    });
    let inline_container_alignment = container_alignment_styles.inline.map(|align| {
        align.resolve_axis_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.inline_axis(),
        )
    });
    let block_container_alignment = container_alignment_styles.block.map(|align| {
        align.resolve_axis_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.block_axis(),
        )
    });
    let alignment = InBothAbstractAxis {
        inline: inline_self.or(inline_container_alignment).unwrap_or(AlignSelf::START),
        block: block_self.or(block_container_alignment).unwrap_or(AlignSelf::START),
    };
    static_position_for_grid_area(logical_grid_area_offset, logical_grid_area_size, alignment)
}

/// Align and size a grid item along a single axis
#[allow(clippy::too_many_arguments)]
pub(super) fn align_item_within_area(
    grid_area: Line<f32>,
    alignment_style: AlignSelf,
    resolved_size: f32,
    position: Position,
    inset: Line<Option<f32>>,
    margin: Line<Option<f32>>,
    baseline_shim: f32,
    baseline_group: BaselineGroup,
) -> (f32, Line<f32>) {
    // Calculate grid area dimension in the axis
    let shim = match baseline_group {
        BaselineGroup::Major => Line { start: baseline_shim, end: 0.0 },
        BaselineGroup::Minor => Line { start: 0.0, end: baseline_shim },
    };
    let non_auto_margin =
        Line { start: margin.start.unwrap_or(0.0) + shim.start, end: margin.end.unwrap_or(0.0) + shim.end };
    let grid_area_size = f32_max(grid_area.end - grid_area.start, 0.0);
    let free_space = f32_max(grid_area_size - resolved_size - non_auto_margin.sum(), 0.0);

    // Expand auto margins to fill available space
    let auto_margin_count = margin.start.is_none() as u8 + margin.end.is_none() as u8;
    let auto_margin_size = if auto_margin_count > 0 { free_space / auto_margin_count as f32 } else { 0.0 };
    let resolved_margin = Line {
        start: margin.start.unwrap_or(auto_margin_size) + shim.start,
        end: margin.end.unwrap_or(auto_margin_size) + shim.end,
    };

    let overflows = resolved_size + non_auto_margin.sum() > grid_area_size;
    let alignment_keyword = resolve_self_alignment_safety(alignment_style, overflows);

    // Compute offset in the axis
    let alignment_based_offset = match alignment_keyword {
        AlignItemsKeyword::Normal
        | AlignItemsKeyword::Start
        | AlignItemsKeyword::FlexStart
        | AlignItemsKeyword::Stretch => resolved_margin.start,
        AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd => grid_area_size - resolved_size - resolved_margin.end,
        AlignItemsKeyword::Baseline => match baseline_group {
            BaselineGroup::Major => resolved_margin.start,
            BaselineGroup::Minor => grid_area_size - resolved_size - resolved_margin.end,
        },
        AlignItemsKeyword::LastBaseline => {
            if position == Position::Absolute {
                grid_area_size - resolved_size - resolved_margin.end
            } else {
                match baseline_group {
                    BaselineGroup::Major => resolved_margin.start,
                    BaselineGroup::Minor => grid_area_size - resolved_size - resolved_margin.end,
                }
            }
        }
        AlignItemsKeyword::Center => {
            (grid_area_size - resolved_size + resolved_margin.start - resolved_margin.end) / 2.0
        }
        // Axis-relative values are resolved to Start/End in `align_and_position_item`.
        AlignItemsKeyword::SelfStart
        | AlignItemsKeyword::SelfEnd
        | AlignItemsKeyword::Left
        | AlignItemsKeyword::Right => unreachable!(),
    };

    let offset_within_area = if position == Position::Absolute {
        match (inset.start, inset.end) {
            (Some(start), _) => start + non_auto_margin.start,
            (None, Some(end)) => grid_area_size - end - resolved_size - non_auto_margin.end,
            (None, None) => alignment_based_offset,
        }
    } else {
        alignment_based_offset
    };

    let mut start = grid_area.start + offset_within_area;
    if position == Position::Relative {
        let relative_inset = inset.start.or(inset.end.map(|pos| -pos));
        start += relative_inset.unwrap_or(0.0);
    }

    (start, resolved_margin)
}

#[cfg(test)]
mod tests {
    use super::static_position_for_grid_area;
    use crate::{AlignSelf, InBothAbstractAxis, LogicalOffset, LogicalSize, StaticPositionEdge};

    #[test]
    fn grid_static_position_retains_alignment_edges_before_child_sizing() {
        let candidate = static_position_for_grid_area(
            LogicalOffset { inline_offset: 10.0, block_offset: 20.0 },
            LogicalSize { inline_size: 100.0, block_size: 80.0 },
            InBothAbstractAxis { inline: AlignSelf::CENTER, block: AlignSelf::END },
        );

        assert_eq!(candidate.offset.inline_offset, 60.0);
        assert_eq!(candidate.offset.block_offset, 100.0);
        assert_eq!(candidate.inline_edge, StaticPositionEdge::Center);
        assert_eq!(candidate.block_edge, StaticPositionEdge::End);
    }
}
