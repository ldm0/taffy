//! This module is a partial implementation of the CSS Grid Level 1 specification
//! <https://www.w3.org/TR/css-grid-1>
use crate::geometry::{AbstractAxis, InBothLogicalAxes, LogicalOffset, LogicalSize, WritingDirection};
use crate::geometry::{Line, Point, Rect, Size};
use crate::style::{AlignItems, AvailableSpace, Position};
use crate::tree::{
    ChildLayoutInput, Layout, LayoutInput, LayoutOutput, LayoutPartialTreeExt, NodeId, RunMode, SizingMode,
};
use crate::util::debug::debug_log;
use crate::util::sys::{f32_max, f32_min, GridTrackVec, Vec};
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{
    style_helpers::*, AlignContent, BoxGenerationMode, BoxSizing, CoreStyle, GridContainerStyle, GridItemStyle,
    JustifyContent, LayoutGridContainer, RequestedAxis,
};
use alignment::{align_tracks, layout_item, GridPlacementContext};
use explicit_grid::{compute_explicit_grid_size_in_axis, initialize_grid_tracks, AutoRepeatStrategy};
use implicit_grid::compute_grid_size_estimate;
use placement::place_grid_items;
use track_sizing::{
    determine_if_item_crosses_flexible_or_intrinsic_tracks, resolve_item_track_indexes, track_sizing_algorithm,
};
use types::{CellOccupancyMatrix, GridItem, GridTrack, NamedLineResolver, TrackCounts};

use super::common::aspect_ratio::{resolve_node_size_constraints, SizeConstraintInput, TransferredSizesMode};
use super::common::intrinsic_size::resolve_content_based_block_constraints;
use super::common::used_size::resolve_used_size;

#[cfg(feature = "detailed_layout_info")]
use types::GridTrackKind;

pub(crate) use types::{GridCoordinate, GridLine, OriginZeroLine, MAX_GRID_TRACKS, MAX_OZ_LINE, MIN_OZ_LINE};

mod alignment;
mod baseline;
mod explicit_grid;
mod implicit_grid;
mod item_sizing;
mod placement;
mod track_sizing;
mod types;
mod util;

