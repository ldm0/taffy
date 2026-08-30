//! Intrinsic Grid sizes for items crossing flexible tracks.

use taffy::prelude::*;
use taffy_test_helpers::{new_test_tree, test_measure_function};

#[derive(Clone, Copy)]
enum Axis {
    Columns,
    Rows,
}

#[derive(Clone, Copy)]
enum IntrinsicConstraint {
    MinContent,
    MaxContent,
}

fn layout_spanning_item(
    axis: Axis,
    constraint: IntrinsicConstraint,
    single_track: bool,
    explicit_minimum: Option<f32>,
) -> (Layout, Layout, Layout) {
    let mut tree = new_test_tree();
    tree.disable_rounding();

    let long_axis_size = 300.0;
    let filler = tree
        .new_leaf(Style {
            size: match axis {
                Axis::Columns => Size::from_lengths(long_axis_size, 10.0),
                Axis::Rows => Size::from_lengths(10.0, long_axis_size),
            },
            ..Default::default()
        })
        .unwrap();
    let intrinsic_constraint = match constraint {
        IntrinsicConstraint::MinContent => AvailableSpace::MinContent,
        IntrinsicConstraint::MaxContent => AvailableSpace::MaxContent,
    };
    let item_min_size = match axis {
        Axis::Columns => Size { width: explicit_minimum.map_or_else(auto, length), height: auto() },
        Axis::Rows => Size { width: auto(), height: explicit_minimum.map_or_else(auto, length) },
    };
    let spanning_item = tree
        .new_with_children(
            Style {
                min_size: item_min_size,
                grid_column: match axis {
                    Axis::Columns if !single_track => Line { start: line(1), end: span(2) },
                    _ => Line::AUTO,
                },
                grid_row: match axis {
                    Axis::Rows if !single_track => Line { start: line(1), end: span(2) },
                    _ => Line::AUTO,
                },
                ..Default::default()
            },
            &[filler],
        )
        .unwrap();
    let tracks = if single_track { vec![fr(1.0)] } else { vec![fr(1.0), length(30.0)] };
    let (columns, rows) = match axis {
        Axis::Columns => (tracks, vec![length(10.0)]),
        Axis::Rows => (vec![length(10.0)], tracks),
    };
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                box_sizing: BoxSizing::ContentBox,
                size: Size::auto(),
                border: Rect::length(10.0),
                grid_template_columns: columns,
                grid_template_rows: rows,
                ..Default::default()
            },
            &[spanning_item],
        )
        .unwrap();

    let available_space = match axis {
        Axis::Columns => Size { width: intrinsic_constraint, height: AvailableSpace::MaxContent },
        Axis::Rows => Size { width: AvailableSpace::MaxContent, height: intrinsic_constraint },
    };
    tree.compute_layout_with_measure(grid, available_space, test_measure_function).unwrap();

    (*tree.layout(grid).unwrap(), *tree.layout(spanning_item).unwrap(), *tree.layout(filler).unwrap())
}

#[test]
fn multitrack_item_crossing_a_flexible_column_does_not_raise_a_min_content_grid() {
    let (grid, item, _) = layout_spanning_item(Axis::Columns, IntrinsicConstraint::MinContent, false, None);

    assert_eq!(grid.size.width, 50.0);
    assert_eq!(item.size.width, 30.0);
}

#[test]
fn multitrack_item_crossing_a_flexible_row_does_not_raise_a_min_content_grid() {
    let (grid, item, _) = layout_spanning_item(Axis::Rows, IntrinsicConstraint::MinContent, false, None);

    assert_eq!(grid.size.height, 50.0);
    assert_eq!(item.size.height, 30.0);
}

#[test]
fn max_content_grid_includes_a_multitrack_items_flexible_track_contribution() {
    let (grid, item, _) = layout_spanning_item(Axis::Columns, IntrinsicConstraint::MaxContent, false, None);

    assert_eq!(grid.size.width, 320.0);
    assert_eq!(item.size.width, 300.0);
}

#[test]
fn single_flexible_track_preserves_the_items_content_based_automatic_minimum() {
    let (grid, item, _) = layout_spanning_item(Axis::Columns, IntrinsicConstraint::MinContent, true, None);

    assert_eq!(grid.size.width, 320.0);
    assert_eq!(item.size.width, 300.0);
}

#[test]
fn explicit_minimum_still_raises_a_flexible_track_under_a_min_content_constraint() {
    let (grid, item, _) = layout_spanning_item(Axis::Columns, IntrinsicConstraint::MinContent, false, Some(120.0));

    assert_eq!(grid.size.width, 140.0);
    assert_eq!(item.size.width, 120.0);
}
