//! Alignment of tracks and final positioning of items
use super::types::{GridBaselineAlignment, GridTrack};
use crate::compute::common::absolute::{
    AbsoluteBlockSizeInput, AbsoluteBlockSizeResolver, AbsoluteBoxSizing, InsetModifiedContainingBlock,
};
use crate::compute::common::alignment::{
    apply_alignment_fallback, compute_alignment_offset, resolve_self_alignment, resolve_self_alignment_safety,
};
use crate::compute::common::aspect_ratio::{
    resolve_formatting_context_size, resolve_size_constraints, FormattingContextSizeInput, SizeConstraintInput,
    TransferredSizesMode,
};
use crate::compute::common::baseline::{
    fragment_logical_block_baseline, fragment_logical_block_baseline_or_synthesize, BaselineGroup, FontBaseline,
};
use crate::compute::common::intrinsic_size::{
    resolve_intrinsic_width_constraints, resolve_ratio_dependent_intrinsic_sizing, IntrinsicWidthInput,
    RatioDependentAutomaticMinimum,
};
use crate::compute::common::used_size::StretchSizeProperties;
use crate::geometry::{
    AbstractAxis, InBothAbstractAxis, Line, LogicalBoxStrut, LogicalOffset, LogicalSize, Point, Rect, Size,
};
use crate::style::{
    AlignContent, AlignItems, AlignItemsKeyword, AlignSelf, AvailableSpace, CoreStyle, GridItemStyle, Position,
};
use crate::tree::{
    ChildLayoutInput, Layout, LayoutInput, LayoutPartialTreeExt, NodeId, RunMode, SizingMode, SizingPurpose,
};
use crate::util::sys::f32_max;
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};

#[cfg(feature = "content_size")]
use crate::compute::common::content_size::compute_content_size_contribution;
use crate::{
    AutoSizeBehavior, BoxSizing, Direction, LayoutGridContainer, OrthogonalFallback, RequestedAxis, WritingDirection,
    WritingMode,
};

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
    /// Normal-flow block-start position used for baseline propagation. Relative
    /// positioning moves the painted fragment, but not the baseline it contributes
    /// to its parent formatting context.
    pub(super) baseline_block_offset: f32,
    /// Used border-box block-size.
    pub(super) block_size: f32,
    /// First baseline relative to the item's border box.
    pub(super) first_baseline: Option<f32>,
    /// Last baseline relative to the item's border box.
    pub(super) last_baseline: Option<f32>,
}

/// Final placement in one Grid axis before and after relative positioning.
struct AlignedAxisPlacement {
    /// Used fragment position, including relative positioning.
    offset: f32,
    /// Position established by Grid alignment, before relative positioning.
    normal_flow_offset: f32,
    /// Resolved used margins in this axis.
    margin: Line<f32>,
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
    baseline_alignment: InBothAbstractAxis<Option<GridBaselineAlignment>>,
    baseline_fallback: InBothAbstractAxis<Option<AlignSelf>>,
    direction: Direction,
    parent_writing_mode: WritingMode,
    container_border_box_size: Size<f32>,
    container_border: Rect<f32>,
    container_scrollbar_insets: Rect<f32>,
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
    let scrollbar_size = tree.get_scrollbar_insets(node).sum_axes();
    let style = tree.get_grid_child_style(node);

