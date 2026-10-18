use taffy::prelude::*;
use taffy::{style::GridTemplateAreas, tree::DetailedLayoutInfo};

#[test]
fn detailed_grid_info_retains_auto_repetitions_separately_from_explicit_tracks() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let child = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                grid_template_columns: vec![repeat("auto-fill", vec![length(20.0)])],
                grid_template_areas: Some(GridTemplateAreas { areas: Vec::new(), row_count: 1, column_count: 8 }),
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.rows.auto_repetitions, 0);
    assert_eq!(info.columns.explicit_tracks, 8);
    assert_eq!(info.columns.auto_repetitions, 5);
}

#[test]
fn orthogonal_grid_items_resolve_self_alignment_in_their_own_writing_mode() {
    use taffy::{Direction, Point, WritingMode};
    for container_direction in [Direction::Ltr, Direction::Rtl] {
        for item_direction in [Direction::Ltr, Direction::Rtl] {
            for item_mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
                for self_start in [true, false] {
                    let mut tree = TaffyTree::<()>::new();
                    let alignment = if self_start { AlignSelf::SELF_START } else { AlignSelf::SELF_END };
                    let item = tree
                        .new_leaf(Style {
                            direction: item_direction,
                            size: Size { width: length(20.0), height: length(50.0) },
                            justify_self: Some(alignment),
                            align_self: Some(alignment),
                            ..Style::default()
                        })
                        .unwrap();
                    tree.set_writing_mode(item, item_mode).unwrap();
                    let grid = tree
                        .new_with_children(
                            Style {
                                display: Display::Grid,
                                direction: container_direction,
                                grid_template_columns: vec![length(100.0)],
                                grid_template_rows: vec![length(150.0)],
                                ..Style::default()
                            },
                            &[item],
                        )
                        .unwrap();
                    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();
                    let right = (item_mode == WritingMode::VerticalRl) == self_start;
                    let bottom = (item_direction == Direction::Rtl) == self_start;
                    assert_eq!(tree.layout(item).unwrap().location,
                        Point { x: if right { 80.0 } else { 0.0 }, y: if bottom { 100.0 } else { 0.0 } },
                        "container={container_direction:?}, item={item_direction:?}/{item_mode:?}, self_start={self_start}");
                }
            }
        }
    }
}

#[test]
fn grid_track_constraints_keep_the_authored_minimum() {
    for horizontal in [true, false] {
        let mut tree = TaffyTree::<()>::new();
        let first = tree.new_leaf(Style::default()).unwrap();
        let second = tree.new_leaf(Style::default()).unwrap();
        let fractional = vec![minmax(length(10.0), fr(1.0)), minmax(length(10.0), fr(4.0))];
        let grid = tree
            .new_with_children(
                Style {
                    display: Display::Grid,
                    box_sizing: BoxSizing::ContentBox,
                    min_size: if horizontal {
                        Size { width: length(70.0), height: auto() }
                    } else {
                        Size { width: auto(), height: length(70.0) }
                    },
                    max_size: if horizontal {
                        Size { width: length(60.0), height: auto() }
                    } else {
                        Size { width: auto(), height: length(60.0) }
                    },
                    grid_template_columns: if horizontal { fractional.clone() } else { vec![length(50.0)] },
                    grid_template_rows: if horizontal { vec![length(50.0)] } else { fractional },
                    gap: if horizontal {
                        Size { width: length(33.0), height: length(0.0) }
                    } else {
                        Size { width: length(0.0), height: length(33.0) }
                    },
                    padding: Rect::length(2.0),
                    border: Rect::length(5.0),
                    ..Style::default()
                },
                &[first, second],
            )
            .unwrap();
        tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();
        let size = tree.layout(second).unwrap().size;
        assert_eq!(if horizontal { size.width } else { size.height }, 27.0);
    }
}

