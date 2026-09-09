//! Alignment of tracks and final positioning of items
use super::types::GridTrack;
use crate::compute::common::alignment::{
    apply_alignment_fallback, compute_alignment_offset, resolve_self_alignment_safety,
};
use crate::compute::common::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use crate::compute::common::baseline::BaselineContext;
use crate::compute::common::intrinsic_size::{resolve_intrinsic_axis_constraints, IntrinsicAxisInput};
use crate::geometry::{InBothLogicalAxes, Line, LogicalSize, Point, Rect, Size};
use crate::style::{
    AlignContent, AlignItems, AlignItemsKeyword, AlignSelf, AvailableSpace, CoreStyle, GridItemStyle, Overflow,
    Position,
};
use crate::tree::{ChildLayoutInput, Layout, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::sys::f32_max;
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};

#[cfg(feature = "content_size")]
use crate::compute::common::content_size::compute_logical_content_size_contribution;
use crate::{AutoSizeBehavior, BaselineType, BoxSizing, Direction, LayoutGridContainer, WritingDirection, WritingMode};

/// Container geometry shared by every final item publication.
#[derive(Clone, Copy)]
pub(super) struct GridPlacementContext {
    /// Writing mode and direction used to project logical grid areas.
    pub(super) flow: WritingDirection,
    /// Physical border-box size of the grid container.
    pub(super) outer_size: Size<f32>,
    /// Physical border and scrollbar insets outside the scrollable content.
    pub(super) border_scrollbar: Rect<f32>,
    /// Baseline family used when an item has no compatible real baseline.
    pub(super) baseline_type: BaselineType,
}

/// Final block-axis geometry and baseline data for a positioned grid item.
///
/// Baselines belong to the final child layout, not to the temporary intrinsic
/// measurement used to calculate baseline shims. Keeping both sets here lets
/// the grid container propagate distinct first and last baselines to its own
/// parent formatting context.
pub(super) struct GridItemPlacement {
    /// Contribution of the positioned item to the grid's scrollable content.
    pub(super) content_size_contribution: LogicalSize<f32>,
    /// Physical origin along the container's block axis, without relative offsets.
    pub(super) block_axis_origin: f32,
    /// Fallback baseline in the container's font and writing-mode context.
    pub(super) synthesized_baseline: f32,
    /// First baseline relative to the item's border box.
    pub(super) first_baseline: Option<f32>,
    /// Last baseline relative to the item's border box.
    pub(super) last_baseline: Option<f32>,
}

/// Final child fragment awaiting alignment with the other items in its grid.
/// Only after all sharing groups have been resolved is its layout published.
pub(super) struct GridItemLayout {
    /// Physical border-box layout, not yet published to the tree.
    pub(super) layout: Layout,
    /// Physical edges of the containing grid area in container coordinates.
    pub(super) grid_area: Rect<f32>,
    /// First baseline coordinates relative to the item's border-box origin.
    pub(super) first_baselines: Point<Option<f32>>,
    /// Last baseline coordinates relative to the item's border-box origin.
    pub(super) last_baselines: Point<Option<f32>>,
    /// Relative positioning moves paint, but not a parent's shared baseline.
    pub(super) relative_offset: Point<f32>,
    /// Overflow styles used when computing the item's scrollable contribution.
    overflow: Point<Overflow>,
}