/// Grid layout algorithm
/// This consists of a few phases:
///   - Resolving the explicit grid
///   - Placing items (which also resolves the implicit grid)
///   - Track (row/column) sizing
///   - Alignment & Final item placement
pub fn compute_grid_layout<Tree: LayoutGridContainer>(
    tree: &mut Tree,
    node: NodeId,
    mut inputs: LayoutInput,
) -> LayoutOutput {
    let writing_mode = tree.get_writing_mode(node);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { known_dimensions, parent_size, available_space, run_mode, .. } = inputs;

    let resolved_aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let scrollbar_insets = tree.get_scrollbar_insets(node);
    let style = tree.get_grid_container_style(node);
    let direction = style.direction();
    let flow = WritingDirection::new(writing_mode, direction);

    // 1. Compute "available grid space"
    // https://www.w3.org/TR/css-grid-1/#available-grid-space
    let aspect_ratio = if inputs.sizing_mode == SizingMode::InherentSize { resolved_aspect_ratio } else { None };
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding_border = padding + border;
    let padding_border_size = padding_border.sum_axes();
    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    let mut intrinsic_dependency = false;
    let (min_size, max_size, preferred_size, preferred_inline_from_aspect_ratio) = match inputs.sizing_mode {
        SizingMode::ContentSize => {
            drop(style);
            (Size::NONE, Size::NONE, Size::NONE, false)
        }
        SizingMode::InherentSize => {
            let raw_size = style.size();
            let mut resolved = resolve_node_size_constraints(
                SizeConstraintInput {
                    size: raw_size
                        .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                        .maybe_add(box_sizing_adjustment),
                    min_size: style
                        .min_size()
                        .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                        .maybe_add(box_sizing_adjustment),
                    max_size: style
                        .max_size()
                        .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                        .maybe_add(box_sizing_adjustment),
                    size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
                    writing_mode,
                    block_auto_behavior: inputs.block_auto_behavior,
                    transferred_sizes_mode: TransferredSizesMode::Normal,
                    aspect_ratio,
                    padding_border: padding_border_size,
                },
                known_dimensions,
            );
            drop(style);
            intrinsic_dependency =
                resolve_content_based_block_constraints(tree, node, &mut inputs, &mut resolved, padding_border_size);
            (
                resolved.min_size,
                resolved.max_size,
                resolved.size,
                writing_mode.to_logical(resolved.aspect_ratio_applied).inline_size,
            )
        }
    };
    let style = tree.get_grid_container_style(node);
    let applied_aspect_ratio = run_mode == RunMode::ComputeSize
        && writing_mode.to_logical(known_dimensions).inline_size.is_none()
        && preferred_inline_from_aspect_ratio;

    let content_box_inset = padding_border + scrollbar_insets;

    let align_content = style.align_content().unwrap_or(AlignContent::STRETCH);
    let justify_content = style.justify_content().unwrap_or(JustifyContent::STRETCH);
    let align_items = style.align_items();
    let justify_items = style.justify_items();

    // Note: we avoid accessing the grid rows/columns methods more than once as this can
    // cause an expensive-ish computation
    let grid_template_columns = style.grid_template_columns();
    let grid_template_rows = style.grid_template_rows();
    let grid_auto_columns = style.grid_auto_columns();
    let grid_auto_rows = style.grid_auto_rows();

    let outer_node_size = resolve_used_size(known_dimensions, preferred_size, min_size, max_size, padding_border_size);
    // CSS sizes and tree constraints enter in physical axes. Track sizing and
    // placement remain in the grid's logical axes until fragment publication.
    let outer_node_size = writing_mode.to_logical(outer_node_size);
    let min_size = writing_mode.to_logical(min_size);
    let max_size = writing_mode.to_logical(max_size);
    let available_space = writing_mode.to_logical(available_space);
    let padding_border_size = writing_mode.to_logical(padding_border_size);
    let padding = flow.to_logical_box_strut(padding);
    let border = flow.to_logical_box_strut(border);
    let scrollbar_insets = flow.to_logical_box_strut(scrollbar_insets);
    let content_box_inset = flow.to_logical_box_strut(content_box_inset);

    let constrained_available_space = outer_node_size
        .map(|size| size.map(AvailableSpace::Definite))
        .unwrap_or(available_space.maybe_clamp(min_size, max_size).maybe_max(padding_border_size));

    let available_grid_space = LogicalSize {
        inline_size: constrained_available_space
            .inline_size
            .map_definite_value(|space| space - content_box_inset.inline_axis_sum()),
        block_size: constrained_available_space
            .block_size
            .map_definite_value(|space| space - content_box_inset.block_axis_sum()),
    };

    // The track sizing algorithm operates on the grid container's content box, so the min/max sizes
    // (which are border-box sizes) need converting to content-box sizes before being passed to it
    let inner_min_size = min_size.maybe_sub(content_box_inset.sum_axes());
    let inner_max_size = max_size.maybe_sub(content_box_inset.sum_axes());
    let mut inner_node_size = LogicalSize {
        inline_size: outer_node_size.inline_size.map(|space| space - content_box_inset.inline_axis_sum()),
        block_size: outer_node_size.block_size.map(|space| space - content_box_inset.block_axis_sum()),
    };

    debug_log!("parent_size", dbg:parent_size);
    debug_log!("outer_node_size", dbg:outer_node_size);
    debug_log!("inner_node_size", dbg:inner_node_size);

    // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
    // is ComputeSize (and thus the container's size is all that we're interested in)
    if run_mode == RunMode::ComputeSize {
        if let LogicalSize { inline_size: Some(inline_size), block_size: Some(block_size) } = outer_node_size {
            return LayoutOutput::from_outer_size(writing_mode.to_physical(LogicalSize { inline_size, block_size }))
                .with_block_constraint_dependency(intrinsic_dependency)
                .with_applied_aspect_ratio(applied_aspect_ratio);
        }

        // We can also short-circuit if the width is known and only the width has been requested.
        if inputs.axis == RequestedAxis::from(writing_mode.inline_axis()) {
            if let Some(inline_size) = outer_node_size.inline_size {
                return LayoutOutput::from_outer_size(
                    writing_mode.to_physical(LogicalSize { inline_size, block_size: 0.0 }),
                )
                .with_block_constraint_dependency(intrinsic_dependency)
                .with_applied_aspect_ratio(applied_aspect_ratio);
            }
        }
    }

    // Absolutely positioned children do not take part in grid placement and do not create
    // implicit tracks, so they are excluded from the grid size estimate.
    let get_child_styles_iter = |node| {
        tree.child_ids(node).map(|child_node: NodeId| tree.get_grid_child_style(child_node)).filter(|style| {
            style.box_generation_mode() != BoxGenerationMode::None && style.position() != Position::Absolute
        })
    };
    let child_styles_iter = get_child_styles_iter(node);

    // 2. Resolve the explicit grid

    // This is very similar to the inner_node_size except if the inner_node_size is not definite but the node
    // has a min- or max- size style then that will be used in it's place.
    let auto_fit_container_size = outer_node_size
        .or(max_size.or(min_size).maybe_clamp(min_size, max_size))
        .maybe_max(padding_border_size)
        .maybe_sub(content_box_inset.sum_axes());

    // If the grid container has a definite size or max size in the relevant axis:
    //   - then the number of repetitions is the largest possible positive integer that does not cause the grid to overflow the content
    //     box of its grid container.
    // Otherwise, if the grid container has a definite min size in the relevant axis:
    //   - then the number of repetitions is the smallest possible positive integer that fulfills that minimum requirement
    // Otherwise, the specified track list repeats only once.
    let auto_repeat_fit_strategy = outer_node_size.or(max_size).map(|val| match val {
        Some(_) => AutoRepeatStrategy::MaxRepetitionsThatDoNotOverflow,
        None => AutoRepeatStrategy::MinRepetitionsThatDoOverflow,
    });

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
        compute_grid_size_estimate(explicit_col_count, explicit_row_count, child_styles_iter);

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
        writing_mode,
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
    initialize_grid_tracks(
        &mut columns,
        final_col_counts,
        &style,
        AbstractAxis::Inline,
        col_auto_repetition_count,
        |column_index| cell_occupancy_matrix.column_is_occupied(column_index),
    );
    initialize_grid_tracks(
        &mut rows,
        final_row_counts,
        &style,
        AbstractAxis::Block,
        row_auto_repetition_count,
        |row_index| cell_occupancy_matrix.row_is_occupied(row_index),
    );

    drop(grid_template_rows);
    drop(grid_template_columns);
    drop(grid_auto_rows);
    drop(grid_auto_columns);
    drop(style);

    // 6. Track Sizing

    // Convert grid placements in origin-zero coordinates to indexes into the GridTrack (rows and columns) vectors
    // This computation is relatively trivial, but it requires the final number of negative (implicit) tracks in
    // each axis, and doing it up-front here means we don't have to keep repeating that calculation
    resolve_item_track_indexes(&mut items, final_col_counts, final_row_counts);
    // For each item, and in each axis, determine whether the item crosses any flexible (fr) tracks
    // Record this as a boolean (per-axis) on each item for later use in the track-sizing algorithm
    determine_if_item_crosses_flexible_or_intrinsic_tracks(&mut items, &columns, &rows);

    // Determine if the grid has any baseline aligned items
    let baseline_context = items
        .iter()
        .any(|item| {
            item.align_self.baseline_preference().is_some() || item.justify_self.baseline_preference().is_some()
        })
        .then(|| baseline::GridBaselineContext::new(tree, node));

    // Run track sizing algorithm for Inline axis
    track_sizing_algorithm(
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
        baseline_context,
    );
    let initial_column_sum = columns.iter().map(|track| track.base_size).sum::<f32>();
    inner_node_size.inline_size = inner_node_size.inline_size.or_else(|| initial_column_sum.into());

    items.iter_mut().for_each(|item| item.grid_area_size_cache = None);

    // Run track sizing algorithm for Block axis
    track_sizing_algorithm(
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
        baseline_context,
    );
    let initial_row_sum = rows.iter().map(|track| track.base_size).sum::<f32>();
    inner_node_size.block_size = inner_node_size.block_size.or_else(|| initial_row_sum.into());

    debug_log!("initial_column_sum", dbg:initial_column_sum);
    debug_log!(dbg: columns.iter().map(|track| track.base_size).collect::<Vec<_>>());
    debug_log!("initial_row_sum", dbg:initial_row_sum);
    debug_log!(dbg: rows.iter().map(|track| track.base_size).collect::<Vec<_>>());

    // 6. Compute container size
    // The initial sizing boundary has already resolved fixed/preferred axes.
    // Only a still-content-sized axis may be clamped from its track sum. Keep
    // that ownership unchanged when intrinsic track sizing is re-run below.
    let resolve_container_size = |tracks: LogicalSize<f32>| LogicalSize {
        inline_size: outer_node_size.inline_size.unwrap_or_else(|| {
            (tracks.inline_size + content_box_inset.inline_axis_sum())
                .maybe_clamp(min_size.inline_size, max_size.inline_size)
                .max(padding_border_size.inline_size)
        }),
        block_size: outer_node_size.block_size.unwrap_or_else(|| {
            (tracks.block_size + content_box_inset.block_axis_sum())
                .maybe_clamp(min_size.block_size, max_size.block_size)
                .max(padding_border_size.block_size)
        }),
    };
    let mut container_border_box =
        resolve_container_size(LogicalSize { inline_size: initial_column_sum, block_size: initial_row_sum });
    let mut container_content_box = LogicalSize {
        inline_size: f32_max(0.0, container_border_box.inline_size - content_box_inset.inline_axis_sum()),
        block_size: f32_max(0.0, container_border_box.block_size - content_box_inset.block_axis_sum()),
    };

    // If only the container's size has been requested
    if run_mode == RunMode::ComputeSize {
        let depends_on_block_constraints =
            intrinsic_dependency || items.iter().any(|item| item.depends_on_block_constraints);
        return LayoutOutput::from_outer_size(writing_mode.to_physical(container_border_box))
            .with_block_constraint_dependency(depends_on_block_constraints)
            .with_applied_aspect_ratio(applied_aspect_ratio);
    }

    // 7. Resolve percentage track base sizes
    // In the case of an indefinitely sized container these resolve to zero during the "Initialise Tracks" step
    // and therefore need to be re-resolved here based on the content-sized content box of the container
    if !available_grid_space.inline_size.is_definite() {
        for column in &mut columns {
            let min: Option<f32> = column
                .min_track_sizing_function
                .resolved_percentage_size(container_content_box.inline_size, |val, basis| tree.calc(val, basis));
            let max: Option<f32> = column
                .max_track_sizing_function
                .resolved_percentage_size(container_content_box.inline_size, |val, basis| tree.calc(val, basis));
            column.base_size = column.base_size.maybe_clamp(min, max);
        }
    }
    if !available_grid_space.block_size.is_definite() {
        for row in &mut rows {
            let min: Option<f32> = row
                .min_track_sizing_function
                .resolved_percentage_size(container_content_box.block_size, |val, basis| tree.calc(val, basis));
            let max: Option<f32> = row
                .max_track_sizing_function
                .resolved_percentage_size(container_content_box.block_size, |val, basis| tree.calc(val, basis));
            row.base_size = row.base_size.maybe_clamp(min, max);
        }
    }

    // Column sizing must be re-run (once) if:
    //   - The grid container's width was initially indefinite and there are any columns with percentage track sizing functions
    //   - Any grid item crossing an intrinsically sized track's min content contribution width has changed
    // TODO: Only rerun sizing for tracks that actually require it rather than for all tracks if any need it.
    let mut rerun_column_sizing;
    let mut intrinsic_column_contribution_changed = false;

    let has_percentage_column = columns.iter().any(|track| track.uses_percentage());
    let has_percentage_row = rows.iter().any(|track| track.uses_percentage());
    let parent_width_indefinite = !available_space.inline_size.is_definite();
    rerun_column_sizing = parent_width_indefinite && has_percentage_column;

    if !rerun_column_sizing {
        intrinsic_column_contribution_changed =
            items.iter_mut().filter(|item| item.crosses_intrinsic_column).any(|item| {
                let grid_area_size = item.grid_area_size(
                    AbstractAxis::Inline,
                    &columns,
                    &rows,
                    inner_node_size,
                    |track: &GridTrack, _| Some(track.base_size),
                    &|val, basis| tree.calc(val, basis),
                );
                let available_space = grid_area_size.with(AbstractAxis::Inline, None);
                let new_min_content_contribution =
                    item.min_content_contribution(AbstractAxis::Inline, tree, grid_area_size, available_space);

                let has_changed = Some(new_min_content_contribution) != item.min_content_contribution_cache.inline_size;

                item.grid_area_size_cache = Some(grid_area_size);
                item.min_content_contribution_cache.inline_size = Some(new_min_content_contribution);
                item.max_content_contribution_cache.inline_size = None;
                item.minimum_contribution_cache.inline_size = None;

                has_changed
            });
        rerun_column_sizing = intrinsic_column_contribution_changed;
    } else {
        // Clear intrinsic width caches
        items.iter_mut().for_each(|item| {
            item.grid_area_size_cache = None;
            item.min_content_contribution_cache.inline_size = None;
            item.max_content_contribution_cache.inline_size = None;
            item.minimum_contribution_cache.inline_size = None;
        });
    }

    let mut intrinsic_row_contribution_changed = false;

    if rerun_column_sizing {
        // Re-run track sizing algorithm for Inline axis
        track_sizing_algorithm(
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
            |track: &GridTrack, _, _| Some(track.base_size),
            baseline_context,
        );

        // Row sizing must be re-run (once) if:
        //   - The grid container's height was initially indefinite and there are any rows with percentage track sizing functions
        //   - Any grid item crossing an intrinsically sized track's min content contribution height has changed
        // TODO: Only rerun sizing for tracks that actually require it rather than for all tracks if any need it.
        let mut rerun_row_sizing;

        let parent_height_indefinite = !available_space.block_size.is_definite();
        rerun_row_sizing = parent_height_indefinite && has_percentage_row;

        if !rerun_row_sizing {
            intrinsic_row_contribution_changed =
                items.iter_mut().filter(|item| item.crosses_intrinsic_column).any(|item| {
                    let grid_area_size = item.grid_area_size(
                        AbstractAxis::Block,
                        &rows,
                        &columns,
                        inner_node_size,
                        |track: &GridTrack, _| Some(track.base_size),
                        &|val, basis| tree.calc(val, basis),
                    );
                    let available_space = grid_area_size.with(AbstractAxis::Block, None);
                    let new_min_content_contribution =
                        item.min_content_contribution(AbstractAxis::Block, tree, grid_area_size, available_space);

                    let has_changed =
                        Some(new_min_content_contribution) != item.min_content_contribution_cache.block_size;

                    item.grid_area_size_cache = Some(grid_area_size);
                    item.min_content_contribution_cache.block_size = Some(new_min_content_contribution);
                    item.max_content_contribution_cache.block_size = None;
                    item.minimum_contribution_cache.block_size = None;

                    has_changed
                });
            rerun_row_sizing = intrinsic_row_contribution_changed;
        } else {
            items.iter_mut().for_each(|item| {
                // Clear intrinsic height caches
                item.grid_area_size_cache = None;
                item.min_content_contribution_cache.block_size = None;
                item.max_content_contribution_cache.block_size = None;
                item.minimum_contribution_cache.block_size = None;
            });
        }

        if rerun_row_sizing {
            // Re-run track sizing algorithm for Block axis
            track_sizing_algorithm(
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
                baseline_context,
            );
        }
    }

    if (intrinsic_column_contribution_changed && !has_percentage_column)
        || (intrinsic_row_contribution_changed && !has_percentage_row)
    {
        let final_column_sum = columns.iter().map(|track| track.base_size).sum::<f32>();
        let final_row_sum = rows.iter().map(|track| track.base_size).sum::<f32>();
        let final_border_box =
            resolve_container_size(LogicalSize { inline_size: final_column_sum, block_size: final_row_sum });

        if intrinsic_column_contribution_changed && !has_percentage_column {
            container_border_box.inline_size = final_border_box.inline_size;
            container_content_box.inline_size =
                f32_max(0.0, container_border_box.inline_size - content_box_inset.inline_axis_sum());
        }

        if intrinsic_row_contribution_changed && !has_percentage_row {
            container_border_box.block_size = final_border_box.block_size;
            container_content_box.block_size =
                f32_max(0.0, container_border_box.block_size - content_box_inset.block_axis_sum());
        }
    }

    // If only the container's size has been requested
    if run_mode == RunMode::ComputeSize {
        let depends_on_block_constraints =
            intrinsic_dependency || items.iter().any(|item| item.depends_on_block_constraints);
        return LayoutOutput::from_outer_size(writing_mode.to_physical(container_border_box))
            .with_block_constraint_dependency(depends_on_block_constraints)
            .with_applied_aspect_ratio(applied_aspect_ratio);
    }

    // 8. Track Alignment

    // Align columns
    let inline_size_without_scrollbar =
        f32_max(container_border_box.inline_size - padding_border_size.inline_size, 0.0);
    let inline_scrollbar_scale = {
        let total = scrollbar_insets.inline_axis_sum();
        if total > 0.0 {
            f32_min(1.0, inline_size_without_scrollbar / total)
        } else {
            1.0
        }
    };
    align_tracks(
        container_content_box.get(AbstractAxis::Inline),
        Line {
            start: padding.inline_start + scrollbar_insets.inline_start * inline_scrollbar_scale,
            end: padding.inline_end + scrollbar_insets.inline_end * inline_scrollbar_scale,
        },
        Line { start: border.inline_start, end: border.inline_end },
        &mut columns,
        justify_content,
    );
    // Align rows
    let block_size_without_scrollbar = f32_max(container_border_box.block_size - padding_border_size.block_size, 0.0);
    let block_scrollbar_scale = {
        let total = scrollbar_insets.block_axis_sum();
        if total > 0.0 {
            f32_min(1.0, block_size_without_scrollbar / total)
        } else {
            1.0
        }
    };
    align_tracks(
        container_content_box.get(AbstractAxis::Block),
        Line {
            start: padding.block_start + scrollbar_insets.block_start * block_scrollbar_scale,
            end: padding.block_end + scrollbar_insets.block_end * block_scrollbar_scale,
        },
        Line { start: border.block_start, end: border.block_end },
        &mut rows,
        align_content,
    );

    // 9. Size, Align, and Position Grid Items

    let container_border_box = writing_mode.to_physical(container_border_box);
    let border = flow.to_physical_box_strut(border);
    let scrollbar_insets = flow.to_physical_box_strut(scrollbar_insets);

    let placement_context = GridPlacementContext {
        flow,
        outer_size: container_border_box,
        border_scrollbar: border + scrollbar_insets,
        baseline_type: tree.get_baseline_type(node),
    };
    #[cfg_attr(not(feature = "content_size"), allow(unused_mut))]
    let mut item_content_size_contribution = LogicalSize::ZERO;
    #[cfg_attr(not(feature = "content_size"), allow(unused_mut, unused))]
    let mut absolute_content_size = LogicalSize::ZERO;

    // Sort items back into original order to allow them to be matched up with styles
    items.sort_by_key(|item| item.source_order);

    let container_alignment_styles = InBothLogicalAxes { inline: justify_items, block: align_items };

    // Finish child layout before collecting the grid's final baseline groups.
    // Intrinsic sizing shims must not escape into used author margins.
    let mut item_layouts: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let grid_area = physical_grid_area(
                flow,
                container_border_box,
                Line {
                    start: columns[item.column_indexes.start as usize + 1].offset,
                    end: columns[item.column_indexes.end as usize].offset,
                },
                Line {
                    start: rows[item.row_indexes.start as usize + 1].offset,
                    end: rows[item.row_indexes.end as usize].offset,
                },
            );
            layout_item(tree, item.node, index as u32, grid_area, container_alignment_styles, direction, writing_mode)
        })
        .collect();
    if let Some(context) = baseline_context {
        baseline::align_final_baselines(tree, context, &mut items, &mut item_layouts);
    }
    for (item, layout) in items.iter_mut().zip(item_layouts) {
        let placement = layout.place(tree, item.node, placement_context);
        item.block_axis_origin = placement.block_axis_origin;
        item.synthesized_baseline = placement.synthesized_baseline;
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
                ),
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
                .map(|line| line.and_then(|line| line.try_into_track_vec_index(final_col_counts)));
            // Convert grid-row-{start/end} into Option's of indexes into the row vector
            // The Option is None if the style property is Auto and an unresolvable Span
            let maybe_row_indexes = name_resolver
                .resolve_row_names(&child_style.grid_row())
                .into_origin_zero(final_row_counts.explicit)
                .resolve_absolutely_positioned_grid_tracks()
                .map(|maybe_grid_line| {
                    maybe_grid_line.and_then(|line: OriginZeroLine| line.try_into_track_vec_index(final_row_counts))
                });

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

            let logical_border = flow.to_logical_box_strut(border + scrollbar_insets);
            let logical_size = writing_mode.to_logical(container_border_box);
            let grid_area = physical_grid_area(
                flow,
                container_border_box,
                Line {
                    start: maybe_col_indexes
                        .start
                        .map(|index| line_as_start_edge(&columns, index))
                        .unwrap_or(logical_border.inline_start),
                    end: maybe_col_indexes
                        .end
                        .map(|index| line_as_end_edge(&columns, index))
                        .unwrap_or(logical_size.inline_size - logical_border.inline_end),
                },
                Line {
                    start: maybe_row_indexes
                        .start
                        .map(|index| line_as_start_edge(&rows, index))
                        .unwrap_or(logical_border.block_start),
                    end: maybe_row_indexes
                        .end
                        .map(|index| line_as_end_edge(&rows, index))
                        .unwrap_or(logical_size.block_size - logical_border.block_end),
                },
            );
            drop(child_style);

            // Out-of-flow items do not participate in grid baseline groups.
            #[cfg_attr(not(feature = "content_size"), allow(unused_variables))]
            let placement =
                layout_item(tree, child, order, grid_area, container_alignment_styles, direction, writing_mode).place(
                    tree,
                    child,
                    placement_context,
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

    // The container's own padding at the end of the content is part of its scrollable
    // overflow region, so it is included in the in-flow content size.
    #[cfg(feature = "content_size")]
    let content_size = {
        let mut content_size = item_content_size_contribution;
        content_size.inline_size += padding.inline_end;
        content_size.block_size += padding.block_end;
        content_size.f32_max(absolute_content_size)
    };
    #[cfg(not(feature = "content_size"))]
    let content_size = item_content_size_contribution;

    let (first_baselines, last_baselines) = if items.is_empty() {
        (Point::NONE, Point::NONE)
    } else {
        let (first, last) = grid_container_baselines(&items, flow);
        match writing_mode.block_axis() {
            crate::AbsoluteAxis::Horizontal => (Point { x: Some(first), y: None }, Point { x: Some(last), y: None }),
            crate::AbsoluteAxis::Vertical => (Point { x: None, y: Some(first) }, Point { x: None, y: Some(last) }),
        }
    };
    LayoutOutput::from_sizes_and_baseline_sets(
        container_border_box,
        writing_mode.to_physical(content_size),
        first_baselines,
        last_baselines,
    )
    .with_block_constraint_dependency(
        intrinsic_dependency || items.iter().any(|item| item.depends_on_block_constraints),
    )
}

