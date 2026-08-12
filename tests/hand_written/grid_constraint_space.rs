//! Constraint-space provenance used by Grid track sizing.
//!
//! A numeric used container size is not necessarily definite during the
//! initial track-sizing pass. Percentage tracks use definite authored or
//! parent-fixed geometry immediately; intrinsic and minimum-clamped geometry
//! becomes their basis only in the second pass.

use taffy::prelude::*;
use taffy::WritingMode;

fn overlapping_items(tree: &mut TaffyTree<()>, contributor_size: Size<Dimension>) -> (NodeId, NodeId) {
    let placement = Line { start: line(1), end: line(2) };
    let contributor = tree
        .new_leaf(Style {
            size: contributor_size,
            grid_row: placement.clone(),
            grid_column: placement.clone(),
            ..Default::default()
        })
        .unwrap();
    let stretched =
        tree.new_leaf(Style { grid_row: placement.clone(), grid_column: placement, ..Default::default() }).unwrap();
    (contributor, stretched)
}

#[test]
fn percentage_row_uses_intrinsic_contribution_before_min_height_clamp() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let (contributor, stretched) = overlapping_items(&mut tree, Size { width: auto(), height: length(120.0) });
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: auto(), height: length(100.0) },
                grid_template_rows: vec![percent(0.5)],
                ..Default::default()
            },
            &[contributor, stretched],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.height, 120.0);
    assert_eq!(tree.layout(stretched).unwrap().size.height, 60.0);
}

#[test]
fn percentage_row_reresolves_against_a_larger_minimum_clamped_height() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let (contributor, stretched) = overlapping_items(&mut tree, Size { width: auto(), height: length(20.0) });
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: auto(), height: length(100.0) },
                grid_template_rows: vec![percent(0.5)],
                ..Default::default()
            },
            &[contributor, stretched],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(stretched).unwrap().size.height, 50.0);
}

#[test]
fn percentage_row_uses_an_authored_definite_height_immediately() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let stretched = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: length(100.0) },
                grid_template_rows: vec![percent(0.5)],
                ..Default::default()
            },
            &[stretched],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(stretched).unwrap().size.height, 50.0);
}

#[test]
fn aspect_ratio_transfers_grid_track_definiteness() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let stretched = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                aspect_ratio: Some(2.0),
                grid_template_rows: vec![percent(0.5)],
                ..Default::default()
            },
            &[stretched],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(stretched).unwrap().size.height, 25.0);
}

#[test]
fn vertical_grid_projects_indefinite_block_size_to_physical_width() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let (contributor, stretched) = overlapping_items(&mut tree, Size { width: length(120.0), height: auto() });
    for node in [contributor, stretched] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: length(100.0), height: auto() },
                grid_template_rows: vec![percent(0.5)],
                ..Default::default()
            },
            &[contributor, stretched],
        )
        .unwrap();
    tree.set_writing_mode(grid, WritingMode::VerticalRl).unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.width, 120.0);
    assert_eq!(tree.layout(stretched).unwrap().size.width, 60.0);
}

#[test]
fn auto_repeat_uses_the_minimum_strategy_for_a_min_clamped_auto_size() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let child =
        tree.new_leaf(Style { grid_column: Line { start: line(3), end: line(4) }, ..Default::default() }).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: length(100.0), height: auto() },
                grid_template_columns: vec![repeat("auto-fill", vec![length(40.0)])],
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.width, 120.0);
    assert_eq!(tree.layout(child).unwrap().location.x, 80.0);
    assert_eq!(tree.layout(child).unwrap().size.width, 40.0);
}
