//! This module is a partial implementation of the CSS Grid Level 1 specification
//! <https://www.w3.org/TR/css-grid-1>
use crate::geometry::{AbstractAxis, InBothAbstractAxis};
use crate::geometry::{Line, LogicalSize, Size};
use crate::style::{AlignItems, AlignSelf, AvailableSpace, Position};
use crate::tree::{
    ChildLayoutInput, Layout, LayoutInput, LayoutOutput, LayoutPartialTree, LayoutPartialTreeExt, NodeId, RunMode,
    SizingMode,
};
use crate::util::debug::debug_log;
use crate::util::sys::{f32_max, f32_min, GridTrackVec, Vec};
use crate::util::MaybeMath;
use crate::util::ResolveOrZero;
use crate::{
    style_helpers::*, AlignContent, BoxGenerationMode, BoxSizing, CoreStyle, GridContainerStyle, GridItemStyle,
    JustifyContent, LayoutGridContainer, RequestedAxis,
};
use alignment::{align_and_position_item, align_tracks};
use explicit_grid::{compute_explicit_grid_size_in_axis, initialize_grid_tracks, AutoRepeatStrategy};
use implicit_grid::compute_grid_size_estimate;
use placement::place_grid_items;
use track_sizing::{
    determine_if_item_crosses_flexible_or_intrinsic_tracks, resolve_item_track_indexes, track_sizing_algorithm,
};
use types::{CellOccupancyMatrix, GridItem, GridTrack, NamedLineResolver, TrackCounts};

use super::common::baseline::physical_baseline;
use super::common::intrinsic_size::{
    apply_contained_intrinsic_size_constraints, resolve_node_size_constraints, AutomaticInlineSizeResolution,
    BlockSizeProperties, ContentBasedBlockSize, NodeSizeConstraintInput,
};
use super::common::used_size::resolve_used_size;

#[cfg(feature = "detailed_layout_info")]
use types::GridTrackKind;

pub(crate) use types::{GridCoordinate, GridLine, OriginZeroLine, MAX_GRID_TRACKS, MAX_OZ_LINE, MIN_OZ_LINE};

mod alignment;
mod explicit_grid;
/// Conversion boundary between logical Grid tracks and physical fragments.
mod flow;
mod implicit_grid;
mod placement;
mod track_sizing;
mod types;
mod util;

use flow::GridFlow;

/// Refresh every cached min-content contribution that can depend on the
/// opposite axis' resolved tracks.
///
/// Track sizing is allowed one additional pass after the container's used
/// size becomes definite. All affected items must be measured in that final
/// constraint space before deciding whether the pass is needed; using
/// `Iterator::any` here would stop at the first change and leave later caches
/// describing the provisional pass.
fn refresh_intrinsic_min_content_contributions<Tree: LayoutPartialTree>(
    tree: &mut Tree,
    items: &mut [GridItem],
    axis: AbstractAxis,
    axis_tracks: &[GridTrack],
    other_axis_tracks: &[GridTrack],
    resolved_inner_node_size: LogicalSize<Option<f32>>,
) -> bool {
    let mut contribution_changed = false;

    for item in items.iter_mut().filter(|item| item.crosses_intrinsic_track(axis)) {
        let grid_area_size = item.grid_area_size(
            axis,
            axis_tracks,
            other_axis_tracks,
            resolved_inner_node_size,
            |track: &GridTrack, _| Some(track.base_size),
            &|val, basis| tree.calc(val, basis),
        );
        let writing_mode = item.parent_writing_direction.mode;
        let available_space = writing_mode.to_physical(writing_mode.to_logical(grid_area_size).with(axis, None));
        let new_contribution = item.min_content_contribution(axis, tree, grid_area_size, available_space);

        contribution_changed |= Some(new_contribution) != item.min_content_contribution_cache.get(axis);
        item.grid_area_size_cache = Some(grid_area_size);
        item.min_content_contribution_cache.set(axis, Some(new_contribution));
        item.max_content_contribution_cache.set(axis, None);
        item.minimum_contribution_cache.set(axis, None);
    }

    contribution_changed
}

/// Invalidate contribution caches in one axis before rerunning with a newly
/// definite available size.
fn clear_intrinsic_contribution_caches(items: &mut [GridItem], axis: AbstractAxis) {
    for item in items {
        item.grid_area_size_cache = None;
        item.min_content_contribution_cache.set(axis, None);
        item.max_content_contribution_cache.set(axis, None);
        item.minimum_contribution_cache.set(axis, None);
    }
}

