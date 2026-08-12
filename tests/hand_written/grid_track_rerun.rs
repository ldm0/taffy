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

#[test]
fn percentage_row_rerun_redistributes_spanning_contribution() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let spanning = tree
        .new_leaf(Style {
            size: Size { width: auto(), height: length(100.0) },
            grid_row: Line { start: line(1), end: line(4) },
            grid_column: Line { start: line(2), end: line(3) },
            ..Default::default()
        })
        .unwrap();
    let first = tree
        .new_leaf(Style {
            grid_row: Line { start: line(1), end: line(2) },
            grid_column: Line { start: line(1), end: line(2) },
            ..Default::default()
        })
        .unwrap();
    let last = tree
        .new_leaf(Style {
            grid_row: Line { start: line(3), end: line(4) },
            grid_column: Line { start: line(1), end: line(2) },
            ..Default::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                align_content: Some(AlignContent::START),
                grid_template_rows: vec![auto(), percent(0.1), auto()],
                ..Default::default()
            },
            &[spanning, first, last],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(first).unwrap().size.height, 45.0);
    assert_eq!(tree.layout(last).unwrap().location.y, 55.0);
    assert_eq!(tree.layout(last).unwrap().size.height, 45.0);
}
