//! Preferred-ratio min/max constraints at the Grid contribution boundary.

use taffy::prelude::*;
use taffy::{Overflow, Point};

fn layout_single_item(grid_style: Style, item_style: Style) -> (Size<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let item = tree.new_leaf(item_style).unwrap();
    let grid = tree.new_with_children(grid_style, &[item]).unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    (tree.layout(grid).unwrap().size, tree.layout(item).unwrap().size)
}

#[test]
fn ratio_transferred_maximum_clamps_row_track_contribution() {
    let (grid, item) = layout_single_item(
        Style { display: Display::Grid, size: Size { width: length(100.0), height: auto() }, ..Default::default() },
        Style {
            size: Size { width: length(100.0), height: auto() },
            max_size: Size { width: auto(), height: length(25.0) },
            aspect_ratio: Some(1.0),
            ..Default::default()
        },
    );

    assert_eq!(grid, Size { width: 100.0, height: 25.0 });
    assert_eq!(item, Size { width: 100.0, height: 25.0 });
}

#[test]
fn ratio_transferred_maximum_clamps_column_track_contribution() {
    let (grid, item) = layout_single_item(
        Style {
            display: Display::Grid,
            size: Size { width: Dimension::min_content(), height: length(100.0) },
            ..Default::default()
        },
        Style {
            size: Size { width: auto(), height: length(100.0) },
            max_size: Size { width: length(25.0), height: auto() },
            aspect_ratio: Some(1.0),
            ..Default::default()
        },
    );

    assert_eq!(grid, Size { width: 25.0, height: 100.0 });
    assert_eq!(item, Size { width: 25.0, height: 100.0 });
}

#[test]
fn scroll_container_ratio_items_use_zero_automatic_minimum_contribution() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let first = tree
        .new_leaf(Style {
            size: Size { width: auto(), height: length(100.0) },
            aspect_ratio: Some(2.0),
            overflow: Point { x: Overflow::Hidden, y: Overflow::Hidden },
            ..Default::default()
        })
        .unwrap();
    let second = tree
        .new_leaf(Style {
            size: Size { width: auto(), height: length(100.0) },
            min_size: Size { width: length(100.0), height: auto() },
            aspect_ratio: Some(0.5),
            overflow: Point { x: Overflow::Hidden, y: Overflow::Hidden },
            ..Default::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                grid_auto_flow: GridAutoFlow::Column,
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
}