/// Grid layout algorithm
/// This consists of a few phases:
///   - Resolving the explicit grid
///   - Placing items (which also resolves the implicit grid)
///   - Track (row/column) sizing
///   - Alignment & Final item placement
pub fn compute_grid_layout<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    node: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    let writing_mode = tree.get_writing_mode(node);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { available_space, run_mode, .. } = inputs;

    let resolved_aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let size_containment = tree.get_size_containment(node);
    let scrollbar_insets = tree.get_scrollbar_insets(node);
    let style = tree.get_grid_container_style(node);
    let direction = style.direction();
    let flow = GridFlow::new(writing_mode, direction);

    // 1. Compute "available grid space"
    // https://www.w3.org/TR/css-grid-1/#available-grid-space
    let aspect_ratio = if inputs.sizing_mode == SizingMode::InherentSize { resolved_aspect_ratio } else { None };
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let margin = style.margin().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding_border = padding + border;
    let padding_border_size = padding_border.sum_axes();
    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    let content_box_inset = padding_border + scrollbar_insets;
    let logical_content_box_inset = flow.to_logical_size(content_box_inset.sum_axes());
    let explicit_contained_outer_size = size_containment.resolve_explicit_outer_size(content_box_inset.sum_axes());
    let logical_containment_axes = flow.to_logical_size(size_containment.axes);
    let logical_contained_content_size = flow.to_logical_size(size_containment.intrinsic_content_size);
    let derive_contained_inline_size =
        logical_containment_axes.inline_size && logical_contained_content_size.inline_size.is_none();
    let derive_contained_block_size =
        logical_containment_axes.block_size && logical_contained_content_size.block_size.is_none();

    let align_content = style.align_content().unwrap_or(AlignContent::STRETCH);
    let justify_content = style.justify_content().unwrap_or(JustifyContent::STRETCH);
    let align_items = style.align_items();
    let justify_items = style.justify_items();

    let raw_size = style.size();
    let raw_min_size = style.min_size();
    let raw_max_size = style.max_size();
    let logical_raw_size = writing_mode.to_logical(raw_size);
    let logical_raw_min_size = writing_mode.to_logical(raw_min_size);
    let logical_raw_max_size = writing_mode.to_logical(raw_max_size);
    let overflow = style.overflow();
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let content_based_block_size = ContentBasedBlockSize::new(
        BlockSizeProperties::new(
            logical_raw_size.block_size,
            logical_raw_min_size.block_size,
            logical_raw_max_size.block_size,
        ),
        aspect_ratio,
        padding_border_size,
        inputs.block_auto_behavior.is_content_based(aspect_ratio.is_some()),
        is_scroll_container,
        false,
    );
    let needs_content_based_block_resolution = inputs.sizing_mode == SizingMode::InherentSize
        && content_based_block_size.requires_resolution()
        && inputs.axis.contains(writing_mode.block_axis());
    drop(style);

    let node_sizing = resolve_node_size_constraints(
        tree,
        node,
        inputs,
        NodeSizeConstraintInput {
            raw_size,
            raw_min_size,
            raw_max_size,
            box_sizing_adjustment,
            padding_border_size,
            aspect_ratio,
            contained_outer_size: explicit_contained_outer_size,
            automatic_inline_size_resolution: AutomaticInlineSizeResolution::DeferToFormattingContext,
        },
    );
    let mut min_size = node_sizing.min_size;
    let mut max_size = node_sizing.max_size;
    let mut preferred_size = node_sizing.preferred_size;
    let outer_node_size = node_sizing.outer_size;
    let definite_outer_node_size = node_sizing.definite_size;
    let applied_aspect_ratio = run_mode == RunMode::ComputeSize && node_sizing.applied_aspect_ratio;
    let available_space = available_space.maybe_sub(margin.sum_axes());
    let constrained_available_space = definite_outer_node_size
        .map(|size| size.map(AvailableSpace::Definite))
        .unwrap_or(available_space.maybe_clamp(min_size, max_size).maybe_max(padding_border_size));

    let mut available_grid_space = flow.to_logical_size(Size {
        width: constrained_available_space
            .width
            .map_definite_value(|space| space - content_box_inset.horizontal_axis_sum()),
        height: constrained_available_space
            .height
            .map_definite_value(|space| space - content_box_inset.vertical_axis_sum()),
    });

    // The track sizing algorithm operates on the grid container's content box, so the min/max sizes
    // (which are border-box sizes) need converting to content-box sizes before being passed to it
    let inner_min_size = flow.to_logical_size(min_size.maybe_sub(content_box_inset.sum_axes()));
    let inner_max_size = flow.to_logical_size(max_size.maybe_sub(content_box_inset.sum_axes()));
    let mut inner_node_size = flow.to_logical_size(Size {
        width: definite_outer_node_size.width.map(|space| space - content_box_inset.horizontal_axis_sum()),
        height: definite_outer_node_size.height.map(|space| space - content_box_inset.vertical_axis_sum()),
    });

    debug_log!("outer_node_size", dbg:outer_node_size);
    debug_log!("inner_node_size", dbg:inner_node_size);

    // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
    // is ComputeSize (and thus the container's size is all that we're interested in)
    if run_mode == RunMode::ComputeSize
        && !needs_content_based_block_resolution
        && !derive_contained_inline_size
        && !derive_contained_block_size
    {
        if let Size { width: Some(width), height: Some(height) } = outer_node_size {
            return LayoutOutput::from_outer_size(Size { width, height })
                .with_block_constraint_dependency(node_sizing.depends_on_block_constraints)
                .with_applied_aspect_ratio(applied_aspect_ratio);
        }

        // We can also short-circuit if the width is known and only the width has been requested.
        if inputs.axis == RequestedAxis::Horizontal {
            if let Some(width) = outer_node_size.width {
                return LayoutOutput::from_outer_size(Size { width, height: 0.0 })
                    .with_block_constraint_dependency(node_sizing.depends_on_block_constraints)
                    .with_applied_aspect_ratio(applied_aspect_ratio);
            }
        }
    }

    // Absolutely positioned children do not take part in grid placement and do not create
    // implicit tracks, so they are excluded from the grid size estimate.
    let style = tree.get_grid_container_style(node);
    // Keep lazily projected track lists alive for the complete placement and
    // initialization phase so style adapters compute them only once.
    let grid_template_columns = style.grid_template_columns();
    let grid_template_rows = style.grid_template_rows();
    let grid_auto_columns = style.grid_auto_columns();
    let grid_auto_rows = style.grid_auto_rows();
    let get_child_styles_iter = |node| {
        tree.child_ids(node).map(|child_node: NodeId| tree.get_grid_child_style(child_node)).filter(|style| {
            style.box_generation_mode() != BoxGenerationMode::None && style.position() != Position::Absolute
        })
    };
    let child_styles_iter = get_child_styles_iter(node);

    // 2. Resolve the explicit grid

    // This is very similar to the inner_node_size except if the inner_node_size is not definite but the node
    // has a min- or max- size style then that will be used in it's place.
    let auto_fit_container_size = flow.to_logical_size(
        definite_outer_node_size
            .or(max_size)
            .or(min_size)
            .maybe_clamp(min_size, max_size)
            .maybe_max(padding_border_size)
            .maybe_sub(content_box_inset.sum_axes()),
    );

    // If the grid container has a definite size or max size in the relevant axis:
    //   - then the number of repetitions is the largest possible positive integer that does not cause the grid to overflow the content
    //     box of its grid container.
    // Otherwise, if the grid container has a definite min size in the relevant axis:
    //   - then the number of repetitions is the smallest possible positive integer that fulfills that minimum requirement
    // Otherwise, the specified track list repeats only once.
    let auto_repeat_fit_strategy = flow.to_logical_size(definite_outer_node_size.or(max_size).map(|val| match val {
        Some(_) => AutoRepeatStrategy::MaxRepetitionsThatDoNotOverflow,
        None => AutoRepeatStrategy::MinRepetitionsThatDoOverflow,
    }));

    // Compute the number of rows and columns in the explicit grid *template*
    // (explicit tracks from grid_areas are computed separately below)
    let (col_auto_repetition_count, grid_template_col_count) = compute_explicit_grid_size_in_axis(
        &style,
        auto_fit_container_size.inline_size,
        auto_repeat_fit_strategy.inline_size,
        |val, basis| tree.calc(val, basis),
        AbstractAxis::Inline,
    );
    let (row_auto_repetition_count, grid_template_row_count) = compute_explicit_grid_size_in_axis(
        &style,
        auto_fit_container_size.block_size,
        auto_repeat_fit_strategy.block_size,
        |val, basis| tree.calc(val, basis),
        AbstractAxis::Block,
    );

    // type CustomIdent<'a> = <<Tree as LayoutPartialTree>::CoreContainerStyle<'_> as CoreStyle>::CustomIdent;
    let mut name_resolver = NamedLineResolver::new(&style, col_auto_repetition_count, row_auto_repetition_count);

    // Clamp the explicit grid to MAX_GRID_TRACKS tracks in each axis
    // https://www.w3.org/TR/css-grid-1/#overlarge-grids
    let explicit_col_count = grid_template_col_count.max(name_resolver.area_column_count()).min(MAX_GRID_TRACKS);
    let explicit_row_count = grid_template_row_count.max(name_resolver.area_row_count()).min(MAX_GRID_TRACKS);

    name_resolver.set_explicit_column_count(explicit_col_count);
    name_resolver.set_explicit_row_count(explicit_row_count);

    // 3. Implicit Grid: Estimate Track Counts
    // Estimate the number of rows and columns in the implicit grid (= the entire grid)
    // This is necessary as part of placement. Doing it early here is a perf optimisation to reduce allocations.
    let (est_col_counts, est_row_counts) =
        compute_grid_size_estimate(explicit_col_count, explicit_row_count, flow.writing_direction(), child_styles_iter);

    // 4. Grid Item Placement
    // Match items (children) to a definite grid position (row start/end and column start/end position)
    let mut items = Vec::with_capacity(tree.child_count(node));
    let mut cell_occupancy_matrix = CellOccupancyMatrix::with_track_counts(est_col_counts, est_row_counts);
    let in_flow_children_iter = || {
        tree.child_ids(node)
            .enumerate()
            .map(|(index, child_node)| (index, child_node, tree.get_grid_child_style(child_node)))
            .filter(|(_, _, style)| {
                style.box_generation_mode() != BoxGenerationMode::None && style.position() != Position::Absolute
            })
    };
    place_grid_items(
        &mut cell_occupancy_matrix,
        &mut items,
        in_flow_children_iter,
        flow.writing_direction(),
        style.grid_auto_flow(),
        align_items.unwrap_or(AlignItems::NORMAL),
        justify_items.unwrap_or(AlignItems::NORMAL),
        &name_resolver,
    );
    for item in &mut items {
        item.aspect_ratio = tree.get_resolved_aspect_ratio(item.node);
    }

    // Extract track counts from previous step (auto-placement can expand the number of tracks)
    let final_col_counts = *cell_occupancy_matrix.track_counts(AbstractAxis::Inline);
    let final_row_counts = *cell_occupancy_matrix.track_counts(AbstractAxis::Block);

    // 5. Initialize Tracks
    // Initialize (explicit and implicit) grid tracks (and gutters)
    // This resolves the min and max track sizing functions for all tracks and gutters
    let mut columns = GridTrackVec::new();
    let mut rows = GridTrackVec::new();
    let inline_reversed = flow.axis_is_reversed(AbstractAxis::Inline);
    let block_reversed = flow.axis_is_reversed(AbstractAxis::Block);
    let column_track_counts_for_init = track_counts_for_initialization(final_col_counts, inline_reversed);
    let row_track_counts_for_init = track_counts_for_initialization(final_row_counts, block_reversed);
    initialize_grid_tracks(
        &mut columns,
        column_track_counts_for_init,
        &style,
        AbstractAxis::Inline,
        col_auto_repetition_count,
        |column_index| {
            let occupancy_index =
                track_occupancy_index_for_initialization(column_index, final_col_counts, inline_reversed);
            cell_occupancy_matrix.column_is_occupied(occupancy_index)
        },
    );
    initialize_grid_tracks(
        &mut rows,
        row_track_counts_for_init,
        &style,
        AbstractAxis::Block,
        row_auto_repetition_count,
        |row_index| {
            let occupancy_index = track_occupancy_index_for_initialization(row_index, final_row_counts, block_reversed);
            cell_occupancy_matrix.row_is_occupied(occupancy_index)
        },
    );
    if inline_reversed {
        reverse_non_gutter_tracks(&mut columns, final_col_counts);
    }
    if block_reversed {
        reverse_non_gutter_tracks(&mut rows, final_row_counts);
    }

    // A contained Grid axis without an explicit intrinsic override is sized
    // from the explicit track definitions with all item contributions
    // removed. Keep this track tree separate from the real one: descendants
    // still participate in final layout and may overflow the contained box,
    // but they cannot feed back into the container's own intrinsic size.
    let no_item_tracks = if derive_contained_inline_size || derive_contained_block_size {
        let explicit_column_counts = TrackCounts::from_raw(0, explicit_col_count, 0);
        let explicit_row_counts = TrackCounts::from_raw(0, explicit_row_count, 0);
        let mut no_item_columns = GridTrackVec::new();
        let mut no_item_rows = GridTrackVec::new();
        initialize_grid_tracks(
            &mut no_item_columns,
            track_counts_for_initialization(explicit_column_counts, inline_reversed),
            &style,
            AbstractAxis::Inline,
            col_auto_repetition_count,
            |_| false,
        );
        initialize_grid_tracks(
            &mut no_item_rows,
            track_counts_for_initialization(explicit_row_counts, block_reversed),
            &style,
            AbstractAxis::Block,
            row_auto_repetition_count,
            |_| false,
        );
        if inline_reversed {
            reverse_non_gutter_tracks(&mut no_item_columns, explicit_column_counts);
        }
        if block_reversed {
            reverse_non_gutter_tracks(&mut no_item_rows, explicit_row_counts);
        }
        Some((no_item_columns, no_item_rows))
    } else {
        None
    };

    drop(grid_template_rows);
    drop(grid_template_columns);
    drop(grid_auto_rows);
    drop(grid_auto_columns);
    drop(style);

    // 6. Track Sizing

    let mut no_item_available_grid_space = available_grid_space;
    let mut no_item_inner_node_size = inner_node_size;
    if derive_contained_inline_size {
        no_item_available_grid_space.inline_size = AvailableSpace::MaxContent;
        no_item_inner_node_size.inline_size = None;
    }
    if derive_contained_block_size {
        no_item_available_grid_space.block_size = AvailableSpace::MaxContent;
        no_item_inner_node_size.block_size = None;
    }
    let no_item_track_size = no_item_tracks
        .map(|(columns, rows)| {
            size_grid_tracks_without_items(
                tree,
                columns,
                rows,
                inner_min_size,
                inner_max_size,
                justify_content,
                align_content,
                no_item_available_grid_space,
                no_item_inner_node_size,
            )
        })
        .unwrap_or(LogicalSize::ZERO);
    let derived_contained_outer_size = flow.to_physical_size(LogicalSize {
        inline_size: derive_contained_inline_size
            .then_some(no_item_track_size.inline_size + logical_content_box_inset.inline_size),
        block_size: derive_contained_block_size
            .then_some(no_item_track_size.block_size + logical_content_box_inset.block_size),
    });
    let intrinsic_contained_outer_size = explicit_contained_outer_size.or(derived_contained_outer_size);

    // Intrinsic min/max keywords are resolved from the same contained
    // contribution as the preferred size. Preserve their authored source
    // ordering when replacing the provisional real-content measurements.
    let mut resolved_constraints = node_sizing.constraints;
    resolved_constraints.size = preferred_size;
    resolved_constraints.min_size = min_size;
    resolved_constraints.max_size = max_size;
    let contained_constraints = apply_contained_intrinsic_size_constraints(
        resolved_constraints,
        raw_size,
        raw_min_size,
        raw_max_size,
        intrinsic_contained_outer_size,
    );
    min_size = contained_constraints.min_size;
    max_size = contained_constraints.max_size;
    preferred_size = contained_constraints.size.or(intrinsic_contained_outer_size.maybe_clamp(min_size, max_size));
    let contained_outer_size = intrinsic_contained_outer_size.maybe_clamp(min_size, max_size);
    let used_outer_size = resolve_used_size(
        inputs.known_dimensions,
        node_sizing.outer_size.or(preferred_size).or(contained_outer_size),
        min_size,
        max_size,
        padding_border_size,
    );

    let logical_intrinsic_contained_outer_size = flow.to_logical_size(intrinsic_contained_outer_size);
    let content_based_block_size = content_based_block_size.with_intrinsic_border_box_override(
        logical_containment_axes.block_size.then_some(logical_intrinsic_contained_outer_size.block_size).flatten(),
    );

    // The contained used size becomes the real track pass' available space.
    // Items remain present in that pass for layout and overflow, but they no
    // longer determine the size of the container itself.
    let logical_used_outer_size = flow.to_logical_size(used_outer_size);
    if logical_containment_axes.inline_size {
        if let Some(outer_size) = logical_used_outer_size.inline_size {
            let inner_size = f32_max(0.0, outer_size - logical_content_box_inset.inline_size);
            inner_node_size.inline_size = Some(inner_size);
            available_grid_space.inline_size = AvailableSpace::Definite(inner_size);
        }
    }
    if logical_containment_axes.block_size {
        if let Some(outer_size) = logical_used_outer_size.block_size {
            let inner_size = f32_max(0.0, outer_size - logical_content_box_inset.block_size);
            inner_node_size.block_size = Some(inner_size);
            available_grid_space.block_size = AvailableSpace::Definite(inner_size);
        }
    }

    // Containment may have resolved intrinsic min/max keywords after track
    // initialization, so the real pass must observe the updated constraints.
    let inner_min_size = flow.to_logical_size(min_size.maybe_sub(content_box_inset.sum_axes()));
    let inner_max_size = flow.to_logical_size(max_size.maybe_sub(content_box_inset.sum_axes()));

    // Convert grid placements in origin-zero coordinates to indexes into the GridTrack (rows and columns) vectors
    // This computation is relatively trivial, but it requires the final number of negative (implicit) tracks in
    // each axis, and doing it up-front here means we don't have to keep repeating that calculation
    resolve_item_track_indexes(&mut items, final_col_counts, final_row_counts);
    // For each item, and in each axis, determine whether the item crosses any flexible (fr) tracks
    // Record this as a boolean (per-axis) on each item for later use in the track-sizing algorithm
    determine_if_item_crosses_flexible_or_intrinsic_tracks(&mut items, &columns, &rows);

    // Determine if the grid has any baseline aligned items
    let has_baseline_aligned_item = items.iter().any(|item| item.align_self == AlignSelf::BASELINE);

    // Keep the original definiteness separate from the provisional numeric
    // sizes produced by intrinsic track sizing. A track collection may need a
    // second pass once the container's used content-box size is known.
    let initial_inner_node_size = inner_node_size;

    // Run track sizing algorithm for Inline axis
    let column_track_sizing_result = track_sizing_algorithm(
        tree,
        AbstractAxis::Inline,
        inner_min_size.get(AbstractAxis::Inline),
        inner_max_size.get(AbstractAxis::Inline),
        justify_content,
        align_content,
        available_grid_space,
        inner_node_size,
        &mut columns,
        &mut rows,
        &mut items,
        |track: &GridTrack, parent_size: Option<f32>, tree: &Tree| {
            track.max_track_sizing_function.definite_value(parent_size, |val, basis| tree.calc(val, basis))
        },
        has_baseline_aligned_item,
    );
    let initial_column_sum = columns.iter().map(|track| track.base_size).sum::<f32>();
    inner_node_size.inline_size = inner_node_size.inline_size.or_else(|| initial_column_sum.into());

    items.iter_mut().for_each(|item| item.grid_area_size_cache = None);

    // Run track sizing algorithm for Block axis
    let row_track_sizing_result = track_sizing_algorithm(
        tree,
        AbstractAxis::Block,
        inner_min_size.get(AbstractAxis::Block),
        inner_max_size.get(AbstractAxis::Block),
        align_content,
        justify_content,
        available_grid_space,
        inner_node_size,
        &mut rows,
        &mut columns,
        &mut items,
        |track: &GridTrack, _, _| Some(track.base_size),
        false, // TODO: Support baseline alignment in the vertical axis
    );
    let initial_row_sum = rows.iter().map(|track| track.base_size).sum::<f32>();

    debug_log!("initial_column_sum", dbg:initial_column_sum);
    debug_log!(dbg: columns.iter().map(|track| track.base_size).collect::<Vec<_>>());
    debug_log!("initial_row_sum", dbg:initial_row_sum);
    debug_log!(dbg: rows.iter().map(|track| track.base_size).collect::<Vec<_>>());

    // 6. Compute container size
    let initial_column_container_size =
        column_track_sizing_result.constrained_available_space.unwrap_or(initial_column_sum);
    let initial_row_container_size = row_track_sizing_result.constrained_available_space.unwrap_or(initial_row_sum);
    let intrinsic_border_box_size = flow.to_physical_size(LogicalSize {
        inline_size: initial_column_container_size + logical_content_box_inset.inline_size,
        block_size: initial_row_container_size + logical_content_box_inset.block_size,
    });
    let resolve_container_border_box = |preferred_size: Size<Option<f32>>,
                                        min_size: Size<Option<f32>>,
                                        max_size: Size<Option<f32>>,
                                        content_based_size: Size<f32>| {
        resolve_used_size(
            inputs.known_dimensions,
            preferred_size.or(content_based_size.map(Some)),
            min_size,
            max_size,
            padding_border_size,
        )
        .unwrap_or(padding_border_size)
    };
    let preliminary_container_border_box =
        resolve_container_border_box(preferred_size, min_size, max_size, intrinsic_border_box_size);
    if needs_content_based_block_resolution {
        let logical_preliminary_size = writing_mode.to_logical(preliminary_container_border_box);
        let logical_intrinsic_size = writing_mode.to_logical(intrinsic_border_box_size);
        content_based_block_size
            .resolve(writing_mode, Some(logical_preliminary_size.inline_size), logical_intrinsic_size.block_size)
            .apply_to_block_axis(
                writing_mode,
                node_sizing.constraints.block_axis_constraints(writing_mode),
                padding_border_size,
                &mut preferred_size,
                &mut min_size,
                &mut max_size,
            );
    }
    let mut container_border_box =
        resolve_container_border_box(preferred_size, min_size, max_size, intrinsic_border_box_size);
    let mut container_content_box = Size {
        width: f32_max(0.0, container_border_box.width - content_box_inset.horizontal_axis_sum()),
        height: f32_max(0.0, container_border_box.height - content_box_inset.vertical_axis_sum()),
    };
    let logical_container_content_box = flow.to_logical_size(container_content_box);
    let resolved_inner_node_size = logical_container_content_box.map(Some);
    let resolved_available_grid_space = logical_container_content_box.map(AvailableSpace::Definite);

    // 7. Resolve percentage track base sizes
    // In the case of an indefinitely sized container these resolve to zero during the "Initialise Tracks" step
    // and therefore need to be re-resolved here based on the content-sized content box of the container
    if initial_inner_node_size.inline_size.is_none() {
        for column in &mut columns {
            let min: Option<f32> = column
                .min_track_sizing_function
                .resolved_percentage_size(logical_container_content_box.inline_size, |val, basis| {
                    tree.calc(val, basis)
                });
            let max: Option<f32> = column
                .max_track_sizing_function
                .resolved_percentage_size(logical_container_content_box.inline_size, |val, basis| {
                    tree.calc(val, basis)
                });
            column.base_size = column.base_size.maybe_clamp(min, max);
        }
    }
    if initial_inner_node_size.block_size.is_none() {
        for row in &mut rows {
            let min: Option<f32> = row
                .min_track_sizing_function
                .resolved_percentage_size(logical_container_content_box.block_size, |val, basis| tree.calc(val, basis));
            let max: Option<f32> = row
                .max_track_sizing_function
                .resolved_percentage_size(logical_container_content_box.block_size, |val, basis| tree.calc(val, basis));
            row.base_size = row.base_size.maybe_clamp(min, max);
        }
    }

    let final_inner_min_size = flow.to_logical_size(min_size.maybe_sub(content_box_inset.sum_axes()));
    let final_inner_max_size = flow.to_logical_size(max_size.maybe_sub(content_box_inset.sum_axes()));
    let initial_logical_inner_size = initial_inner_node_size;
    let final_logical_preferred_size = writing_mode.to_logical(preferred_size);
    let ratio_resolved_block_size_after_track_sizing = needs_content_based_block_resolution
        && content_based_block_size.resolves_auto_size_from_ratio()
        && initial_logical_inner_size.block_size.is_none()
        && final_logical_preferred_size.block_size.is_some();

    // A ratio-dependent automatic block size becomes definite only after the
    // first pass establishes the final inline size. Re-run the complete
    // columns-then-rows pipeline in that resolved constraint space so every
    // track and cross-axis contribution observes the same final geometry.
    if ratio_resolved_block_size_after_track_sizing {
        clear_intrinsic_contribution_caches(&mut items, AbstractAxis::Inline);
        clear_intrinsic_contribution_caches(&mut items, AbstractAxis::Block);
        let _ = track_sizing_algorithm(
            tree,
            AbstractAxis::Inline,
            final_inner_min_size.get(AbstractAxis::Inline),
            final_inner_max_size.get(AbstractAxis::Inline),
            justify_content,
            align_content,
            resolved_available_grid_space,
            resolved_inner_node_size,
            &mut columns,
            &mut rows,
            &mut items,
            |track: &GridTrack, _, _| Some(track.base_size),
            has_baseline_aligned_item,
        );

        items.iter_mut().for_each(|item| item.grid_area_size_cache = None);
        let _ = track_sizing_algorithm(
            tree,
            AbstractAxis::Block,
            final_inner_min_size.get(AbstractAxis::Block),
            final_inner_max_size.get(AbstractAxis::Block),
            align_content,
            justify_content,
            resolved_available_grid_space,
            resolved_inner_node_size,
            &mut rows,
            &mut columns,
            &mut items,
            |track: &GridTrack, _, _| Some(track.base_size),
            false, // TODO: Support baseline alignment in the vertical axis
        );
    }

    // An initially indefinite axis is sized one more time if its track
    // collection depends on the resolved available size (percentage or flex),
    // or if sizing the opposite axis changed an intrinsic contribution.
    let mut intrinsic_column_contribution_changed = false;
    let has_percentage_column = columns.iter().any(|track| track.uses_percentage());
    let has_percentage_row = rows.iter().any(|track| track.uses_percentage());
    let mut rerun_column_sizing = !ratio_resolved_block_size_after_track_sizing
        && initial_inner_node_size.inline_size.is_none()
        && columns.iter().any(GridTrack::depends_on_available_size);

    if !rerun_column_sizing && !ratio_resolved_block_size_after_track_sizing {
        intrinsic_column_contribution_changed = refresh_intrinsic_min_content_contributions(
            tree,
            &mut items,
            AbstractAxis::Inline,
            &columns,
            &rows,
            resolved_inner_node_size,
        );
        rerun_column_sizing = intrinsic_column_contribution_changed;
    } else {
        clear_intrinsic_contribution_caches(&mut items, AbstractAxis::Inline);
    }

    if rerun_column_sizing {
        // Re-run track sizing algorithm for Inline axis
        let _ = track_sizing_algorithm(
            tree,
            AbstractAxis::Inline,
            final_inner_min_size.get(AbstractAxis::Inline),
            final_inner_max_size.get(AbstractAxis::Inline),
            justify_content,
            align_content,
            resolved_available_grid_space,
            resolved_inner_node_size,
            &mut columns,
            &mut rows,
            &mut items,
            |track: &GridTrack, _, _| Some(track.base_size),
            has_baseline_aligned_item,
        );
    }

    // Block-axis sizing always observes the final column geometry. It must not
    // be conditional on whether columns happened to need their own rerun.
    let mut rerun_row_sizing = !ratio_resolved_block_size_after_track_sizing
        && initial_inner_node_size.block_size.is_none()
        && rows.iter().any(GridTrack::depends_on_available_size);
    let mut intrinsic_row_contribution_changed = false;
    if !rerun_row_sizing && !ratio_resolved_block_size_after_track_sizing {
        intrinsic_row_contribution_changed = refresh_intrinsic_min_content_contributions(
            tree,
            &mut items,
            AbstractAxis::Block,
            &rows,
            &columns,
            resolved_inner_node_size,
        );
        rerun_row_sizing = intrinsic_row_contribution_changed;
    } else {
        clear_intrinsic_contribution_caches(&mut items, AbstractAxis::Block);
    }

    if rerun_row_sizing {
        // Re-run track sizing algorithm for Block axis
        let _ = track_sizing_algorithm(
            tree,
            AbstractAxis::Block,
            final_inner_min_size.get(AbstractAxis::Block),
            final_inner_max_size.get(AbstractAxis::Block),
            align_content,
            justify_content,
            resolved_available_grid_space,
            resolved_inner_node_size,
            &mut rows,
            &mut columns,
            &mut items,
            |track: &GridTrack, _, _| Some(track.base_size),
            false, // TODO: Support baseline alignment in the vertical axis
        );
    }

    if (intrinsic_column_contribution_changed && !has_percentage_column)
        || (intrinsic_row_contribution_changed && !has_percentage_row)
    {
        let final_column_sum = columns.iter().map(|track| track.base_size).sum::<f32>();
        let final_row_sum = rows.iter().map(|track| track.base_size).sum::<f32>();
        let final_container_border_box = resolve_container_border_box(
            preferred_size,
            min_size,
            max_size,
            flow.to_physical_size(LogicalSize {
                inline_size: final_column_sum + logical_content_box_inset.inline_size,
                block_size: final_row_sum + logical_content_box_inset.block_size,
            }),
        );
        let final_logical_container_border_box = flow.to_logical_size(final_container_border_box);
        let mut logical_container_border_box = flow.to_logical_size(container_border_box);
        let mut logical_container_content_box = flow.to_logical_size(container_content_box);

        if intrinsic_column_contribution_changed && !has_percentage_column {
            logical_container_border_box.inline_size = final_logical_container_border_box.inline_size;
            logical_container_content_box.inline_size =
                f32_max(0.0, logical_container_border_box.inline_size - logical_content_box_inset.inline_size);
        }

        if intrinsic_row_contribution_changed && !has_percentage_row {
            logical_container_border_box.block_size = final_logical_container_border_box.block_size;
            logical_container_content_box.block_size =
                f32_max(0.0, logical_container_border_box.block_size - logical_content_box_inset.block_size);
        }
        container_border_box = flow.to_physical_size(logical_container_border_box);
        container_content_box = flow.to_physical_size(logical_container_content_box);
    }

    // If only the container's size has been requested
    if run_mode == RunMode::ComputeSize {
        let depends_on_block_constraints = items.iter().any(|item| item.depends_on_block_constraints);
        return LayoutOutput::from_outer_size(container_border_box)
            .with_block_constraint_dependency(depends_on_block_constraints || node_sizing.depends_on_block_constraints)
            .with_applied_aspect_ratio(applied_aspect_ratio);
    }

    // 8. Track Alignment

    let logical_container_border_box = flow.to_logical_size(container_border_box);
    let logical_container_content_box = flow.to_logical_size(container_content_box);
    let logical_padding_border_size = flow.to_logical_size(padding_border_size);

    // Align columns in the container's logical inline axis. Track offsets are
    // physical low-to-high coordinates, so padding and scrollbar edges cross
    // through GridFlow while alignment itself retains logical start/end.
    let inline_size_without_scrollbar =
        f32_max(logical_container_border_box.inline_size - logical_padding_border_size.inline_size, 0.0);
    let inline_scrollbar = flow.physical_axis_line(scrollbar_insets, AbstractAxis::Inline);
    let inline_scrollbar_scale = {
        let total = inline_scrollbar.sum();
        if total > 0.0 {
            f32_min(1.0, inline_size_without_scrollbar / total)
        } else {
            1.0
        }
    };
    let inline_padding = flow.physical_axis_line(padding, AbstractAxis::Inline);
    align_tracks(
        logical_container_content_box.inline_size,
        Line {
            start: inline_padding.start + inline_scrollbar.start * inline_scrollbar_scale,
            end: inline_padding.end + inline_scrollbar.end * inline_scrollbar_scale,
        },
        flow.physical_axis_line(border, AbstractAxis::Inline),
        &mut columns,
        justify_content,
        flow.axis_is_reversed(AbstractAxis::Inline),
    );
    // Align rows in the logical block axis.
    let block_size_without_scrollbar =
        f32_max(logical_container_border_box.block_size - logical_padding_border_size.block_size, 0.0);
    let block_scrollbar = flow.physical_axis_line(scrollbar_insets, AbstractAxis::Block);
    let block_scrollbar_scale = {
        let total = block_scrollbar.sum();
        if total > 0.0 {
            f32_min(1.0, block_size_without_scrollbar / total)
        } else {
            1.0
        }
    };
    let block_padding = flow.physical_axis_line(padding, AbstractAxis::Block);
    align_tracks(
        logical_container_content_box.block_size,
        Line {
            start: block_padding.start + block_scrollbar.start * block_scrollbar_scale,
            end: block_padding.end + block_scrollbar.end * block_scrollbar_scale,
        },
        flow.physical_axis_line(border, AbstractAxis::Block),
        &mut rows,
        align_content,
        flow.axis_is_reversed(AbstractAxis::Block),
    );

    // 9. Size, Align, and Position Grid Items

    #[cfg_attr(not(feature = "content_size"), allow(unused_mut))]
    let mut item_content_size_contribution = Size::ZERO;
    #[cfg_attr(not(feature = "content_size"), allow(unused_mut, unused))]
    let mut absolute_content_size = Size::ZERO;

    // Sort items back into original order to allow them to be matched up with styles
    items.sort_by_key(|item| item.source_order);

    let container_alignment_styles = InBothAbstractAxis { inline: justify_items, block: align_items };

    // Position in-flow children (stored in items vector)
    for (index, item) in items.iter_mut().enumerate() {
        let grid_area = flow.to_physical_rect(
            Line {
                start: columns[item.column_indexes.start as usize + 1].offset,
                end: columns[item.column_indexes.end as usize].offset,
            },
            Line {
                start: rows[item.row_indexes.start as usize + 1].offset,
                end: rows[item.row_indexes.end as usize].offset,
            },
        );
        #[cfg_attr(not(feature = "content_size"), allow(unused_variables))]
        let placement = align_and_position_item(
            tree,
            item.node,
            index as u32,
            grid_area,
            container_alignment_styles,
            item.baseline_shim,
            direction,
            writing_mode,
            container_border_box,
            border,
            scrollbar_insets,
        );
        item.block_offset = placement.block_offset;
        item.block_size = placement.block_size;
        item.first_baseline = placement.first_baseline;
        item.last_baseline = placement.last_baseline;

        #[cfg(feature = "content_size")]
        {
            item_content_size_contribution =
                item_content_size_contribution.f32_max(placement.content_size_contribution);
        }
    }

    // Position hidden and absolutely positioned children
    let mut order = items.len() as u32;
    (0..tree.child_count(node)).for_each(|index| {
        let child = tree.get_child_id(node, index);
        let child_style = tree.get_grid_child_style(child);

        // Position hidden child
        if child_style.box_generation_mode() == BoxGenerationMode::None {
            drop(child_style);
            tree.set_unrounded_layout(child, &Layout::with_order(order));
            tree.perform_child_layout(
                child,
                ChildLayoutInput::new(
                    Size::NONE,
                    Size::NONE,
                    writing_mode,
                    Size::MAX_CONTENT,
                    SizingMode::InherentSize,
                    Line::FALSE,
                )
                .without_orthogonal_fallback(),
            );
            order += 1;
            return;
        }

        // Position absolutely positioned child
        if child_style.position() == Position::Absolute {
            // Convert grid-col-{start/end} into Option's of indexes into the columns vector
            // The Option is None if the style property is Auto and an unresolvable Span
            let maybe_col_indexes = name_resolver
                .resolve_column_names(&child_style.grid_column())
                .into_origin_zero(final_col_counts.explicit)
                .resolve_absolutely_positioned_grid_tracks()
                .map(|maybe_grid_line| {
                    maybe_grid_line
                        .map(|line: OriginZeroLine| {
                            if inline_reversed {
                                OriginZeroLine(final_col_counts.explicit as i16 - line.0)
                            } else {
                                line
                            }
                        })
                        .and_then(|line| line.try_into_track_vec_index(final_col_counts))
                });
            let maybe_col_indexes = if inline_reversed {
                Line { start: maybe_col_indexes.end, end: maybe_col_indexes.start }
            } else {
                maybe_col_indexes
            };
            // Convert grid-row-{start/end} into Option's of indexes into the row vector
            // The Option is None if the style property is Auto and an unresolvable Span
            let maybe_row_indexes = name_resolver
                .resolve_row_names(&child_style.grid_row())
                .into_origin_zero(final_row_counts.explicit)
                .resolve_absolutely_positioned_grid_tracks()
                .map(|maybe_grid_line| {
                    maybe_grid_line
                        .map(|line: OriginZeroLine| {
                            if block_reversed {
                                OriginZeroLine(final_row_counts.explicit as i16 - line.0)
                            } else {
                                line
                            }
                        })
                        .and_then(|line| line.try_into_track_vec_index(final_row_counts))
                });
            let maybe_row_indexes = if block_reversed {
                Line { start: maybe_row_indexes.end, end: maybe_row_indexes.start }
            } else {
                maybe_row_indexes
            };

            // Content alignment (align-content/justify-content) may distribute free space before, between,
            // or after tracks. Grid lines used by absolutely positioned items resolve to the edges of the
            // tracks adjacent to the line rather than to the raw gutter offset:
            //   - As a start edge, a line resolves to the start of the track that follows it
            //   - As an end edge, a line resolves to the end of the track that precedes it
            /// Resolve a grid line (by track vector index) used as a start edge to a position
            fn line_as_start_edge(tracks: &[GridTrack], index: usize) -> f32 {
                tracks.get(index + 1).unwrap_or(&tracks[index]).offset
            }
            /// Resolve a grid line (by track vector index) used as an end edge to a position
            fn line_as_end_edge(tracks: &[GridTrack], index: usize) -> f32 {
                if index == 0 {
                    tracks.get(1).unwrap_or(&tracks[0]).offset
                } else {
                    tracks[index].offset
                }
            }

            let inline_inset = flow.physical_axis_line(border + scrollbar_insets, AbstractAxis::Inline);
            let block_inset = flow.physical_axis_line(border + scrollbar_insets, AbstractAxis::Block);
            let inline_containing_bounds =
                Line { start: inline_inset.start, end: logical_container_border_box.inline_size - inline_inset.end };
            let block_containing_bounds =
                Line { start: block_inset.start, end: logical_container_border_box.block_size - block_inset.end };
            let grid_area = flow.to_physical_rect(
                Line {
                    start: maybe_col_indexes
                        .start
                        .map(|index| line_as_start_edge(&columns, index))
                        .unwrap_or(inline_containing_bounds.start),
                    end: maybe_col_indexes
                        .end
                        .map(|index| line_as_end_edge(&columns, index))
                        .unwrap_or(inline_containing_bounds.end),
                },
                Line {
                    start: maybe_row_indexes
                        .start
                        .map(|index| line_as_start_edge(&rows, index))
                        .unwrap_or(block_containing_bounds.start),
                    end: maybe_row_indexes
                        .end
                        .map(|index| line_as_end_edge(&rows, index))
                        .unwrap_or(block_containing_bounds.end),
                },
            );
            drop(child_style);

            // TODO: Baseline alignment support for absolutely positioned items (should check if is actually specified)
            #[cfg_attr(not(feature = "content_size"), allow(unused_variables))]
            let placement = align_and_position_item(
                tree,
                child,
                order,
                grid_area,
                container_alignment_styles,
                0.0,
                direction,
                writing_mode,
                container_border_box,
                border,
                scrollbar_insets,
            );
            #[cfg(feature = "content_size")]
            {
                absolute_content_size = absolute_content_size.f32_max(placement.content_size_contribution);
            }

            order += 1;
        }
    });

    // Set detailed grid information
    #[cfg(feature = "detailed_layout_info")]
    tree.set_detailed_grid_info(
        node,
        DetailedGridInfo {
            rows: DetailedGridTracksInfo::from_grid_tracks_and_track_count(
                final_row_counts,
                row_auto_repetition_count,
                rows,
            ),
            columns: DetailedGridTracksInfo::from_grid_tracks_and_track_count(
                final_col_counts,
                col_auto_repetition_count,
                columns,
            ),
            items: items.iter().map(DetailedGridItemsInfo::from_grid_item).collect(),
        },
    );

    // If there are not items then return just the container size (no baseline)
    if items.is_empty() {
        return LayoutOutput::from_outer_size(container_border_box);
    }

    let (first_baseline, last_baseline) = grid_container_baselines(&items);

    // The container's own padding at the end of the content is part of its scrollable
    // overflow region, so it is included in the in-flow content size.
    #[cfg(feature = "content_size")]
    let content_size = {
        let logical_padding = flow.writing_direction().to_logical_box_strut(padding);
        let mut logical_content_size = flow.to_logical_size(item_content_size_contribution);
        logical_content_size.inline_size += logical_padding.inline_end;
        logical_content_size.block_size += logical_padding.block_end;
        flow.to_physical_size(logical_content_size).f32_max(absolute_content_size)
    };
    #[cfg(not(feature = "content_size"))]
    let content_size = item_content_size_contribution;

    LayoutOutput::from_sizes_and_baseline_sets(
        container_border_box,
        content_size,
        physical_baseline(Some(first_baseline), container_border_box, flow.writing_direction()),
        physical_baseline(Some(last_baseline), container_border_box, flow.writing_direction()),
    )
}

