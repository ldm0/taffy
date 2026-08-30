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

fn layout_grid_item_with_percentage_descendant(
    writing_mode: WritingMode,
    mut item_style: Style,
) -> (Size<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let vertical = !writing_mode.is_horizontal();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: if vertical {
                Size { width: length(20.0), height: auto() }
            } else {
                Size { width: auto(), height: length(20.0) }
            },
            ..Default::default()
        })
        .unwrap();
    let percentage = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: if vertical {
                    Size { width: percent(0.5), height: auto() }
                } else {
                    Size { width: auto(), height: percent(0.5) }
                },
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    item_style.display = Display::Block;
    let item = tree.new_with_children(item_style, &[percentage]).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![length(100.0)],
                grid_template_rows: vec![length(100.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();
    for node in [content, percentage, item, grid] {
        tree.set_writing_mode(node, writing_mode).unwrap();
    }

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    (tree.layout(item).unwrap().size, tree.layout(percentage).unwrap().size)
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
fn aspect_ratio_resolved_block_size_centers_item_in_stretched_auto_row() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let item = tree.new_leaf(Style { size: Size::from_lengths(100.0, 50.0), ..Default::default() }).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                aspect_ratio: Some(1.0),
                align_items: Some(AlignItems::CENTER),
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 50.0 });
    assert_eq!(tree.layout(item).unwrap().location.y, 25.0);
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
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(stretched).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn minimum_clamped_block_size_stretches_auto_row() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let stretched = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                min_size: Size { width: auto(), height: length(100.0) },
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
    // item creates a second. The final 100px inline size must transfer back to
    // the automatic block size instead of leaving the initial 60px minimum.
    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(child).unwrap().location.x, 50.0);
    assert_eq!(tree.layout(child).unwrap().size.width, 50.0);
}

#[test]
fn intrinsic_keyword_grid_item_height_remains_indefinite_for_descendants() {
    let (item, percentage) = layout_grid_item_with_percentage_descendant(
        WritingMode::HorizontalTb,
        Style {
            size: Size { width: auto(), height: Dimension::max_content() },
            align_self: Some(AlignSelf::START),
            ..Default::default()
        },
    );

    assert_eq!(item.height, 20.0);
    assert_eq!(percentage.height, 20.0);
}

#[test]
fn minimum_clamped_grid_item_height_remains_indefinite_for_descendants() {
    let (item, percentage) = layout_grid_item_with_percentage_descendant(
        WritingMode::HorizontalTb,
        Style {
            min_size: Size { width: auto(), height: length(100.0) },
            align_self: Some(AlignSelf::START),
            ..Default::default()
        },
    );

    assert_eq!(item.height, 100.0);
    assert_eq!(percentage.height, 20.0);
}

#[test]
fn authored_grid_item_height_is_definite_for_descendants() {
    let (item, percentage) = layout_grid_item_with_percentage_descendant(
        WritingMode::HorizontalTb,
        Style {
            size: Size { width: auto(), height: length(100.0) },
            align_self: Some(AlignSelf::START),
            ..Default::default()
        },
    );

    assert_eq!(item.height, 100.0);
    assert_eq!(percentage.height, 50.0);
}

#[test]
fn stretched_grid_item_height_is_definite_for_descendants() {
    let (item, percentage) = layout_grid_item_with_percentage_descendant(WritingMode::HorizontalTb, Style::default());

    assert_eq!(item.height, 100.0);
    assert_eq!(percentage.height, 50.0);
}

#[test]
fn ratio_transferred_grid_item_height_is_definite_for_descendants() {
    let (item, percentage) = layout_grid_item_with_percentage_descendant(
        WritingMode::HorizontalTb,
        Style {
            size: Size { width: length(100.0), height: auto() },
            aspect_ratio: Some(2.0),
            align_self: Some(AlignSelf::START),
            ..Default::default()
        },
    );

    assert_eq!(item.height, 50.0);
    assert_eq!(percentage.height, 25.0);
}

#[test]
fn vertical_intrinsic_grid_item_block_size_remains_indefinite_for_descendants() {
    let (item, percentage) = layout_grid_item_with_percentage_descendant(
        WritingMode::VerticalRl,
        Style {
            size: Size { width: Dimension::max_content(), height: auto() },
            align_self: Some(AlignSelf::START),
            ..Default::default()
        },
    );

    assert_eq!(item.width, 20.0);
    assert_eq!(percentage.width, 20.0);
}

#[test]
fn orthogonal_grid_item_percentages_do_not_use_the_viewport_fallback() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let content = tree.new_leaf(Style { size: Size::from_lengths(30.0, 30.0), ..Default::default() }).unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size::from_percent(1.0, 1.0),
                grid_column: Line { start: line(2), end: line(3) },
                grid_row: Line { start: line(2), end: line(3) },
                justify_self: Some(JustifySelf::START),
                align_self: Some(AlignSelf::START),
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(10.0, 10.0),
                grid_template_columns: vec![length(3.0), auto(), length(4.0)],
                grid_template_rows: vec![length(1.0), auto(), length(2.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();
    for node in [item, content] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }

    tree.compute_layout(grid, Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) })
        .unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 10.0, height: 10.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 30.0, height: 30.0 });
}

#[test]
fn cyclic_percentage_minimums_do_not_enable_the_grid_automatic_minimum() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let content = tree.new_leaf(Style { size: Size::from_lengths(30.0, 30.0), ..Default::default() }).unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size::from_percent(1.0, 1.0),
                min_size: Size::from_percent(1.0, 1.0),
                grid_column: Line { start: line(2), end: line(3) },
                grid_row: Line { start: line(2), end: line(3) },
                justify_self: Some(JustifySelf::START),
                align_self: Some(AlignSelf::START),
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(10.0, 10.0),
                grid_template_columns: vec![length(3.0), auto(), length(4.0)],
                grid_template_rows: vec![length(1.0), auto(), length(2.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) })
        .unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 10.0, height: 10.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 3.0, height: 7.0 });
}