#[test]
fn grid_ratio_transfer_follows_authored_axis_clamping() {
    use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};
    let cases = [
        (
            Size { width: auto(), height: length(25.0) },
            Size { width: auto(), height: length(30.0) },
            Size { width: auto(), height: auto() },
            Size { width: 60.0, height: 30.0 },
        ),
        (
            Size { width: length(60.0), height: auto() },
            Size { width: length(80.0), height: auto() },
            Size { width: auto(), height: auto() },
            Size { width: 80.0, height: 40.0 },
        ),
        (
            Size { width: length(120.0), height: auto() },
            Size { width: auto(), height: auto() },
            Size { width: auto(), height: length(25.0) },
            Size { width: 120.0, height: 25.0 },
        ),
        (
            Size { width: length(25.0), height: auto() },
            Size { width: auto(), height: length(100.0) },
            Size { width: auto(), height: auto() },
            Size { width: 25.0, height: 100.0 },
        ),
    ];
    for (size, min_size, max_size, expected) in cases {
        let mut tree = new_test_tree();
        let child = tree
            .new_leaf_with_context(
                Style { item_is_replaced: true, aspect_ratio: Some(2.0), size, min_size, max_size, ..Style::default() },
                TestNodeContext::aspect_ratio(100.0, 0.5),
            )
            .unwrap();
        let grid = tree
            .new_with_children(
                Style {
                    display: Display::Grid,
                    size: Size { width: Dimension::min_content(), height: auto() },
                    ..Style::default()
                },
                &[child],
            )
            .unwrap();
        tree.compute_layout_with_measure_output(grid, Size::MAX_CONTENT, test_measure_function).unwrap();
        assert_eq!(tree.layout(child).unwrap().size, expected);
        assert_eq!(tree.layout(grid).unwrap().size, expected);
    }
}

#[test]
fn grid_replaced_alignment_changes_recompute_the_used_size() {
    use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};
    let mut tree = new_test_tree();
    let child = tree
        .new_leaf_with_context(
            Style { item_is_replaced: true, aspect_ratio: Some(1.0), ..Style::default() },
            TestNodeContext::aspect_ratio(100.0, 1.0),
        )
        .unwrap();
    let style = Style {
        display: Display::Grid,
        grid_template_columns: vec![length(100.0)],
        grid_template_rows: vec![length(200.0)],
        align_items: AlignItems::STRETCH,
        ..Style::default()
    };
    let root = tree.new_with_children(style.clone(), &[child]).unwrap();
    tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 200.0, height: 200.0 });
    tree.set_style(root, Style { align_items: AlignItems::START, ..style }).unwrap();
    tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 100.0, height: 100.0 });
    tree.mark_dirty(child).unwrap();
    tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn grid_absolute_safe_overflow_matches_start_in_vertical_writing_modes() {
    for mode in [taffy::WritingMode::HorizontalTb, taffy::WritingMode::VerticalRl, taffy::WritingMode::VerticalLr] {
        for child_width in [21.0, 33.0] {
            let mut positions = Vec::new();
            for safe in [false, true] {
                let mut tree = TaffyTree::<()>::new();
                let child = tree
                    .new_leaf(Style {
                        position: Position::Absolute,
                        grid_row: Line { start: line(2), end: line(3) },
                        grid_column: Line { start: line(2), end: line(3) },
                        justify_self: Some(if safe { AlignSelf::SAFE_END } else { AlignSelf::START }),
                        align_self: Some(if safe { AlignSelf::SAFE_CENTER } else { AlignSelf::START }),
                        size: Size::from_lengths(child_width, child_width),
                        ..Style::default()
                    })
                    .unwrap();
                tree.set_writing_mode(child, mode).unwrap();
                let root = tree
                    .new_with_children(
                        Style {
                            display: Display::Grid,
                            position: Position::Relative,
                            grid_template_columns: vec![length(2.0), length(20.0), length(2.0)],
                            grid_template_rows: vec![length(3.0), length(14.0), length(3.0)],
                            size: Size::from_lengths(30.0, 25.0),
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                tree.set_writing_mode(root, mode).unwrap();
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                positions.push(tree.layout(child).unwrap().location);
            }
            assert_eq!(positions[0], positions[1], "{mode:?}/{child_width}");
        }
    }
}

#[test]
fn absolute_grid_start_alignment_matches_regular_physical_track_coordinates() {
    for mode in [taffy::WritingMode::HorizontalTb, taffy::WritingMode::VerticalRl, taffy::WritingMode::VerticalLr] {
        for direction in [taffy::Direction::Ltr, taffy::Direction::Rtl] {
            let mut positions = Vec::new();
            for position in [Position::Relative, Position::Absolute] {
                let mut tree = TaffyTree::<()>::new();
                let child = tree
                    .new_leaf(Style {
                        position,
                        justify_self: Some(AlignSelf::START),
                        align_self: Some(AlignSelf::START),
                        size: Size::from_lengths(70.0, 20.0),
                        ..Style::default()
                    })
                    .unwrap();
                tree.set_writing_mode(child, mode).unwrap();
                let root = tree
                    .new_with_children(
                        Style {
                            display: Display::Grid,
                            position: Position::Relative,
                            direction,
                            grid_template_columns: vec![length(100.0)],
                            grid_template_rows: vec![length(50.0)],
                            size: Size::from_lengths(200.0, 100.0),
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                tree.set_writing_mode(root, mode).unwrap();
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                positions.push(tree.layout(child).unwrap().location);
            }
            assert_eq!(positions[0], positions[1], "{mode:?}/{direction:?}");
        }
    }
}