/// Size explicit Grid tracks with an empty item set.
///
/// CSS size containment is unusual for Grid: without an explicit
/// `contain-intrinsic-size`, its intrinsic size still includes track
/// definitions. This isolated pass mirrors Blink's
/// `BuildGridSizingTreeIgnoringChildren`; the real track tree remains intact
/// for final item layout.
#[allow(clippy::too_many_arguments)]
fn size_grid_tracks_without_items<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    mut columns: GridTrackVec<GridTrack>,
    mut rows: GridTrackVec<GridTrack>,
    inner_min_size: LogicalSize<Option<f32>>,
    inner_max_size: LogicalSize<Option<f32>>,
    justify_content: JustifyContent,
    align_content: AlignContent,
    available_grid_space: LogicalSize<AvailableSpace>,
    mut inner_node_size: LogicalSize<Option<f32>>,
) -> LogicalSize<f32> {
    let mut items = Vec::<GridItem>::new();
    track_sizing_algorithm(
        tree,
        AbstractAxis::Inline,
        inner_min_size.inline_size,
        inner_max_size.inline_size,
        justify_content,
        align_content,
        available_grid_space,
        inner_node_size,
        &mut columns,
        &mut rows,
        &mut items,
        |track: &GridTrack, parent_size: Option<f32>, tree: &Tree| {
            track.max_track_sizing_function.definite_value(parent_size, |val, basis| tree.calc(val, basis))
        },
        false,
    );
    let inline_size = columns.iter().map(|track| track.base_size).sum::<f32>();
    inner_node_size.inline_size = inner_node_size.inline_size.or(Some(inline_size));

    track_sizing_algorithm(
        tree,
        AbstractAxis::Block,
        inner_min_size.block_size,
        inner_max_size.block_size,
        align_content,
        justify_content,
        available_grid_space,
        inner_node_size,
        &mut rows,
        &mut columns,
        &mut items,
        |track: &GridTrack, _, _| Some(track.base_size),
        false,
    );
    let block_size = rows.iter().map(|track| track.base_size).sum::<f32>();

    LogicalSize { inline_size, block_size }
}

