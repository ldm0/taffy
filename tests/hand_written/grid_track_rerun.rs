use taffy::prelude::*;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

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
