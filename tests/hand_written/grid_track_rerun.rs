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
fn flexible_row_rerun_uses_the_maximum_clamped_container_size() {
    let (grid, _, track_probe) = layout_overlapping_items(
        Style {
            max_size: Size { width: auto(), height: length(80.0) },
            grid_template_columns: vec![length(10.0)],
            grid_template_rows: vec![flex(0.5)],
            ..Default::default()
        },
        Size::from_lengths(10.0, 200.0),
    );

    assert_eq!(grid.size.height, 80.0);
    assert_eq!(track_probe.size.height, 40.0);
}

#[test]
fn flexible_track_rerun_distributes_space_between_tracks_after_gaps() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let first = tree
        .new_leaf(Style {
            grid_column: Line { start: line(1), end: line(2) },
            grid_row: Line { start: line(1), end: line(2) },
            ..Default::default()
        })
        .unwrap();
    let second = tree
        .new_leaf(Style {
            grid_column: Line { start: line(2), end: line(3) },
            grid_row: Line { start: line(1), end: line(2) },
            ..Default::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: length(200.0), height: auto() },
                gap: Size { width: length(10.0), height: zero() },
                grid_template_columns: vec![flex(0.25), flex(0.75)],
                grid_template_rows: vec![length(10.0)],
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.width, 200.0);
    assert_eq!(tree.layout(first).unwrap().size.width, 47.5);
    assert_eq!(tree.layout(second).unwrap().location.x, 57.5);
    assert_eq!(tree.layout(second).unwrap().size.width, 142.5);
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

#[test]
fn final_auto_repeat_inline_size_reresolves_ratio_dependent_block_size() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let child =
        tree.new_leaf(Style { grid_column: Line { start: line(2), end: line(3) }, ..Default::default() }).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: auto(), height: length(60.0) },
                aspect_ratio: Some(1.0),
                grid_template_columns: vec![repeat("auto-fill", vec![length(50.0)])],
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    // The transferred minimum admits one track, then the explicitly placed
    // item creates a second. The final inline size must become the ratio basis
    // for the automatic block size rather than leaving the provisional 60px.
    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(child).unwrap().location.x, 50.0);
    assert_eq!(tree.layout(child).unwrap().size.width, 50.0);
}

#[test]
fn percentage_minimum_transfers_through_ratio_before_auto_repeat() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let hidden = tree.new_leaf(Style { display: Display::None, ..Default::default() }).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: auto(), height: percent(0.6) },
                aspect_ratio: Some(1.0),
                grid_template_columns: vec![repeat("auto-fill", vec![length(50.0)])],
                ..Default::default()
            },
            &[hidden],
        )
        .unwrap();
    let percentage_container = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: auto(), height: percent(1.0) }, ..Default::default() },
            &[grid],
        )
        .unwrap();
    let sizing_container = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Default::default()
            },
            &[percentage_container],
        )
        .unwrap();

    tree.compute_layout(sizing_container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(sizing_container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(percentage_container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn aspect_ratio_resolved_block_size_stretches_auto_row() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let stretched = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                aspect_ratio: Some(1.0),
                ..Default::default()
            },
            &[stretched],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(stretched).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn aspect_ratio_resolved_vertical_block_size_stretches_auto_track() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let stretched = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: length(100.0) },
                aspect_ratio: Some(1.0),
                ..Default::default()
            },
            &[stretched],
        )
        .unwrap();
    for node in [grid, stretched] {
        tree.set_writing_mode(node, taffy::WritingMode::VerticalRl).unwrap();
    }

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(stretched).unwrap().size, Size { width: 100.0, height: 100.0 });
}