/// Select the grid container's baselines from final item fragments.
///
/// This follows Blink's `GridBaselineAccumulator`: a baseline-sharing group in the first/last
/// occupied row wins, otherwise selection falls back to the first/last item in
/// grid order. Fallback selection uses the child's corresponding baseline and
/// synthesizes one at its block-end border edge only when that baseline is
/// absent.
fn grid_container_baselines(items: &[GridItem]) -> (f32, f32) {
    debug_assert!(!items.is_empty());

    let first_occupied_row = items.iter().map(|item| item.row_indexes.start).min().unwrap();
    let first_item = items
        .iter()
        .filter(|item| item.row_indexes.start == first_occupied_row && item.align_self == AlignSelf::BASELINE)
        .min_by_key(|item| (item.column_indexes.start, item.source_order))
        .or_else(|| {
            items
                .iter()
                .filter(|item| item.row_indexes.start == first_occupied_row)
                .min_by_key(|item| (item.column_indexes.start, item.source_order))
        })
        .unwrap();
    let first_baseline = first_item.block_offset + first_item.first_baseline.unwrap_or(first_item.block_size);

    // GridTrackVec stores one line/gutter slot on each side of a row track, so
    // the start index of an item's last occupied row is two slots before its
    // end index.
    let last_occupied_row = items.iter().map(|item| item.row_indexes.end.saturating_sub(2)).max().unwrap();
    let last_item = items
        .iter()
        .filter(|item| item.row_indexes.start == last_occupied_row && item.align_self == AlignSelf::BASELINE)
        .max_by_key(|item| (item.column_indexes.end, item.source_order))
        .or_else(|| items.iter().max_by_key(|item| (item.row_indexes.end, item.column_indexes.end, item.source_order)))
        .unwrap();
    let last_baseline =
        if last_item.align_self == AlignSelf::BASELINE && last_item.row_indexes.start == last_occupied_row {
            // Taffy currently supports first-baseline sharing groups. When such a
            // group exists in the last occupied row, its shared major baseline is
            // also the grid container's last baseline.
            last_item.first_baseline.unwrap_or(last_item.block_size)
        } else {
            last_item.last_baseline.unwrap_or(last_item.block_size)
        };

    (first_baseline, last_item.block_offset + last_baseline)
}