impl GridItemLayout {
    /// Publish the aligned fragment and return its contribution to its parent.
    pub(super) fn place(
        mut self,
        tree: &mut impl LayoutGridContainer,
        node: NodeId,
        container: GridPlacementContext,
    ) -> GridItemPlacement {
        if let Some(in_flow) = self.layout.in_flow.as_mut() {
            in_flow.location = Point {
                x: self.layout.location.x - self.relative_offset.x,
                y: self.layout.location.y - self.relative_offset.y,
            };
        }
        tree.set_unrounded_layout(node, &self.layout);
        let mode = container.flow.mode;
        #[cfg(feature = "content_size")]
        let contribution = {
            let mut origin =
                container.flow.converter(container.outer_size).to_logical_point(self.layout.location, self.layout.size);
            let inset = container.flow.to_logical_box_strut(container.border_scrollbar);
            origin.inline_offset -= inset.inline_start;
            origin.block_offset -= inset.block_start;
            compute_logical_content_size_contribution(
                origin,
                mode.to_logical(self.layout.size),
                mode.to_logical(self.layout.content_size),
                mode.to_logical(Size { width: self.overflow.x, height: self.overflow.y }),
            )
        };
        #[cfg(not(feature = "content_size"))]
        let contribution = {
            let _ = (container.outer_size, container.border_scrollbar, self.overflow);
            LogicalSize::ZERO
        };
        let baseline_context = BaselineContext { writing_mode: mode, baseline_type: container.baseline_type };
        let child_mode = tree.get_writing_mode(node);
        GridItemPlacement {
            content_size_contribution: contribution,
            block_axis_origin: match mode.block_axis() {
                crate::AbsoluteAxis::Horizontal => self.layout.location.x - self.relative_offset.x,
                crate::AbsoluteAxis::Vertical => self.layout.location.y - self.relative_offset.y,
            },
            synthesized_baseline: baseline_context.resolve(Point::NONE, child_mode, self.layout.size, false),
            first_baseline: baseline_context.real_baseline(self.first_baselines, child_mode),
            last_baseline: baseline_context.real_baseline(self.last_baselines, child_mode),
        }
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

/// Lay out a grid item at its final used size. Baseline group alignment occurs
/// after all the sibling fragments exist, before `GridItemLayout::place`.
pub(super) fn layout_item(
    tree: &mut impl LayoutGridContainer,
    node: NodeId,
    order: u32,
    grid_area: Rect<f32>,
    container_alignment_styles: InBothLogicalAxes<Option<AlignItems>>,
    direction: Direction,
    parent_writing_mode: WritingMode,
) -> GridItemLayout {
    let grid_area_size = Size { width: grid_area.right - grid_area.left, height: grid_area.bottom - grid_area.top };
    let percentage_basis = parent_writing_mode.to_logical(grid_area_size).inline_size;

    let aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let scrollbar_size = tree.get_scrollbar_insets(node).sum_axes();
    let item_writing_mode = tree.get_writing_mode(node);
    let style = tree.get_grid_child_style(node);

    let overflow = style.overflow();
    let item_direction = style.direction();
    let justify_self = style.justify_self().map(|align| {
        align.resolve_self_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.inline_axis(),
        )
    });
    let align_self = style.align_self().map(|align| {
        align.resolve_self_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.block_axis(),
        )
    });
    let container_alignment_styles = InBothLogicalAxes {
        inline: container_alignment_styles.inline.map(|align| {
            align.resolve_self_relative(
                item_writing_mode,
                item_direction,
                parent_writing_mode,
                direction,
                parent_writing_mode.inline_axis(),
            )
        }),
        block: container_alignment_styles.block.map(|align| {
            align.resolve_self_relative(
                item_writing_mode,
                item_direction,
                parent_writing_mode,
                direction,
                parent_writing_mode.block_axis(),
            )
        }),
    };

