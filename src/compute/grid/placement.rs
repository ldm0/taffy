//! Places items in flow-relative grid tracks and resolves the implicit grid.
//! <https://www.w3.org/TR/css-grid-1/#placement>
//!
//! Line numbers and occupancy are logical: they increase from inline/block
//! start in every writing mode. Direction is applied only when final fragments
//! are converted to physical coordinates, not by mirroring placements.
use super::types::{CellOccupancyMatrix, CellOccupancyState, GridItem};
use super::{NamedLineResolver, OriginZeroLine, MAX_OZ_LINE, MIN_OZ_LINE};
use crate::geometry::{AbstractAxis, InBothLogicalAxes, Line};
use crate::style::{AlignItems, GridAutoFlow, OriginZeroGridPlacement};
use crate::tree::NodeId;
use crate::util::sys::Vec;
use crate::{CoreStyle, GridItemStyle, WritingMode};

/// Advance one logical track without overflowing the coordinate representation.
fn advance_position(position: OriginZeroLine) -> OriginZeroLine {
    OriginZeroLine(position.0.saturating_add(1))
}

/// Resolve an indefinite span toward logical end.
fn resolve_indefinite_grid_span(position: OriginZeroLine, span: u16) -> Line<OriginZeroLine> {
    Line {
        start: position,
        end: OriginZeroLine((i32::from(position.0) + i32::from(span)).min(i32::from(i16::MAX)) as i16),
    }
}

/// CSS Grid §8.5: resolve placement without any physical-axis assumptions.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_grid_items<'a, S, ChildIter>(
    cell_occupancy_matrix: &mut CellOccupancyMatrix,
    items: &mut Vec<GridItem>,
    children_iter: impl Fn() -> ChildIter,
    parent_writing_mode: WritingMode,
    grid_auto_flow: GridAutoFlow,
    align_items: AlignItems,
    justify_items: AlignItems,
    named_line_resolver: &NamedLineResolver<<S as CoreStyle>::CustomIdent>,
) where
    S: GridItemStyle + 'a,
    ChildIter: Iterator<Item = (usize, NodeId, S)>,
{
    let primary_axis = grid_auto_flow.primary_axis();
    let secondary_axis = primary_axis.other();
    let explicit_col_count = cell_occupancy_matrix.track_counts(AbstractAxis::Inline).explicit;
    let explicit_row_count = cell_occupancy_matrix.track_counts(AbstractAxis::Block).explicit;
    let map_placement = |(index, node, style): (usize, NodeId, S)| {
        let placement = InBothLogicalAxes {
            inline: named_line_resolver
                .resolve_column_names(&style.grid_column())
                .map(|value| value.into_origin_zero_placement(explicit_col_count)),
            block: named_line_resolver
                .resolve_row_names(&style.grid_row())
                .map(|value| value.into_origin_zero_placement(explicit_row_count)),
        };
        (index, node, placement, style)
    };

    // 1. Place items with definite positions in both axes.
    for (index, node, placement, style) in children_iter()
        .map(map_placement)
        .filter(|(_, _, placement, _)| placement.inline.is_definite() && placement.block.is_definite())
    {
        record_grid_placement(
            cell_occupancy_matrix,
            items,
            node,
            index,
            style,
            parent_writing_mode,
            align_items,
            justify_items,
            primary_axis,
            placement.get(primary_axis).resolve_definite_grid_lines(),
            placement.get(secondary_axis).resolve_definite_grid_lines(),
            CellOccupancyState::DefinitelyPlaced,
        );
    }

    // 2. Place items locked to a row (or column for column auto-flow).
    for (index, node, placement, style) in children_iter().map(map_placement).filter(|(_, _, placement, _)| {
        placement.get(secondary_axis).is_definite() && !placement.get(primary_axis).is_definite()
    }) {
        let (primary_span, secondary_span) =
            place_definite_secondary_axis_item(cell_occupancy_matrix, placement, grid_auto_flow);
        record_grid_placement(
            cell_occupancy_matrix,
            items,
            node,
            index,
            style,
            parent_writing_mode,
            align_items,
            justify_items,
            primary_axis,
            primary_span,
            secondary_span,
            CellOccupancyState::AutoPlaced,
        );
    }

    // 3. The conservative estimate and recorded definite placements have
    // already expanded the implicit grid to accommodate definite positions
    // and the largest span of an otherwise unpositioned item.

    // 4. Place remaining items using the logical auto-placement cursor.
    let grid_start = (
        cell_occupancy_matrix.track_counts(primary_axis).implicit_start_line(),
        cell_occupancy_matrix.track_counts(secondary_axis).implicit_start_line(),
    );
    let mut cursor = grid_start;
    for (index, node, placement, style) in
        children_iter().map(map_placement).filter(|(_, _, placement, _)| !placement.get(secondary_axis).is_definite())
    {
        let (primary_span, secondary_span) =
            place_indefinitely_positioned_item(cell_occupancy_matrix, placement, grid_auto_flow, cursor);
        record_grid_placement(
            cell_occupancy_matrix,
            items,
            node,
            index,
            style,
            parent_writing_mode,
            align_items,
            justify_items,
            primary_axis,
            primary_span,
            secondary_span,
            CellOccupancyState::AutoPlaced,
        );
        cursor = if grid_auto_flow.is_dense() { grid_start } else { (primary_span.end, secondary_span.start) };
    }
}