/// Reverse non-gutter tracks in-place while preserving line/gutter slots.
fn reverse_non_gutter_tracks(tracks: &mut [GridTrack], track_counts: TrackCounts) {
    // When the explicit grid has 0/1 tracks, reversing the flow is entirely
    // determined by implicit tracks. Reverse every non-gutter track in that case.
    if track_counts.explicit <= 1 {
        const MIN_TRACK_VEC_LEN_TO_REVERSE: usize = 5;
        if tracks.len() < MIN_TRACK_VEC_LEN_TO_REVERSE {
            return;
        }
        let mut left = 1;
        let mut right = tracks.len() - 2;
        while left < right {
            tracks.swap(left, right);
            left += 2;
            right = right.saturating_sub(2);
        }
        return;
    }

    let explicit_track_count = track_counts.explicit as usize;
    if explicit_track_count < 2 {
        return;
    }

    let mut left = track_counts.negative_implicit as usize;
    let mut right = left + explicit_track_count - 1;
    while left < right {
        tracks.swap((2 * left) + 1, (2 * right) + 1);
        left += 1;
        right = right.saturating_sub(1);
    }
}

/// Swap implicit track sides when a one-track grid is stored from the
/// physical low edge but its logical start lies at the high edge.
fn track_counts_for_initialization(mut track_counts: TrackCounts, axis_reversed: bool) -> TrackCounts {
    if axis_reversed && track_counts.explicit <= 1 {
        core::mem::swap(&mut track_counts.negative_implicit, &mut track_counts.positive_implicit);
    }
    track_counts
}