/// Select the grid container's baselines from final item fragments.
///
/// Export follows the final participating group in a logical edge row, then
/// a child baseline in grid order. Returned offsets remain physical.
fn grid_container_baselines(items: &[GridItem], flow: WritingDirection) -> (f32, f32) {
    baseline::container_baselines(items, flow)
}

/// Project a resolved logical grid area once at the child-fragment boundary.
fn physical_grid_area(
    flow: WritingDirection,
    container_size: Size<f32>,
    inline: Line<f32>,
    block: Line<f32>,
) -> Rect<f32> {
    let size = flow
        .mode
        .to_physical(LogicalSize { inline_size: inline.end - inline.start, block_size: block.end - block.start });
    let origin = flow
        .converter(container_size)
        .to_physical_point(LogicalOffset { inline_offset: inline.start, block_offset: block.start }, size);
    Rect { left: origin.x, right: origin.x + size.width, top: origin.y, bottom: origin.y + size.height }
}

/// Used grid tracks and item placements in logical grid order.
///
/// Columns run from inline-start to inline-end; rows run from block-start to
/// block-end. Track arrays and item line indices are not reordered into
/// physical left-to-right or top-to-bottom order for RTL or vertical grids.
#[derive(Debug, Clone, PartialEq)]
#[cfg(feature = "detailed_layout_info")]
pub struct DetailedGridInfo {
    /// Tracks along the block axis: <https://drafts.csswg.org/css-grid-1/#grid-row>.
    pub rows: DetailedGridTracksInfo,
    /// Tracks along the inline axis: <https://drafts.csswg.org/css-grid-1/#grid-column>.
    pub columns: DetailedGridTracksInfo,
    /// <https://drafts.csswg.org/css-grid-1/#grid-items>
    pub items: Vec<DetailedGridItemsInfo>,
}

