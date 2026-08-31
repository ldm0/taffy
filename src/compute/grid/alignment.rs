//! Alignment of tracks and final positioning of items
use super::types::GridTrack;
use crate::compute::common::absolute::{AbsoluteBlockSizeInput, AbsoluteBlockSizeResolver, AbsoluteBoxSizing};
use crate::compute::common::alignment::{
    apply_alignment_fallback, compute_alignment_offset, resolve_self_alignment, resolve_self_alignment_safety,
};
use crate::compute::common::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use crate::compute::common::intrinsic_size::{
    resolve_intrinsic_width_constraints, resolve_ratio_dependent_content_contribution, IntrinsicWidthInput,
};
use crate::geometry::{InBothAbsAxis, Line, Point, Rect, Size};
use crate::style::{
    AlignContent, AlignItems, AlignItemsKeyword, AlignSelf, AvailableSpace, CoreStyle, GridItemStyle, Position,
};
use crate::tree::{
    ChildLayoutInput, Layout, LayoutInput, LayoutPartialTreeExt, NodeId, RunMode, SizingMode, SizingPurpose,
};
use crate::util::sys::f32_max;
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};

#[cfg(feature = "content_size")]
use crate::compute::common::content_size::{compute_content_size_contribution, content_size_contribution_location};
use crate::{
    AutoSizeBehavior, BoxSizing, Direction, LayoutGridContainer, OrthogonalFallback, RequestedAxis, WritingMode,
};

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
    pub(super) block_start: f32,
    /// Used border-box block-size.
    pub(super) block_size: f32,
    /// First baseline relative to the item's border box.
    pub(super) first_baseline: Option<f32>,
    /// Last baseline relative to the item's border box.
    pub(super) last_baseline: Option<f32>,
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
    container_alignment_styles: InBothAbsAxis<Option<AlignItems>>,
    baseline_shim: f32,
    direction: Direction,
    parent_writing_mode: WritingMode,
    container_border_box_width: f32,
    container_border: Rect<f32>,
    container_scrollbar_insets: Rect<f32>,
) -> GridItemPlacement {
    let grid_area_size = Size { width: grid_area.right - grid_area.left, height: grid_area.bottom - grid_area.top };
    let percentage_basis = parent_writing_mode.to_logical(grid_area_size).inline_size;

    let aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let item_writing_mode = tree.get_writing_mode(node);
    let scrollbar_size = tree.get_scrollbar_insets(node).sum_axes();
    let style = tree.get_grid_child_style(node);

    let overflow = style.overflow();
    let item_direction = style.direction();
    let justify_self = style.justify_self().map(|align| {
        align.resolve_self_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            crate::AbsoluteAxis::Horizontal,
        )
    });
    let align_self = style.align_self().map(|align| {
        align.resolve_self_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            crate::AbsoluteAxis::Vertical,
        )
    });
    let container_alignment_styles = InBothAbsAxis {
        horizontal: container_alignment_styles.horizontal.map(|align| {
            align.resolve_self_relative(
                item_writing_mode,
                item_direction,
                parent_writing_mode,
                direction,
                crate::AbsoluteAxis::Horizontal,
            )
        }),
        vertical: container_alignment_styles.vertical.map(|align| {
            align.resolve_self_relative(
                item_writing_mode,
                item_direction,
                parent_writing_mode,
                direction,
                crate::AbsoluteAxis::Vertical,
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
    let is_replaced = style.is_compressible_replaced();
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
        height: grid_area_size.height.maybe_sub(margin.top).maybe_sub(margin.bottom) - baseline_shim,
    };
    let inset_modified_available_size = if position == Position::Absolute {
        Size {
            width: grid_area_minus_item_margins_size.width
                - inset_horizontal.start.unwrap_or(0.0)
                - inset_horizontal.end.unwrap_or(0.0),
            height: grid_area_minus_item_margins_size.height
                - inset_vertical.start.unwrap_or(0.0)
                - inset_vertical.end.unwrap_or(0.0),
        }
    } else {
        grid_area_minus_item_margins_size
    };
    let intrinsic_available_space = AvailableSpace::Definite(f32_max(inset_modified_available_size.width, 0.0));
    let intrinsic_inputs = LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: SizingMode::InherentSize,
        sizing_purpose: SizingPurpose::IntrinsicContribution,
        axis: RequestedAxis::Horizontal,
        inline_auto_behavior: AutoSizeBehavior::FitContent,
        block_auto_behavior: crate::AutoSizeBehavior::FitContent,
        orthogonal_fallback: OrthogonalFallback::Suppress,
        known_dimensions: Size::NONE,
        definite_dimensions: Size::NONE,
        parent_size: grid_area_size.map(Some),
        parent_writing_mode,
        available_space: Size {
            width: intrinsic_available_space,
            height: AvailableSpace::Definite(grid_area_minus_item_margins_size.height),
        },
        vertical_margins_are_collapsible: Line::FALSE,
    };
    let ratio_content_contribution = resolve_ratio_dependent_content_contribution(
        inherent_size,
        min_size,
        max_size,
        aspect_ratio,
        padding_border_size,
        crate::AbsoluteAxis::Horizontal,
        aspect_ratio.is_some()
            && [raw_size.height, raw_min_size.height, raw_max_size.height]
                .into_iter()
                .any(|value| value.may_have_percentage_dependence() || value.is_stretch()),
    );
    let intrinsic = resolve_intrinsic_width_constraints(
        tree,
        node,
        intrinsic_inputs,
        IntrinsicWidthInput {
            preferred: raw_size.width,
            min: raw_min_size.width,
            max: raw_max_size.width,
            available_space: intrinsic_available_space,
            ratio_content_contribution,
        },
    );
    inherent_size.width = inherent_size.width.or(intrinsic.preferred.value);
    min_size.width = min_size.width.or(intrinsic.min.value);
    max_size.width = max_size.width.or(intrinsic.max.value);
    let normal_auto_size = if is_replaced { AutoSizeBehavior::FitContent } else { AutoSizeBehavior::StretchImplicit };
    let resolved_alignment = InBothAbsAxis {
        horizontal: resolve_self_alignment(
            justify_self.or(container_alignment_styles.horizontal).unwrap_or(AlignSelf::NORMAL),
            AlignSelf::START,
            normal_auto_size,
        ),
        vertical: resolve_self_alignment(
            align_self.or(container_alignment_styles.vertical).unwrap_or(AlignSelf::NORMAL),
            AlignSelf::START,
            normal_auto_size,
        ),
    };
    let alignment_styles = InBothAbsAxis {
        horizontal: resolved_alignment.horizontal.position,
        vertical: resolved_alignment.vertical.position,
    };
    let mut auto_size = InBothAbsAxis {
        horizontal: resolved_alignment.horizontal.auto_size,
        vertical: resolved_alignment.vertical.auto_size,
    };
    if position != Position::Absolute {
        // Auto margins take precedence over self-alignment for in-flow grid
        // items. Out-of-flow margins participate in absolute positioning
        // instead and do not alter the item's alignment-derived auto sizing.
        if margin.left.is_none() || margin.right.is_none() {
            auto_size.horizontal = AutoSizeBehavior::FitContent;
        }
        if margin.top.is_none() || margin.bottom.is_none() {
            auto_size.vertical = AutoSizeBehavior::FitContent;
        }
    }
    let (inline_auto_behavior, block_auto_behavior) = match item_writing_mode.inline_axis() {
        crate::AbsoluteAxis::Horizontal => (auto_size.horizontal, auto_size.vertical),
        crate::AbsoluteAxis::Vertical => (auto_size.vertical, auto_size.horizontal),
    };

    let resolved = resolve_size_constraints(SizeConstraintInput {
        size: inherent_size,
        min_size,
        max_size,
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode: item_writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border: padding_border_size,
    });
    let block_size_resolver = (position == Position::Absolute).then(|| {
        AbsoluteBlockSizeResolver::new(AbsoluteBlockSizeInput {
            writing_mode: item_writing_mode,
            size: raw_size,
            min_size: raw_min_size,
            max_size: raw_max_size,
            aspect_ratio,
            padding_border: padding_border_size,
            block_auto_behavior,
            is_scroll_container: overflow.x.is_scroll_container() || overflow.y.is_scroll_container(),
            is_replaced,
            constraint_sources: resolved.block_axis_constraints(item_writing_mode),
        })
    });
    let aspect_ratio_applied = resolved.aspect_ratio_applied;
    inherent_size = resolved.size;
    min_size = resolved.min_size.or(padding_border_size.map(Some)).maybe_max(padding_border_size);
    max_size = resolved.max_size;

    // If node is absolutely positioned and width is not set explicitly, then deduce it
    // from left, right and container_content_box if both are set.
    let width = inherent_size.width.or_else(|| {
        // Apply width derived from both the left and right properties of an absolutely
        // positioned element being set
        if position == Position::Absolute && !auto_size.horizontal.is_content_based(aspect_ratio_applied.width) {
            if let (Some(left), Some(right)) = (inset_horizontal.start, inset_horizontal.end) {
                return Some(f32_max(grid_area_minus_item_margins_size.width - left - right, 0.0));
            }
        }

        // Apply width based on stretch sizing if:
        //  - The alignment resolves auto sizing to stretch
        //  - The node is not absolutely positioned
        //  - The node does not have auto margins in this axis.
        if !auto_size.horizontal.is_content_based(aspect_ratio_applied.width) && position != Position::Absolute {
            return Some(grid_area_minus_item_margins_size.width);
        }

        None
    });

    // Reapply aspect ratio after stretch and absolute position width adjustments
    let Size { width, height } = Size { width, height: inherent_size.height }.maybe_apply_aspect_ratio_with_box_sizing(
        aspect_ratio,
        BoxSizing::BorderBox,
        padding_border_size,
    );

    let height = height.or_else(|| {
        if position == Position::Absolute && !auto_size.vertical.is_content_based(aspect_ratio_applied.height) {
            if let (Some(top), Some(bottom)) = (inset_vertical.start, inset_vertical.end) {
                return Some(f32_max(grid_area_minus_item_margins_size.height - top - bottom, 0.0));
            }
        }

        // Apply height based on stretch sizing if:
        //  - The alignment resolves auto sizing to stretch
        //  - The node is not absolutely positioned
        //  - The node does not have auto margins in this axis.
        if !auto_size.vertical.is_content_based(aspect_ratio_applied.height) && position != Position::Absolute {
            return Some(grid_area_minus_item_margins_size.height);
        }

        None
    });
    // Reapply aspect ratio after stretch and absolute position height adjustments
    let Size { width, height } = Size { width, height }.maybe_apply_aspect_ratio_with_box_sizing(
        aspect_ratio,
        BoxSizing::BorderBox,
        padding_border_size,
    );

    // Clamp size by min and max width/height. Content-dependent block-axis
    // constraints are resolved only now, once the absolute inline size and
    // inset-modified containing block are known.
    let mut used_size = Size { width, height }.maybe_clamp(min_size, max_size);
    if let Some(resolver) = block_size_resolver {
        let sizing = resolver.resolve(
            tree,
            node,
            ChildLayoutInput::new(
                used_size,
                grid_area_size.map(Some),
                parent_writing_mode,
                inset_modified_available_size.map(|size| AvailableSpace::Definite(f32_max(size, 0.0))),
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .without_orthogonal_fallback(),
            AbsoluteBoxSizing { size: used_size, min_size, max_size },
        );
        used_size = sizing.size;
        min_size = sizing.min_size;
        max_size = sizing.max_size;
    }
    let Size { width, height } = used_size;

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
            )
            .without_orthogonal_fallback(),
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
        )
        .without_orthogonal_fallback(),
    );

    // Resolve final size
    let Size { width, height } = size.unwrap_or(layout_output.size).maybe_clamp(min_size, max_size);

    let (x, x_margin) = align_item_within_area(
        Line { start: grid_area.left, end: grid_area.right },
        alignment_styles.horizontal,
        width,
        position,
        inset_horizontal,
        margin.horizontal_components(),
        0.0,
        direction,
    );
    let (y, y_margin) = align_item_within_area(
        Line { start: grid_area.top, end: grid_area.bottom },
        alignment_styles.vertical,
        height,
        position,
        inset_vertical,
        margin.vertical_components(),
        baseline_shim,
        Direction::Ltr,
    );

    let resolved_margin = Rect { left: x_margin.start, right: x_margin.end, top: y_margin.start, bottom: y_margin.end };

    tree.set_unrounded_layout(
        node,
        &Layout {
            order,
            location: Point { x, y },
            size: Size { width, height },
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
        let contribution_location = content_size_contribution_location(
            Point { x, y },
            Size { width, height },
            container_border_box_width,
            container_border,
            container_scrollbar_insets,
            direction,
        );
        compute_content_size_contribution(
            contribution_location,
            Size { width, height },
            layout_output.content_size,
            overflow,
        )
    };
    #[cfg(not(feature = "content_size"))]
    let contribution = Size::ZERO;

    GridItemPlacement {
        content_size_contribution: contribution,
        block_start: y,
        block_size: height,
        first_baseline: layout_output.first_baselines.y,
        last_baseline: layout_output.last_baselines.y,
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
    baseline_shim: f32,
    direction: Direction,
) -> (f32, Line<f32>) {
    // Calculate grid area dimension in the axis
    let non_auto_margin = Line { start: margin.start.unwrap_or(0.0) + baseline_shim, end: margin.end.unwrap_or(0.0) };
    let grid_area_size = f32_max(grid_area.end - grid_area.start, 0.0);
    let free_space = f32_max(grid_area_size - resolved_size - non_auto_margin.sum(), 0.0);

    // Expand auto margins to fill available space
    let auto_margin_count = margin.start.is_none() as u8 + margin.end.is_none() as u8;
    let auto_margin_size = if auto_margin_count > 0 { free_space / auto_margin_count as f32 } else { 0.0 };
    let resolved_margin = Line {
        start: margin.start.unwrap_or(auto_margin_size) + baseline_shim,
        end: margin.end.unwrap_or(auto_margin_size),
    };

    let overflows = resolved_size + non_auto_margin.sum() > grid_area_size;
    let alignment_keyword = resolve_self_alignment_safety(alignment_style, overflows);

    // Compute offset in the axis
    let alignment_based_offset = match alignment_keyword {
        // TODO: Add support for baseline alignment. For now we treat it as "start".
        AlignItemsKeyword::Normal
        | AlignItemsKeyword::Start
        | AlignItemsKeyword::FlexStart
        | AlignItemsKeyword::Baseline
        | AlignItemsKeyword::Stretch => {
            if direction.is_rtl() {
                grid_area_size - resolved_size - resolved_margin.end
            } else {
                resolved_margin.start
            }
        }
        AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd => {
            if direction.is_rtl() {
                resolved_margin.start
            } else {
                grid_area_size - resolved_size - resolved_margin.end
            }
        }
        AlignItemsKeyword::Center => {
            (grid_area_size - resolved_size + resolved_margin.start - resolved_margin.end) / 2.0
        }
        // SelfStart/SelfEnd are resolved to Start/End against the item's own direction in
        // `align_and_position_item`.
        AlignItemsKeyword::SelfStart | AlignItemsKeyword::SelfEnd => unreachable!(),
    };

    let offset_within_area = if position == Position::Absolute {
        match (inset.start, inset.end) {
            (Some(start), Some(end)) => {
                if direction.is_rtl() {
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
        let relative_inset = if direction.is_rtl() {
            inset.end.map(|pos| -pos).or(inset.start)
        } else {
            inset.start.or(inset.end.map(|pos| -pos))
        };
        start += relative_inset.unwrap_or(0.0);
    }

    (start, resolved_margin)
}