/// Map initialized track indexes back to logical occupancy indexes when an
/// axis is stored in reverse physical order.
fn track_occupancy_index_for_initialization(
    track_index: usize,
    track_counts: TrackCounts,
    axis_reversed: bool,
) -> usize {
    if !axis_reversed {
        return track_index;
    }
    if track_counts.explicit <= 1 {
        return track_counts.len() - track_index - 1;
    }

    let explicit_start = track_counts.negative_implicit as usize;
    let explicit_end = explicit_start + track_counts.explicit as usize;
    if (explicit_start..explicit_end).contains(&track_index) {
        explicit_start + (explicit_end - track_index - 1)
    } else {
        track_index
    }
}

/// Information from the computation of grid
#[derive(Debug, Clone, PartialEq)]
#[cfg(feature = "detailed_layout_info")]
pub struct DetailedGridInfo {
    /// <https://drafts.csswg.org/css-grid-1/#grid-row>
    pub rows: DetailedGridTracksInfo,
    /// <https://drafts.csswg.org/css-grid-1/#grid-column>
    pub columns: DetailedGridTracksInfo,
    /// <https://drafts.csswg.org/css-grid-1/#grid-items>
    pub items: Vec<DetailedGridItemsInfo>,
}

/// Information from the computation of grids tracks
#[derive(Debug, Clone, PartialEq)]
#[cfg(feature = "detailed_layout_info")]
pub struct DetailedGridTracksInfo {
    /// Number of leading implicit grid tracks
    pub negative_implicit_tracks: u16,
    /// Number of explicit grid tracks
    pub explicit_tracks: u16,
    /// Number of trailing implicit grid tracks
    pub positive_implicit_tracks: u16,
    /// Number of expansions of the axis' `repeat(auto-fill, ...)` or
    /// `repeat(auto-fit, ...)` component.
    ///
    /// This is retained separately because `explicit_tracks` can also be
    /// enlarged by `grid-template-areas`, so the final track count cannot be
    /// used to reconstruct the auto-repeat expansion unambiguously.
    pub auto_repetitions: u16,