    let position = style.position();
    let inset_horizontal = style
        .inset()
        .horizontal_components()
        .map(|size| size.resolve_to_option(grid_area_size.width, |val, basis| tree.calc(val, basis)));
    let inset_vertical = style
        .inset()
        .vertical_components()
        .map(|size| size.resolve_to_option(grid_area_size.height, |val, basis| tree.calc(val, basis)));
    let padding =
        style.padding().map(|p| p.resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis)));
    let border = style.border().map(|p| p.resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis)));
    let padding_border_size = (padding + border).sum_axes();

    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    let raw_size = style.size();
    let raw_min_size = style.min_size();
    let raw_max_size = style.max_size();
    let mut inherent_size =
        raw_size.maybe_resolve(grid_area_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);
    let mut min_size =
        raw_min_size.maybe_resolve(grid_area_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);
    let mut max_size =
        raw_max_size.maybe_resolve(grid_area_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);

    let margin =
        style.margin().map(|margin| margin.resolve_to_option(percentage_basis, |val, basis| tree.calc(val, basis)));

    drop(style);

    let grid_area_minus_item_margins_size = Size {
        width: grid_area_size.width.maybe_sub(margin.left).maybe_sub(margin.right),
        height: grid_area_size.height.maybe_sub(margin.top).maybe_sub(margin.bottom),
    };
    // Resolve intrinsic keywords on the item's physical axes. A vertical
    // child's inline-size is height; its grid area remains the percentage basis.
    for axis in [item_writing_mode.inline_axis(), item_writing_mode.block_axis()] {
        let insets = match axis {
            crate::AbsoluteAxis::Horizontal => inset_horizontal,
            crate::AbsoluteAxis::Vertical => inset_vertical,
        };
        let mut available = grid_area_minus_item_margins_size.get_abs(axis);
        if position == Position::Absolute {
            available -= insets.start.unwrap_or(0.0) + insets.end.unwrap_or(0.0);
        }
        let intrinsic = resolve_intrinsic_axis_constraints(
            tree,
            node,
            ChildLayoutInput::new(
                Size::NONE,
                grid_area_size.map(Some),
                parent_writing_mode,
                grid_area_minus_item_margins_size.map(AvailableSpace::Definite),
                SizingMode::InherentSize,
                Line::FALSE,
            ),
            IntrinsicAxisInput {
                preferred: raw_size.get_abs(axis),
                min: raw_min_size.get_abs(axis),
                max: raw_max_size.get_abs(axis),
                available_space: AvailableSpace::Definite(available.max(0.0)),
                axis,
            },
        );
        match axis {
            crate::AbsoluteAxis::Horizontal => {
                inherent_size.width = inherent_size.width.or(intrinsic.preferred);
                min_size.width = min_size.width.or(intrinsic.min);
                max_size.width = max_size.width.or(intrinsic.max);
            }
            crate::AbsoluteAxis::Vertical => {
                inherent_size.height = inherent_size.height.or(intrinsic.preferred);
                min_size.height = min_size.height.or(intrinsic.min);
                max_size.height = max_size.height.or(intrinsic.max);
            }
        }
    }
    let resolved = resolve_size_constraints(SizeConstraintInput {
        size: inherent_size,
        min_size,
        max_size,
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode: item_writing_mode,
        block_auto_behavior: AutoSizeBehavior::FitContent,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border: padding_border_size,
    });
    inherent_size = resolved.size;
    min_size = resolved.min_size.or(padding_border_size.map(Some)).maybe_max(padding_border_size);
    max_size = resolved.max_size;

    // Resolve default alignment styles if they are set on neither the parent or the node itself
    // Note: if the child has a preferred aspect ratio but neither width or height are set, then the width is stretched
    // and the then height is calculated from the width according the aspect ratio
    // See: https://www.w3.org/TR/css-grid-1/#grid-item-sizing
    let alignment_styles = InBothLogicalAxes {
        inline: justify_self.or(container_alignment_styles.inline).unwrap_or_else(|| {
            if parent_writing_mode.to_logical(inherent_size).inline_size.is_some() {
                AlignSelf::START
            } else {
                AlignSelf::STRETCH
            }
        }),
        block: align_self.or(container_alignment_styles.block).unwrap_or_else(|| {
            if parent_writing_mode.to_logical(inherent_size).block_size.is_some() || aspect_ratio.is_some() {
                AlignSelf::START
            } else {
                AlignSelf::STRETCH
            }
        }),
    };

    let available_logical_size = parent_writing_mode.to_logical(grid_area_minus_item_margins_size);
    let axis_size = |axis| {
        let physical_axis = parent_writing_mode.physical_axis(axis);
        let (inset, margin) = match physical_axis {
            crate::AbsoluteAxis::Horizontal => (inset_horizontal, margin.horizontal_components()),
            crate::AbsoluteAxis::Vertical => (inset_vertical, margin.vertical_components()),
        };
        if position == Position::Absolute {
            return match (inset.start, inset.end) {
                (Some(start), Some(end)) => Some((available_logical_size.get(axis) - start - end).max(0.0)),
                _ => None,
            };
        }
        (margin.start.is_some() && margin.end.is_some() && alignment_styles.get(axis) == AlignSelf::STRETCH)
            .then_some(available_logical_size.get(axis))
    };
    // Inline stretch is resolved first, then aspect-ratio transfer, then
    // block stretch. The tree/ratio boundary always retains physical sizes.
    let mut logical_size = parent_writing_mode.to_logical(inherent_size);
    logical_size.inline_size = logical_size.inline_size.or_else(|| axis_size(crate::geometry::AbstractAxis::Inline));
    let physical_size = parent_writing_mode.to_physical(logical_size).maybe_apply_aspect_ratio_with_box_sizing(
        aspect_ratio,
        BoxSizing::BorderBox,
        padding_border_size,
    );
    logical_size = parent_writing_mode.to_logical(physical_size);
    logical_size.block_size = logical_size.block_size.or_else(|| axis_size(crate::geometry::AbstractAxis::Block));
    let Size { width, height } = parent_writing_mode
        .to_physical(logical_size)
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border_size)
        .maybe_clamp(min_size, max_size);

    // Layout node
    let size = if position == Position::Absolute && (width.is_none() || height.is_none()) {
        tree.measure_child_size_both(
            node,
            ChildLayoutInput::new(
                Size { width, height },
                grid_area_size.map(Option::Some),
                parent_writing_mode,
                grid_area_minus_item_margins_size.map(AvailableSpace::Definite),
                SizingMode::InherentSize,
                Line::FALSE,
            ),
        )
        .map(Some)
    } else {
        Size { width, height }
    };

    let layout_output = tree.perform_child_layout(
        node,
        ChildLayoutInput::new(
            size,
            grid_area_size.map(Option::Some),
            parent_writing_mode,
            grid_area_minus_item_margins_size.map(AvailableSpace::Definite),
            SizingMode::InherentSize,
            Line::FALSE,
        ),
    );

    // Resolve final size
    let Size { width, height } = size.unwrap_or(layout_output.size).maybe_clamp(min_size, max_size);

    let physical_alignment = parent_writing_mode
        .to_physical(LogicalSize { inline_size: alignment_styles.inline, block_size: alignment_styles.block });
    let horizontal_reversed = parent_writing_mode.is_axis_flow_reversed(crate::AbsoluteAxis::Horizontal, direction);
    let vertical_reversed = parent_writing_mode.is_axis_flow_reversed(crate::AbsoluteAxis::Vertical, direction);
    let (x, x_margin) = align_item_within_area(
        Line { start: grid_area.left, end: grid_area.right },
        physical_alignment.width,
        width,
        position,
        inset_horizontal,
        margin.horizontal_components(),
        horizontal_reversed,
    );
    let (y, y_margin) = align_item_within_area(
        Line { start: grid_area.top, end: grid_area.bottom },
        physical_alignment.height,
        height,
        position,
        inset_vertical,
        margin.vertical_components(),
        vertical_reversed,
    );

    let resolved_margin = Rect { left: x_margin.start, right: x_margin.end, top: y_margin.start, bottom: y_margin.end };

    GridItemLayout {
        layout: Layout {
            order,
            location: Point { x, y },
            in_flow: (position != Position::Absolute)
                .then_some(crate::InFlowLayout { location: Point::ZERO, margin: resolved_margin }),
            size: Size { width, height },
            #[cfg(feature = "content_size")]
            content_size: layout_output.content_size,
            scrollbar_size,
            padding,
            border,
            margin: resolved_margin,
        },
        grid_area,
        first_baselines: layout_output.first_baselines,
        last_baselines: layout_output.last_baselines,
        relative_offset: if position == Position::Relative {
            Point {
                x: if horizontal_reversed {
                    inset_horizontal.end.map(|value| -value).or(inset_horizontal.start)
                } else {
                    inset_horizontal.start.or(inset_horizontal.end.map(|value| -value))
                }
                .unwrap_or(0.0),
                y: if vertical_reversed {
                    inset_vertical.end.map(|value| -value).or(inset_vertical.start)
                } else {
                    inset_vertical.start.or(inset_vertical.end.map(|value| -value))
                }
                .unwrap_or(0.0),
            }
        } else {
            Point::ZERO
        },
        overflow,
    }
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
    axis_start_reversed: bool,
) -> (f32, Line<f32>) {
    // Calculate grid area dimension in the axis
    let non_auto_margin = Line { start: margin.start.unwrap_or(0.0), end: margin.end.unwrap_or(0.0) };
    let grid_area_size = f32_max(grid_area.end - grid_area.start, 0.0);
    let free_space = f32_max(grid_area_size - resolved_size - non_auto_margin.sum(), 0.0);

    // Expand auto margins to fill available space
    let auto_margin_count = margin.start.is_none() as u8 + margin.end.is_none() as u8;
    let auto_margin_size = if auto_margin_count > 0 { free_space / auto_margin_count as f32 } else { 0.0 };
    let resolved_margin =
        Line { start: margin.start.unwrap_or(auto_margin_size), end: margin.end.unwrap_or(auto_margin_size) };

    let overflows = resolved_size + non_auto_margin.sum() > grid_area_size;
    let alignment_keyword = resolve_self_alignment_safety(alignment_style, overflows);

    // Compute offset in the axis
    let alignment_based_offset = match alignment_keyword {
        // Baseline group alignment is resolved from the final sibling
        // fragments. This is the fallback used by nonparticipating items.
        AlignItemsKeyword::Start
        | AlignItemsKeyword::FlexStart
        | AlignItemsKeyword::Baseline
        | AlignItemsKeyword::Stretch => {
            if axis_start_reversed {
                grid_area_size - resolved_size - resolved_margin.end
            } else {
                resolved_margin.start
            }
        }
        AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd | AlignItemsKeyword::LastBaseline => {
            if axis_start_reversed {
                resolved_margin.start
            } else {
                grid_area_size - resolved_size - resolved_margin.end
            }
        }
        AlignItemsKeyword::Center => {
            (grid_area_size - resolved_size + resolved_margin.start - resolved_margin.end) / 2.0
        }
        // SelfStart/SelfEnd are resolved to Start/End against the item's own direction in
        // `layout_item`.
        AlignItemsKeyword::SelfStart | AlignItemsKeyword::SelfEnd => unreachable!(),
    };

    let offset_within_area = if position == Position::Absolute {
        match (inset.start, inset.end) {
            (Some(start), Some(end)) => {
                if axis_start_reversed {
                    grid_area_size - end - resolved_size - non_auto_margin.end
                } else {
                    start + non_auto_margin.start
                }
            }
            (Some(start), None) => start + non_auto_margin.start,
            (None, Some(end)) => grid_area_size - end - resolved_size - non_auto_margin.end,
            (None, None) => alignment_based_offset,
        }
    } else {
        alignment_based_offset
    };

    let mut start = grid_area.start + offset_within_area;
    if position == Position::Relative {
        let relative_inset = if axis_start_reversed {
            inset.end.map(|pos| -pos).or(inset.start)
        } else {
            inset.start.or(inset.end.map(|pos| -pos))
        };
        start += relative_inset.unwrap_or(0.0);
    }

    (start, resolved_margin)
}
