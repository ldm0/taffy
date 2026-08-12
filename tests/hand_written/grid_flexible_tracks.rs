//! Constraint propagation while expanding flexible Grid tracks.

use taffy::prelude::*;
use taffy::WritingMode;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode as TestWritingMode};

#[test]
fn indefinite_flexible_track_includes_item_margin_in_its_max_content_contribution() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item = tree
        .new_leaf_with_context(
            Style {
                display: Display::Block,
                margin: Rect { left: length(10.0), ..Rect::zero() },
                ..Default::default()
            },
            TestNodeContext::fixed(20.0, 50.0),
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: length(50.0) },
                grid_template_columns: vec![minmax(length(0.0), fr(1.0))],
                grid_template_rows: vec![length(50.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 30.0, height: 50.0 });
    assert_eq!(tree.layout(item).unwrap().location.x, 10.0);
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 20.0, height: 50.0 });
}

#[test]
fn indefinite_flexible_track_measures_orthogonal_item_in_its_grid_area() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item = tree
        .new_leaf_with_context(
            Style { display: Display::Block, ..Default::default() },
            TestNodeContext::ahem_text("AAAAA\u{200b}AAAAA".into(), TestWritingMode::Vertical),
        )
        .unwrap();
    tree.set_writing_mode(item, WritingMode::VerticalLr).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: length(50.0) },
                grid_template_columns: vec![minmax(length(0.0), fr(1.0))],
                grid_template_rows: vec![length(50.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 20.0, height: 50.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 20.0, height: 50.0 });
}