/// Used tracks in logical order, including leading and trailing implicit tracks.
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
    use crate::{AlignSelf, Style};

    #[allow(clippy::too_many_arguments)]
    fn baseline_item(
        source_order: u16,
        row_indexes: Line<u16>,
        column_indexes: Line<u16>,
        participates_in_baseline_alignment: bool,
        y_position: f32,
        height: f32,
        first_baseline: Option<f32>,
        last_baseline: Option<f32>,
    ) -> GridItem {
        let style: Style =
            Style { align_self: participates_in_baseline_alignment.then_some(AlignSelf::BASELINE), ..Style::default() };
        let mut item = GridItem::new_with_placement_style_and_order(
            NodeId::new(u64::from(source_order)),
            crate::WritingMode::HorizontalTb,
            InBothLogicalAxes {
                inline: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
                block: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
            },
            style,
            InBothLogicalAxes { inline: AlignItems::STRETCH, block: AlignItems::STRETCH },
            source_order,
        );
        item.row_indexes = row_indexes;
        item.column_indexes = column_indexes;
        item.block_axis_origin = y_position;
        item.synthesized_baseline = height;
        item.first_baseline = first_baseline;
        item.last_baseline = last_baseline;
        if participates_in_baseline_alignment {
            // These selector tests provide the result of group collection,
            // not just a requested align-self value (which might be ineligible).
            let position = first_baseline.unwrap_or(height);
            item.alignment_baselines.block = Some(baseline::GridItemBaseline {
                position,
                metrics: crate::compute::common::baseline::BaselineMetrics {
                    side: crate::compute::common::baseline::BaselineSide::Min,
                    ascent: position,
                    descent: height - position,
                },
            });
        }
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

        assert_eq!(grid_container_baselines(&items, WritingDirection::default()), (18.0, 82.0));
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

        assert_eq!(grid_container_baselines(&items, WritingDirection::default()), (12.0, 58.0));
    }
}