    let overflow = style.overflow();
    let item_direction = style.direction();
    let inline_self = style.justify_self().map(|align| {
        align.resolve_self_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.inline_axis(),
        )
    });
    let block_self = style.align_self().map(|align| {
        align.resolve_self_relative(
            item_writing_mode,
            item_direction,
            parent_writing_mode,
            direction,
            parent_writing_mode.block_axis(),
        )
    });
    let logical_container_alignment = InBothAbstractAxis {
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
    let logical_inset = flow.writing_direction().to_logical_box_strut(Rect {
        left: inset_horizontal.start,
        right: inset_horizontal.end,
        top: inset_vertical.start,
        bottom: inset_vertical.end,
    });
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
    let logical_margin = flow.writing_direction().to_logical_box_strut(margin);

    drop(style);

    let logical_grid_area_minus_item_margins_size = LogicalSize {
        inline_size: logical_grid_area_size
            .inline_size
            .maybe_sub(logical_margin.inline_start)
            .maybe_sub(logical_margin.inline_end),
        block_size: logical_grid_area_size
            .block_size
            .maybe_sub(logical_margin.block_start)
            .maybe_sub(logical_margin.block_end),
    };
    let grid_area_minus_item_margins_size = flow.to_physical_size(logical_grid_area_minus_item_margins_size);
    let non_auto_margin = margin.map(|value| value.unwrap_or(0.0));
    let absolute_imcb = (position == Position::Absolute).then(|| {
        InsetModifiedContainingBlock::new(
            grid_area_size,
            Rect {
                left: inset_horizontal.start,
                right: inset_horizontal.end,
                top: inset_vertical.start,
                bottom: inset_vertical.end,
            },
            grid_area_size,
            margin,
        )
    });
    let inset_modified_available_size = absolute_imcb
        .map(InsetModifiedContainingBlock::stretch_border_box_opportunity)
        .unwrap_or(grid_area_minus_item_margins_size);
    let child_available_size = absolute_imcb
        .map(InsetModifiedContainingBlock::margin_box_opportunity)
        .unwrap_or(inset_modified_available_size + non_auto_margin.sum_axes());
    if let Some(imcb) = absolute_imcb {
        let authored_stretch = StretchSizeProperties::new(raw_size, raw_min_size, raw_max_size)
            .resolve(imcb.authored_stretch_available_space(), padding_border_size);
        inherent_size = inherent_size.or(authored_stretch.preferred);
        min_size = min_size.or(authored_stretch.min);
        max_size = max_size.or(authored_stretch.max);
    }
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
        available_space: child_available_size.map(|size| AvailableSpace::Definite(f32_max(size, 0.0))),
        ignored_margins_for_stretch: Rect::default(),
        vertical_margins_are_collapsible: Line::FALSE,
    };
    let ratio_dependent_sizing = resolve_ratio_dependent_intrinsic_sizing(
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
            ratio_dependent_sizing,
        },
    );
    inherent_size.width = inherent_size.width.or(intrinsic.preferred.value);
    min_size.width = min_size.width.or(intrinsic.min.value);
    max_size.width = max_size.width.or(intrinsic.max.value);
    let normal_auto_size = if is_replaced { AutoSizeBehavior::FitContent } else { AutoSizeBehavior::StretchImplicit };
    let resolved_alignment = InBothAbstractAxis {
        inline: resolve_self_alignment(
            inline_self.or(logical_container_alignment.inline).unwrap_or(AlignSelf::NORMAL),
            AlignSelf::START,
            normal_auto_size,
        ),
        block: resolve_self_alignment(
            block_self.or(logical_container_alignment.block).unwrap_or(AlignSelf::NORMAL),
            AlignSelf::START,
            normal_auto_size,
        ),
    };
    let mut alignment_styles =
        InBothAbstractAxis { inline: resolved_alignment.inline.position, block: resolved_alignment.block.position };
    if let Some(fallback) = baseline_fallback.inline {
        alignment_styles.inline = fallback;
    }
    if let Some(fallback) = baseline_fallback.block {
        alignment_styles.block = fallback;
    }
    if position == Position::Absolute {
        for alignment in [&mut alignment_styles.inline, &mut alignment_styles.block] {
            if alignment.is_baseline() {
                *alignment = if alignment.is_last_baseline() { AlignSelf::END } else { AlignSelf::START };
            }
        }
    }
    let mut logical_auto_size =
        InBothAbstractAxis { inline: resolved_alignment.inline.auto_size, block: resolved_alignment.block.auto_size };
    if position != Position::Absolute {
        // Auto margins take precedence over self-alignment for in-flow grid
        // items. Out-of-flow margins participate in absolute positioning
        // instead and do not alter the item's alignment-derived auto sizing.
        if logical_margin.inline_start.is_none() || logical_margin.inline_end.is_none() {
            logical_auto_size.inline = AutoSizeBehavior::FitContent;
        }
        if logical_margin.block_start.is_none() || logical_margin.block_end.is_none() {
            logical_auto_size.block = AutoSizeBehavior::FitContent;
        }
    }
    let auto_size = flow.to_physical_axes(logical_auto_size);
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
    let inline_automatic_minimum = if position == Position::Absolute {
        RatioDependentAutomaticMinimum::new(
            resolved.axis_constraints(crate::AbsoluteAxis::Horizontal),
            resolved.aspect_ratio_applied.width || intrinsic.preferred.applied_aspect_ratio,
            raw_min_size.width,
            tree.is_scroll_container_for_automatic_minimum(node),
            is_replaced,
        )
    } else {
        None
    };
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
    inherent_size = resolved.size;
    min_size = resolved.min_size.or(padding_border_size.map(Some)).maybe_max(padding_border_size);
    max_size = resolved.max_size;

    let stretch_size = if let Some(imcb) = absolute_imcb {
        imcb.implicit_auto_stretch_size()
    } else {
        grid_area_minus_item_margins_size.map(Some)
    };
    let Size { width, height } = resolve_formatting_context_size(FormattingContextSizeInput {
        size: inherent_size,
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode: item_writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        stretch_size,
        aspect_ratio,
        padding_border: padding_border_size,
    });

    // Clamp size by min and max width/height. Content-dependent block-axis
    // constraints are resolved only now, once the absolute inline size and
    // inset-modified containing block are known.
    let mut used_size = Size { width, height }.maybe_clamp(min_size, max_size);
    let resolved_sizing = AbsoluteBoxSizing { size: used_size, min_size, max_size }.resolve_ratio_automatic_minimum(
        tree,
        node,
        ChildLayoutInput::new(
            used_size,
            grid_area_size.map(Some),
            parent_writing_mode,
            child_available_size.map(|size| AvailableSpace::Definite(f32_max(size, 0.0))),
            SizingMode::InherentSize,
            Line::FALSE,
        )
        .without_orthogonal_fallback(),
        crate::AbsoluteAxis::Horizontal,
        inline_automatic_minimum,
    );
    used_size = resolved_sizing.size;
    min_size = resolved_sizing.min_size;
    max_size = resolved_sizing.max_size;
    if let Some(resolver) = block_size_resolver {
        let sizing = resolver.resolve(
            tree,
            node,
            ChildLayoutInput::new(
                used_size,
                grid_area_size.map(Some),
                parent_writing_mode,
                child_available_size.map(|size| AvailableSpace::Definite(f32_max(size, 0.0))),
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
                child_available_size.map(|size| AvailableSpace::Definite(f32_max(size, 0.0))),
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
            child_available_size.map(|size| AvailableSpace::Definite(f32_max(size, 0.0))),
            SizingMode::InherentSize,
            Line::FALSE,
        )
        .without_orthogonal_fallback(),
    );

    // Resolve final size
    let Size { width, height } = size.unwrap_or(layout_output.size).maybe_clamp(min_size, max_size);

    let physical_size = Size { width, height };
    let logical_size = flow.to_logical_size(physical_size);
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let font_baseline = FontBaseline::for_writing_mode(parent_writing_mode);
    let resolve_baseline_offset = |axis: AbstractAxis, alignment: AlignSelf, input: Option<GridBaselineAlignment>| {
        let input = input?;
        debug_assert!(alignment.is_baseline());

        let baseline_writing_direction = WritingDirection::new(input.writing_mode, Direction::Ltr);
        let fragment_baselines =
            if alignment.is_last_baseline() { layout_output.last_baselines } else { layout_output.first_baselines };
        let baseline_block_size = input.writing_mode.to_logical(physical_size).block_size;
        let baseline_from_start = fragment_logical_block_baseline_or_synthesize(
            fragment_baselines,
            physical_size,
            item_writing_mode,
            baseline_writing_direction,
            font_baseline,
        );
        let baseline_from_start =
            if is_scroll_container { baseline_from_start.clamp(0.0, baseline_block_size) } else { baseline_from_start };
        let item_baseline =
            if alignment.is_last_baseline() { baseline_block_size - baseline_from_start } else { baseline_from_start };
        let baseline_delta = input.track_baseline - item_baseline;

        Some(match input.group {
            BaselineGroup::Major => baseline_delta,
            BaselineGroup::Minor => logical_grid_area_size.get(axis) - baseline_delta - logical_size.get(axis),
        })
    };
    let baseline_offset = InBothAbstractAxis {
        inline: resolve_baseline_offset(AbstractAxis::Inline, alignment_styles.inline, baseline_alignment.inline),
        block: resolve_baseline_offset(AbstractAxis::Block, alignment_styles.block, baseline_alignment.block),
    };
    let inline_placement = align_item_within_area(
        Line {
            start: logical_grid_area_offset.inline_offset,
            end: logical_grid_area_offset.inline_offset + logical_grid_area_size.inline_size,
        },
        alignment_styles.inline,
        logical_size.inline_size,
        position,
        Line { start: logical_inset.inline_start, end: logical_inset.inline_end },
        Line { start: logical_margin.inline_start, end: logical_margin.inline_end },
        baseline_offset.inline,
    );
    let block_placement = align_item_within_area(
        Line {
            start: logical_grid_area_offset.block_offset,
            end: logical_grid_area_offset.block_offset + logical_grid_area_size.block_size,
        },
        alignment_styles.block,
        logical_size.block_size,
        position,
        Line { start: logical_inset.block_start, end: logical_inset.block_end },
        Line { start: logical_margin.block_start, end: logical_margin.block_end },
        baseline_offset.block,
    );
    let logical_location =
        LogicalOffset { inline_offset: inline_placement.offset, block_offset: block_placement.offset };
    let location = converter.to_physical_point(logical_location, physical_size);

    let resolved_margin = flow.writing_direction().to_physical_box_strut(LogicalBoxStrut {
        inline_start: inline_placement.margin.start,
        inline_end: inline_placement.margin.end,
        block_start: block_placement.margin.start,
        block_end: block_placement.margin.end,
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
        let logical_container_inset =
            flow.writing_direction().to_logical_box_strut(container_border + container_scrollbar_insets);
        let contribution_location = Point {
            x: logical_location.inline_offset - logical_container_inset.inline_start,
            y: logical_location.block_offset - logical_container_inset.block_start,
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

    let resolve_baseline = |baselines| {
        fragment_logical_block_baseline(baselines, physical_size, item_writing_mode, flow.writing_direction()).map(
            |baseline| {
                if overflow.x.is_scroll_container() || overflow.y.is_scroll_container() {
                    baseline.clamp(0.0, logical_size.block_size)
                } else {
                    baseline
                }
            },
        )
    };
    GridItemPlacement {
        content_size_contribution: contribution,
        baseline_block_offset: block_placement.normal_flow_offset,
        block_size: logical_size.block_size,
        first_baseline: resolve_baseline(layout_output.first_baselines),
        last_baseline: resolve_baseline(layout_output.last_baselines),
    }
}

/// Align and size a grid item along a single axis
#[allow(clippy::too_many_arguments)]
fn align_item_within_area(
    grid_area: Line<f32>,
    alignment_style: AlignSelf,
    resolved_size: f32,
    position: Position,
    inset: Line<Option<f32>>,
    margin: Line<Option<f32>>,
    baseline_offset: Option<f32>,
) -> AlignedAxisPlacement {
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
        AlignItemsKeyword::Normal
        | AlignItemsKeyword::Start
        | AlignItemsKeyword::FlexStart
        | AlignItemsKeyword::Stretch => resolved_margin.start,
        AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd => grid_area_size - resolved_size - resolved_margin.end,
        AlignItemsKeyword::Baseline => baseline_offset.unwrap_or(resolved_margin.start),
        AlignItemsKeyword::LastBaseline => {
            baseline_offset.unwrap_or(grid_area_size - resolved_size - resolved_margin.end)
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
            (Some(start), _) => start + non_auto_margin.start,
            (None, Some(end)) => grid_area_size - end - resolved_size - non_auto_margin.end,
            (None, None) => alignment_based_offset,
        }
    } else {
        alignment_based_offset
    };

    let normal_flow_offset = grid_area.start + offset_within_area;
    let mut offset = normal_flow_offset;
    if position == Position::Relative {
        let relative_inset = inset.start.or(inset.end.map(|pos| -pos));
        offset += relative_inset.unwrap_or(0.0);
    }

    AlignedAxisPlacement { offset, normal_flow_offset, margin: resolved_margin }
}