    /// Gutters between tracks
    pub gutters: Vec<f32>,
    /// The used size of the tracks
    pub sizes: Vec<f32>,
}

#[cfg(feature = "detailed_layout_info")]
impl DetailedGridTracksInfo {
    /// Get the base_size of [`GridTrack`] with a kind [`types::GridTrackKind`]
    #[inline(always)]
    fn grid_track_base_size_of_kind(grid_tracks: &[GridTrack], kind: GridTrackKind) -> Vec<f32> {
        grid_tracks
            .iter()
            .filter_map(|track| match track.kind == kind {
                true => Some(track.base_size),
                false => None,
            })
            .collect()
    }

    /// Get the sizes of the gutters
    fn gutters_from_grid_track_layout(grid_tracks: &[GridTrack]) -> Vec<f32> {
        DetailedGridTracksInfo::grid_track_base_size_of_kind(grid_tracks, GridTrackKind::Gutter)
    }

    /// Get the sizes of the tracks
    fn sizes_from_grid_track_layout(grid_tracks: &[GridTrack]) -> Vec<f32> {
        DetailedGridTracksInfo::grid_track_base_size_of_kind(grid_tracks, GridTrackKind::Track)
    }

    /// Construct DetailedGridTracksInfo from TrackCounts and GridTracks
    fn from_grid_tracks_and_track_count(
        track_count: TrackCounts,
        auto_repetitions: u16,
        grid_tracks: Vec<GridTrack>,
    ) -> Self {
        DetailedGridTracksInfo {
            negative_implicit_tracks: track_count.negative_implicit,
            explicit_tracks: track_count.explicit,
            positive_implicit_tracks: track_count.positive_implicit,
            auto_repetitions,
            gutters: DetailedGridTracksInfo::gutters_from_grid_track_layout(&grid_tracks),
            sizes: DetailedGridTracksInfo::sizes_from_grid_track_layout(&grid_tracks),
        }
    }
}