/// Place an item with a definite secondary-axis position.
fn place_definite_secondary_axis_item(
    occupancy: &CellOccupancyMatrix,
    placement: InBothLogicalAxes<Line<OriginZeroGridPlacement>>,
    auto_flow: GridAutoFlow,
) -> (Line<OriginZeroLine>, Line<OriginZeroLine>) {
    let primary_axis = auto_flow.primary_axis();
    let primary_start = occupancy.track_counts(primary_axis).implicit_start_line();
    let secondary_span = placement.get(primary_axis.other()).resolve_definite_grid_lines();
    let mut position = if auto_flow.is_dense() {
        primary_start
    } else {
        occupancy
            .last_of_type(primary_axis, secondary_span.start, CellOccupancyState::AutoPlaced)
            .unwrap_or(primary_start)
    };
    let span = placement.get(primary_axis).indefinite_span();
    loop {
        let primary_span = resolve_indefinite_grid_span(position, span);
        match occupancy.line_area_collision_jump(primary_axis, primary_span, secondary_span) {
            None => return (primary_span, secondary_span),
            Some(next) => position = next,
        }
    }
}

/// Place an item whose secondary-axis position is automatic.
fn place_indefinitely_positioned_item(
    occupancy: &CellOccupancyMatrix,
    placement: InBothLogicalAxes<Line<OriginZeroGridPlacement>>,
    auto_flow: GridAutoFlow,
    cursor: (OriginZeroLine, OriginZeroLine),
) -> (Line<OriginZeroLine>, Line<OriginZeroLine>) {
    let primary_axis = auto_flow.primary_axis();
    let secondary_axis = primary_axis.other();
    let primary_style = placement.get(primary_axis);
    let secondary_span = placement.get(secondary_axis).indefinite_span();
    let primary_start = occupancy.track_counts(primary_axis).implicit_start_line();
    let primary_end = occupancy.track_counts(primary_axis).implicit_end_line();
    let secondary_start = occupancy.track_counts(secondary_axis).implicit_start_line();
    let (mut primary_idx, mut secondary_idx) = cursor;

    if primary_style.is_definite() {
        let primary_span = primary_style.resolve_definite_grid_lines();
        secondary_idx = if auto_flow.is_dense() {
            secondary_start
        } else if primary_span.start < primary_idx {
            advance_position(secondary_idx)
        } else {
            secondary_idx
        };
        loop {
            let secondary_span = resolve_indefinite_grid_span(secondary_idx, secondary_span);
            match occupancy.line_area_collision_jump(secondary_axis, secondary_span, primary_span) {
                None => return (primary_span, secondary_span),
                Some(next) => secondary_idx = next,
            }
        }
    }

    let primary_span = primary_style.indefinite_span();
    let spans_all_primary_tracks = usize::from(primary_span) >= occupancy.track_counts(primary_axis).len();
    loop {
        let primary_span = resolve_indefinite_grid_span(primary_idx, primary_span);
        let secondary_span = resolve_indefinite_grid_span(secondary_idx, secondary_span);
        if primary_span.end > primary_end {
            secondary_idx = advance_position(secondary_idx);
            primary_idx = primary_start;
            continue;
        }
        // A full-width item fits only in an entirely unoccupied stripe.
        if spans_all_primary_tracks {
            match occupancy.occupied_track_jump(secondary_axis, secondary_span) {
                None => return (primary_span, secondary_span),
                Some(next) => {
                    secondary_idx = next;
                    primary_idx = primary_start;
                    continue;
                }
            }
        }
        match occupancy.line_area_collision_jump(primary_axis, primary_span, secondary_span) {
            None => return (primary_span, secondary_span),
            Some(next) => primary_idx = next,
        }
    }
}

