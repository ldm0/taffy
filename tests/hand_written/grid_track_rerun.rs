//! Additional Grid track-sizing passes after an initially indefinite
//! container axis acquires its used size.

use taffy::prelude::*;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

fn layout_overlapping_items(mut grid_style: Style, contributor_size: Size<Dimension>) -> (Layout, Layout, Layout) {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let placement = Line { start: line(1), end: line(2) };
    let contributor = tree
        .new_leaf(Style {
            size: contributor_size,
            grid_column: placement.clone(),
            grid_row: placement.clone(),
            ..Default::default()
        })
        .unwrap();
    let track_probe =
        tree.new_leaf(Style { grid_column: placement.clone(), grid_row: placement, ..Default::default() }).unwrap();
    grid_style.display = Display::Grid;
    let grid = tree.new_with_children(grid_style, &[contributor, track_probe]).unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    (*tree.layout(grid).unwrap(), *tree.layout(contributor).unwrap(), *tree.layout(track_probe).unwrap())
}

#[test]
fn indefinite_flexible_tracks_rerun_against_the_resolved_container_size() {
    // Mirrors WPT grid-template-flexible-rerun-track-sizing.html. The first
    // intrinsic pass establishes a 100px container in each axis; the .5fr
    // tracks are then resolved once more against that definite size.
    let (grid, contributor, track_probe) = layout_overlapping_items(
        Style { grid_template_columns: vec![flex(0.5)], grid_template_rows: vec![flex(0.5)], ..Default::default() },
        Size::from_lengths(200.0, 200.0),
    );

    assert_eq!(grid.size, Size { width: 100.0, height: 100.0 });
    assert_eq!(contributor.size, Size { width: 200.0, height: 200.0 });
    assert_eq!(track_probe.size, Size { width: 50.0, height: 50.0 });
}

#[test]
fn block_axis_rerun_does_not_depend_on_an_inline_axis_rerun() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style { grid_template_columns: vec![length(50.0)], grid_template_rows: vec![flex(0.5)], ..Default::default() },
        Size::from_lengths(50.0, 200.0),
    );

    assert_eq!(grid.size, Size { width: 50.0, height: 100.0 });
    assert_eq!(track_probe.size, Size { width: 50.0, height: 50.0 });
}

#[test]
fn flexible_track_rerun_uses_the_minimum_clamped_container_size() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style {
            min_size: Size { width: length(200.0), height: auto() },
            grid_template_columns: vec![flex(0.5)],
            grid_template_rows: vec![length(10.0)],
            ..Default::default()
        },
        Size::from_lengths(100.0, 10.0),
    );

    assert_eq!(grid.size.width, 200.0);
    assert_eq!(track_probe.size.width, 100.0);
}

#[test]
fn flexible_track_rerun_uses_the_maximum_clamped_container_size() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style {
            max_size: Size { width: length(80.0), height: auto() },
            grid_template_columns: vec![flex(0.5)],
            grid_template_rows: vec![length(10.0)],
            ..Default::default()
        },
        Size::from_lengths(200.0, 10.0),
    );

    assert_eq!(grid.size.width, 80.0);
    assert_eq!(track_probe.size.width, 40.0);
}

#[test]
fn flexible_track_rerun_uses_content_box_space_inside_padding_and_border() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style {
            min_size: Size { width: length(230.0), height: auto() },
            padding: Rect { left: length(10.0), right: length(10.0), top: zero(), bottom: zero() },
            border: Rect { left: length(5.0), right: length(5.0), top: zero(), bottom: zero() },
            grid_template_columns: vec![flex(0.5)],
            grid_template_rows: vec![length(10.0)],
            ..Default::default()
        },
        Size::from_lengths(100.0, 10.0),
    );

    assert_eq!(grid.size.width, 230.0);
    assert_eq!(track_probe.size.width, 100.0);
    assert_eq!(track_probe.location.x, 15.0);
}

#[test]
fn percentage_track_uses_the_same_available_size_dependency_path() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style {
            min_size: Size { width: length(200.0), height: auto() },
            grid_template_columns: vec![percent(0.5)],
            grid_template_rows: vec![length(10.0)],
            ..Default::default()
        },
        Size::from_lengths(100.0, 10.0),
    );

    assert_eq!(grid.size.width, 200.0);
    assert_eq!(track_probe.size.width, 100.0);
}

#[test]
fn definite_grid_size_resolves_flexible_tracks_in_the_initial_pass() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style {
            size: Size::from_lengths(200.0, 100.0),
            grid_template_columns: vec![flex(0.5)],
            grid_template_rows: vec![flex(0.5)],
            ..Default::default()
        },
        Size::from_lengths(10.0, 10.0),
    );

    assert_eq!(grid.size, Size { width: 200.0, height: 100.0 });
    assert_eq!(track_probe.size, Size { width: 100.0, height: 50.0 });
}

#[test]
fn percentage_column_rerun_updates_an_intrinsic_rows_block_contribution() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style::default(),
            TestNodeContext::ahem_text("aa\u{200b}bb".to_owned(), WritingMode::Horizontal),
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style { display: Display::Grid, grid_template_columns: vec![percent(0.5)], ..Default::default() },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 40.0, height: 20.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 20.0, height: 20.0 });
}

#[test]
fn row_rerun_refreshes_every_intrinsic_item_before_track_sizing() {
    let mut tree = new_test_tree();
    let first = tree
        .new_leaf_with_context(
            Style { grid_row: Line { start: line(1), end: line(2) }, ..Default::default() },
            TestNodeContext::ahem_text("aa\u{200b}bb".to_owned(), WritingMode::Horizontal),
        )
        .unwrap();
    let second = tree
        .new_leaf_with_context(
            Style { grid_row: Line { start: line(2), end: line(3) }, ..Default::default() },
            TestNodeContext::ahem_text("cc\u{200b}dd".to_owned(), WritingMode::Horizontal),
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style { display: Display::Grid, grid_template_columns: vec![percent(0.5)], ..Default::default() },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 40.0, height: 40.0 });
    assert_eq!(tree.layout(first).unwrap().size, Size { width: 20.0, height: 20.0 });
    assert_eq!(tree.layout(second).unwrap().size, Size { width: 20.0, height: 20.0 });
    assert_eq!(tree.layout(second).unwrap().location.y, 20.0);
}