/// Grid area information from the placement algorithm
///
/// The values is 1-indexed grid line numbers bounding the area.
/// This matches the Chrome and Firefox's format as of 2nd Jan 2024.
#[derive(Debug, Clone, PartialEq)]
#[cfg(feature = "detailed_layout_info")]
pub struct DetailedGridItemsInfo {
    /// row-start with 1-indexed grid line numbers
    pub row_start: u16,
    /// row-end with 1-indexed grid line numbers
    pub row_end: u16,
    /// column-start with 1-indexed grid line numbers
    pub column_start: u16,
    /// column-end with 1-indexed grid line numbers
    pub column_end: u16,
}

/// Grid area information from the placement algorithm
#[cfg(feature = "detailed_layout_info")]
impl DetailedGridItemsInfo {
    /// Construct from GridItems
    #[inline(always)]
    fn from_grid_item(grid_item: &GridItem) -> Self {
        /// Conversion from the indexes of Vec<GridTrack> into 1-indexed grid line numbers. See [`GridItem::row_indexes`] or [`GridItem::column_indexes`]
        #[inline(always)]
        fn to_one_indexed_grid_line(grid_track_index: u16) -> u16 {
            grid_track_index / 2 + 1
        }

        DetailedGridItemsInfo {
            row_start: to_one_indexed_grid_line(grid_item.row_indexes.start),
            row_end: to_one_indexed_grid_line(grid_item.row_indexes.end),
            column_start: to_one_indexed_grid_line(grid_item.column_indexes.start),
            column_end: to_one_indexed_grid_line(grid_item.column_indexes.end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Style;

    #[allow(clippy::too_many_arguments)]
    fn baseline_item(
        source_order: u16,
        row_indexes: Line<u16>,
        column_indexes: Line<u16>,
        participates_in_baseline_alignment: bool,
        block_offset: f32,
        block_size: f32,
        first_baseline: Option<f32>,
        last_baseline: Option<f32>,
    ) -> GridItem {
        let style: Style =
            Style { align_self: participates_in_baseline_alignment.then_some(AlignSelf::BASELINE), ..Style::default() };
        let mut item = GridItem::new_with_placement_style_and_order(
            NodeId::new(u64::from(source_order)),
            crate::WritingDirection::new(crate::WritingMode::HorizontalTb, crate::Direction::Ltr),
            InBothAbstractAxis {
                inline: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
                block: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
            },
            style,
            InBothAbstractAxis { inline: AlignItems::STRETCH, block: AlignItems::STRETCH },
            source_order,
        );
        item.row_indexes = row_indexes;
        item.column_indexes = column_indexes;
        item.block_offset = block_offset;
        item.block_size = block_size;
        item.first_baseline = first_baseline;
        item.last_baseline = last_baseline;
        item
    }

    #[test]
    fn grid_propagates_distinct_final_child_baseline_sets() {
        let items = vec![
            baseline_item(
                0,
                Line { start: 0, end: 2 },
                Line { start: 0, end: 2 },
                false,
                10.0,
                30.0,
                Some(8.0),
                Some(24.0),
            ),
            baseline_item(
                1,
                Line { start: 2, end: 4 },
                Line { start: 0, end: 2 },
                false,
                50.0,
                40.0,
                Some(10.0),
                Some(32.0),
            ),
        ];

        assert_eq!(grid_container_baselines(&items), (18.0, 82.0));
    }

    #[test]
    fn grid_baseline_sharing_groups_take_priority_in_edge_rows() {
        let items = vec![
            baseline_item(
                0,
                Line { start: 0, end: 2 },
                Line { start: 0, end: 2 },
                false,
                0.0,
                20.0,
                Some(5.0),
                Some(15.0),
            ),
            baseline_item(
                1,
                Line { start: 0, end: 2 },
                Line { start: 2, end: 4 },
                true,
                0.0,
                24.0,
                Some(12.0),
                Some(18.0),
            ),
            baseline_item(
                2,
                Line { start: 2, end: 4 },
                Line { start: 0, end: 2 },
                false,
                40.0,
                34.0,
                Some(6.0),
                Some(28.0),
            ),
            baseline_item(
                3,
                Line { start: 2, end: 4 },
                Line { start: 2, end: 4 },
                true,
                44.0,
                30.0,
                Some(14.0),
                Some(20.0),
            ),
        ];

        assert_eq!(grid_container_baselines(&items), (12.0, 58.0));
    }
}