/// Clamp into the limited logical grid, preserving at least one track.
/// <https://www.w3.org/TR/css-grid-1/#overlarge-grids>
fn clamp_span_to_limited_grid(span: Line<OriginZeroLine>) -> Line<OriginZeroLine> {
    let start = span.start.0.clamp(MIN_OZ_LINE, MAX_OZ_LINE - 1);
    let end = span.end.0.clamp(start + 1, MAX_OZ_LINE);
    Line { start: OriginZeroLine(start), end: OriginZeroLine(end) }
}

/// Publish placement to both occupancy and the grid's logical item list.
#[allow(clippy::too_many_arguments)]
fn record_grid_placement<S: GridItemStyle>(
    occupancy: &mut CellOccupancyMatrix,
    items: &mut Vec<GridItem>,
    node: NodeId,
    index: usize,
    style: S,
    parent_writing_mode: WritingMode,
    parent_align_items: AlignItems,
    parent_justify_items: AlignItems,
    primary_axis: AbstractAxis,
    primary_span: Line<OriginZeroLine>,
    secondary_span: Line<OriginZeroLine>,
    placement_type: CellOccupancyState,
) {
    let primary_span = clamp_span_to_limited_grid(primary_span);
    let secondary_span = clamp_span_to_limited_grid(secondary_span);
    occupancy.mark_area_as(primary_axis, primary_span, secondary_span, placement_type);
    let (column, row) = match primary_axis {
        AbstractAxis::Inline => (primary_span, secondary_span),
        AbstractAxis::Block => (secondary_span, primary_span),
    };
    items.push(GridItem::new_with_placement_style_and_order(
        node,
        parent_writing_mode,
        InBothLogicalAxes { inline: column, block: row },
        style,
        InBothLogicalAxes { inline: parent_justify_items, block: parent_align_items },
        index as u16,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_placement_algorithm {
        use crate::compute::grid::implicit_grid::compute_grid_size_estimate;
        use crate::compute::grid::types::TrackCounts;
        use crate::compute::grid::util::*;
        use crate::compute::grid::CellOccupancyMatrix;
        use crate::compute::grid::NamedLineResolver;
        use crate::compute::grid::OriginZeroLine;
        use crate::prelude::*;
        use crate::style::GridAutoFlow;

        use super::super::place_grid_items;

        type ExpectedPlacement = (i16, i16, i16, i16);

        fn placement_test_runner(
            explicit_col_count: u16,
            explicit_row_count: u16,
            children: Vec<(usize, Style, ExpectedPlacement)>,
            expected_col_counts: TrackCounts,
            expected_row_counts: TrackCounts,
            flow: GridAutoFlow,
        ) {
            // Setup test
            let children_iter = || children.iter().map(|(index, style, _)| (*index, NodeId::from(*index), style));
            let child_styles_iter = children.iter().map(|(_, style, _)| style);
            let estimated_sizes = compute_grid_size_estimate(explicit_col_count, explicit_row_count, child_styles_iter);
            let mut items = Vec::new();
            let mut cell_occupancy_matrix =
                CellOccupancyMatrix::with_track_counts(estimated_sizes.0, estimated_sizes.1);
            let mut name_resolver = NamedLineResolver::new(&Style::DEFAULT, 0, 0);
            name_resolver.set_explicit_column_count(explicit_col_count);
            name_resolver.set_explicit_row_count(explicit_row_count);

            // Run placement algorithm
            place_grid_items(
                &mut cell_occupancy_matrix,
                &mut items,
                children_iter,
                crate::WritingMode::HorizontalTb,
                flow,
                AlignSelf::START,
                AlignSelf::START,
                // TODO: actually test named line resolution
                &name_resolver,
            );

            // Assert that each item has been placed in the right location
            let mut sorted_children = children.clone();
            sorted_children.sort_by_key(|child| child.0);
            for (idx, ((id, _style, expected_placement), item)) in sorted_children.iter().zip(items.iter()).enumerate()
            {
                assert_eq!(item.node, NodeId::from(*id));
                let actual_placement = (item.column.start, item.column.end, item.row.start, item.row.end);
                assert_eq!(actual_placement, (*expected_placement).into_oz(), "Item {idx} (0-indexed)");
            }

            // Assert that the correct number of implicit rows have been generated
            let actual_row_counts = *cell_occupancy_matrix.track_counts(crate::compute::grid::AbstractAxis::Block);
            assert_eq!(actual_row_counts, expected_row_counts, "row track counts");
            let actual_col_counts = *cell_occupancy_matrix.track_counts(crate::compute::grid::AbstractAxis::Inline);
            assert_eq!(actual_col_counts, expected_col_counts, "column track counts");
        }

        #[test]
        fn test_only_fixed_placement() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                vec![
                    // node, style (grid coords), expected_placement (oz coords)
                    (1, (line(1), auto(), line(1), auto()).into_grid_child(), (0, 1, 0, 1)),
                    (2, (line(-4), auto(), line(-3), auto()).into_grid_child(), (-1, 0, 0, 1)),
                    (3, (line(-3), auto(), line(-4), auto()).into_grid_child(), (0, 1, -1, 0)),
                    (4, (line(3), span(2), line(5), auto()).into_grid_child(), (2, 4, 4, 5)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 1, explicit: 2, positive_implicit: 2 };
            let expected_rows = TrackCounts { negative_implicit: 1, explicit: 2, positive_implicit: 3 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_placement_spanning_origin() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                vec![
                    // node, style (grid coords), expected_placement (oz coords)
                    (1, (line(-1), line(-1), line(-1), line(-1)).into_grid_child(), (2, 3, 2, 3)),
                    (2, (line(-1), span(2), line(-1), span(2)).into_grid_child(), (2, 4, 2, 4)),
                    (3, (line(-4), line(-4), line(-4), line(-4)).into_grid_child(), (-1, 0, -1, 0)),
                    (4, (line(-4), span(2), line(-4), span(2)).into_grid_child(), (-1, 1, -1, 1)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 1, explicit: 2, positive_implicit: 2 };
            let expected_rows = TrackCounts { negative_implicit: 1, explicit: 2, positive_implicit: 2 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_only_auto_placement_row_flow() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                let auto_child = (auto(), auto(), auto(), auto()).into_grid_child();
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, auto_child.clone(), (0, 1, 0, 1)),
                    (2, auto_child.clone(), (1, 2, 0, 1)),
                    (3, auto_child.clone(), (0, 1, 1, 2)),
                    (4, auto_child.clone(), (1, 2, 1, 2)),
                    (5, auto_child.clone(), (0, 1, 2, 3)),
                    (6, auto_child.clone(), (1, 2, 2, 3)),
                    (7, auto_child.clone(), (0, 1, 3, 4)),
                    (8, auto_child.clone(), (1, 2, 3, 4)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 0 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 2 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_only_auto_placement_column_flow() {
            let flow = GridAutoFlow::Column;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                let auto_child = (auto(), auto(), auto(), auto()).into_grid_child();
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, auto_child.clone(), (0, 1, 0, 1)),
                    (2, auto_child.clone(), (0, 1, 1, 2)),
                    (3, auto_child.clone(), (1, 2, 0, 1)),
                    (4, auto_child.clone(), (1, 2, 1, 2)),
                    (5, auto_child.clone(), (2, 3, 0, 1)),
                    (6, auto_child.clone(), (2, 3, 1, 2)),
                    (7, auto_child.clone(), (3, 4, 0, 1)),
                    (8, auto_child.clone(), (3, 4, 1, 2)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 2 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 0 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_oversized_item() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, (span(5), auto(), auto(), auto()).into_grid_child(), (0, 5, 0, 1)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 3 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 0 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_fixed_in_secondary_axis() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, (span(2), auto(), line(1), auto()).into_grid_child(), (0, 2, 0, 1)),
                    (2, (auto(), auto(), line(2), auto()).into_grid_child(), (0, 1, 1, 2)),
                    (3, (auto(), auto(), line(1), auto()).into_grid_child(), (2, 3, 0, 1)),
                    (4, (auto(), auto(), line(4), auto()).into_grid_child(), (0, 1, 3, 4)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 1 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 2 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_definite_in_secondary_axis_with_fully_definite_negative() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (2, (auto(), auto(), line(2), auto()).into_grid_child(), (0, 1, 1, 2)),
                    (1, (line(-4), auto(), line(2), auto()).into_grid_child(), (-1, 0, 1, 2)),
                    (3, (auto(), auto(), line(1), auto()).into_grid_child(), (-1, 0, 0, 1)),
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 1, explicit: 2, positive_implicit: 0 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 0 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_dense_packing_algorithm() {
            let flow = GridAutoFlow::RowDense;
            let explicit_col_count = 4;
            let explicit_row_count = 4;
            let children = {
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, (line(2), auto(), line(1), auto()).into_grid_child(), (1, 2, 0, 1)), // Definitely positioned in column 2
                    (2, (span(2), auto(), auto(), auto()).into_grid_child(), (2, 4, 0, 1)), // Spans 2 columns, so positioned after item 1
                    (3, (auto(), auto(), auto(), auto()).into_grid_child(), (0, 1, 0, 1)), // Spans 1 column, so should be positioned before item 1
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 0, explicit: 4, positive_implicit: 0 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 4, positive_implicit: 0 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_sparse_packing_algorithm() {
            let flow = GridAutoFlow::Row;
            let explicit_col_count = 4;
            let explicit_row_count = 4;
            let children = {
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, (auto(), span(3), auto(), auto()).into_grid_child(), (0, 3, 0, 1)), // Width 3
                    (2, (auto(), span(3), auto(), auto()).into_grid_child(), (0, 3, 1, 2)), // Width 3 (wraps to next row)
                    (3, (auto(), span(1), auto(), auto()).into_grid_child(), (3, 4, 1, 2)), // Width 1 (uses second row as we're already on it)
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 0, explicit: 4, positive_implicit: 0 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 4, positive_implicit: 0 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_auto_placement_in_negative_tracks() {
            let flow = GridAutoFlow::RowDense;
            let explicit_col_count = 2;
            let explicit_row_count = 2;
            let children = {
                vec![
                    // output order, node, style (grid coords), expected_placement (oz coords)
                    (1, (line(-5), auto(), line(1), auto()).into_grid_child(), (-2, -1, 0, 1)), // Row 1. Definitely positioned in column -2
                    (2, (auto(), auto(), line(2), auto()).into_grid_child(), (-2, -1, 1, 2)), // Row 2. Auto positioned in column -2
                    (3, (auto(), auto(), auto(), auto()).into_grid_child(), (-1, 0, 0, 1)), // Row 1. Auto positioned in column -1
                ]
            };
            let expected_cols = TrackCounts { negative_implicit: 2, explicit: 2, positive_implicit: 0 };
            let expected_rows = TrackCounts { negative_implicit: 0, explicit: 2, positive_implicit: 0 };
            placement_test_runner(explicit_col_count, explicit_row_count, children, expected_cols, expected_rows, flow);
        }

        #[test]
        fn test_overlarge_placement_uses_logical_limits() {
            let explicit_col_count = 9_000;
            let explicit_row_count = 0;
            let style = (line(-19_005), auto(), auto(), auto()).into_grid_child();
            let children = [(0, style)];
            let estimated_sizes = compute_grid_size_estimate(
                explicit_col_count,
                explicit_row_count,
                children.iter().map(|(_, style)| style),
            );
            let mut items = Vec::new();
            let mut cell_occupancy_matrix =
                CellOccupancyMatrix::with_track_counts(estimated_sizes.0, estimated_sizes.1);
            let mut name_resolver = NamedLineResolver::new(&Style::DEFAULT, 0, 0);
            name_resolver.set_explicit_column_count(explicit_col_count);
            name_resolver.set_explicit_row_count(explicit_row_count);
            place_grid_items(
                &mut cell_occupancy_matrix,
                &mut items,
                || children.iter().map(|(index, style)| (*index, NodeId::from(*index), style)),
                crate::WritingMode::HorizontalTb,
                GridAutoFlow::Row,
                AlignSelf::START,
                AlignSelf::START,
                &name_resolver,
            );
            assert_eq!(items[0].column, Line { start: OriginZeroLine(-10_000), end: OriginZeroLine(-9_999) });
        }
    }

    #[test]
    fn auto_placement_cursor_saturates_at_integer_bounds() {
        assert_eq!(advance_position(OriginZeroLine(i16::MAX)), OriginZeroLine(i16::MAX));
        assert_eq!(advance_position(OriginZeroLine(i16::MIN)), OriginZeroLine(i16::MIN + 1));
    }

    #[test]
    fn indefinite_spans_saturate_at_integer_bounds() {
        assert_eq!(
            resolve_indefinite_grid_span(OriginZeroLine(i16::MAX), 1),
            Line { start: OriginZeroLine(i16::MAX), end: OriginZeroLine(i16::MAX) }
        );
        assert_eq!(
            resolve_indefinite_grid_span(OriginZeroLine(i16::MIN), 1),
            Line { start: OriginZeroLine(i16::MIN), end: OriginZeroLine(i16::MIN + 1) }
        );
    }

    #[test]
    fn spans_are_clamped_before_physical_projection() {
        for (input, expected) in [
            (
                Line { start: OriginZeroLine(-10_004), end: OriginZeroLine(-10_003) },
                Line { start: OriginZeroLine(-10_000), end: OriginZeroLine(-9_999) },
            ),
            (
                Line { start: OriginZeroLine(10_004), end: OriginZeroLine(10_005) },
                Line { start: OriginZeroLine(9_999), end: OriginZeroLine(10_000) },
            ),
        ] {
            assert_eq!(clamp_span_to_limited_grid(input), expected);
        }
    }
}
