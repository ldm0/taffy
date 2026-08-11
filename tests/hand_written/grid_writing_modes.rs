use taffy::prelude::*;
use taffy::{Direction, LogicalOffset, LogicalSize, Point, WritingDirection, WritingMode};

fn layout_two_by_two_grid(writing_mode: WritingMode, direction: Direction) -> [(Point<f32>, Size<f32>); 4] {
    let mut tree = TaffyTree::<()>::new();
    let children = core::array::from_fn(|_| {
        let child = tree.new_leaf(Style::default()).unwrap();
        tree.set_writing_mode(child, writing_mode).unwrap();
        child
    });
    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                direction,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![length(40.0), length(60.0)],
                grid_template_rows: vec![length(30.0), length(70.0)],
                ..Style::default()
            },
            &children,
        )
        .unwrap();
    tree.set_writing_mode(container, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    children.map(|child| {
        let layout = tree.layout(child).unwrap();
        (layout.location, layout.size)
    })
}

fn layout_start_aligned_grid_item(writing_mode: WritingMode, direction: Direction) -> Point<f32> {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            size: Size { width: length(20.0), height: length(10.0) },
            justify_self: Some(AlignSelf::START),
            align_self: Some(AlignSelf::START),
            ..Style::default()
        })
        .unwrap();
    tree.set_writing_mode(child, writing_mode).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                direction,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![fr(1.0)],
                grid_template_rows: vec![fr(1.0)],
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_writing_mode(container, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(child).unwrap().location
}

fn layout_percentage_grid_with_padding(
    writing_mode: WritingMode,
    direction: Direction,
) -> [(Point<f32>, Size<f32>); 4] {
    let mut tree = TaffyTree::<()>::new();
    let children = core::array::from_fn(|_| {
        let child = tree.new_leaf(Style::default()).unwrap();
        tree.set_writing_mode(child, writing_mode).unwrap();
        child
    });
    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                direction,
                size: Size { width: length(120.0), height: length(200.0) },
                padding: Rect { left: length(7.0), right: length(13.0), top: length(11.0), bottom: length(19.0) },
                gap: Size { width: length(10.0), height: length(20.0) },
                grid_template_columns: vec![percent(0.25), fr(1.0)],
                grid_template_rows: vec![percent(0.25), fr(1.0)],
                ..Style::default()
            },
            &children,
        )
        .unwrap();
    tree.set_writing_mode(container, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    children.map(|child| {
        let layout = tree.unrounded_layout(child);
        (layout.location, layout.size)
    })
}

fn layout_explicit_first_grid_cell(writing_mode: WritingMode, direction: Direction) -> (Point<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            grid_column: Line { start: line(1), end: line(2) },
            grid_row: Line { start: line(1), end: line(2) },
            ..Style::default()
        })
        .unwrap();
    tree.set_writing_mode(child, writing_mode).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                direction,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![length(40.0), length(60.0)],
                grid_template_rows: vec![length(30.0), length(70.0)],
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_writing_mode(container, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    let layout = tree.unrounded_layout(child);
    (layout.location, layout.size)
}

fn layout_absolute_second_grid_cell(writing_mode: WritingMode, direction: Direction) -> (Point<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            position: Position::Absolute,
            size: Size { width: length(10.0), height: length(8.0) },
            justify_self: Some(AlignSelf::START),
            align_self: Some(AlignSelf::START),
            grid_column: Line { start: line(2), end: line(3) },
            grid_row: Line { start: line(2), end: line(3) },
            ..Style::default()
        })
        .unwrap();
    tree.set_writing_mode(child, writing_mode).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                direction,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![length(40.0), length(60.0)],
                grid_template_rows: vec![length(30.0), length(70.0)],
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_writing_mode(container, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    let layout = tree.unrounded_layout(child);
    (layout.location, layout.size)
}

fn expected_percentage_grid_with_padding(
    writing_mode: WritingMode,
    direction: Direction,
) -> [(Point<f32>, Size<f32>); 4] {
    let outer_size = Size { width: 120.0, height: 200.0 };
    let writing_direction = WritingDirection::new(writing_mode, direction);
    let converter = writing_direction.converter(outer_size);
    let padding = writing_direction.to_logical_box_strut(Rect { left: 7.0, right: 13.0, top: 11.0, bottom: 19.0 });
    let outer_logical_size = converter.to_logical_size(outer_size);
    let content_inline_size = outer_logical_size.inline_size - padding.inline_start - padding.inline_end;
    let content_block_size = outer_logical_size.block_size - padding.block_start - padding.block_end;
    let inline_tracks = [content_inline_size * 0.25, content_inline_size * 0.75 - 10.0];
    let block_tracks = [content_block_size * 0.25, content_block_size * 0.75 - 20.0];

    core::array::from_fn(|index| {
        let inline_index = index % 2;
        let block_index = index / 2;
        let logical_size =
            LogicalSize { inline_size: inline_tracks[inline_index], block_size: block_tracks[block_index] };
        let physical_size = converter.to_physical_size(logical_size);
        let logical_offset = LogicalOffset {
            inline_offset: padding.inline_start + if inline_index == 0 { 0.0 } else { inline_tracks[0] + 10.0 },
            block_offset: padding.block_start + if block_index == 0 { 0.0 } else { block_tracks[0] + 20.0 },
        };
        (converter.to_physical_point(logical_offset, physical_size), physical_size)
    })
}

#[test]
fn grid_columns_and_rows_follow_the_containers_writing_direction() {
    let horizontal_ltr = [
        (Point { x: 0.0, y: 0.0 }, Size { width: 40.0, height: 30.0 }),
        (Point { x: 40.0, y: 0.0 }, Size { width: 60.0, height: 30.0 }),
        (Point { x: 0.0, y: 30.0 }, Size { width: 40.0, height: 70.0 }),
        (Point { x: 40.0, y: 30.0 }, Size { width: 60.0, height: 70.0 }),
    ];
    let horizontal_rtl = [
        (Point { x: 60.0, y: 0.0 }, Size { width: 40.0, height: 30.0 }),
        (Point { x: 0.0, y: 0.0 }, Size { width: 60.0, height: 30.0 }),
        (Point { x: 60.0, y: 30.0 }, Size { width: 40.0, height: 70.0 }),
        (Point { x: 0.0, y: 30.0 }, Size { width: 60.0, height: 70.0 }),
    ];
    let vertical_lr_ltr = [
        (Point { x: 0.0, y: 0.0 }, Size { width: 30.0, height: 40.0 }),
        (Point { x: 0.0, y: 40.0 }, Size { width: 30.0, height: 60.0 }),
        (Point { x: 30.0, y: 0.0 }, Size { width: 70.0, height: 40.0 }),
        (Point { x: 30.0, y: 40.0 }, Size { width: 70.0, height: 60.0 }),
    ];
    let vertical_lr_rtl = [
        (Point { x: 0.0, y: 60.0 }, Size { width: 30.0, height: 40.0 }),
        (Point { x: 0.0, y: 0.0 }, Size { width: 30.0, height: 60.0 }),
        (Point { x: 30.0, y: 60.0 }, Size { width: 70.0, height: 40.0 }),
        (Point { x: 30.0, y: 0.0 }, Size { width: 70.0, height: 60.0 }),
    ];
    let vertical_rl_ltr = [
        (Point { x: 70.0, y: 0.0 }, Size { width: 30.0, height: 40.0 }),
        (Point { x: 70.0, y: 40.0 }, Size { width: 30.0, height: 60.0 }),
        (Point { x: 0.0, y: 0.0 }, Size { width: 70.0, height: 40.0 }),
        (Point { x: 0.0, y: 40.0 }, Size { width: 70.0, height: 60.0 }),
    ];
    let vertical_rl_rtl = [
        (Point { x: 70.0, y: 60.0 }, Size { width: 30.0, height: 40.0 }),
        (Point { x: 70.0, y: 0.0 }, Size { width: 30.0, height: 60.0 }),
        (Point { x: 0.0, y: 60.0 }, Size { width: 70.0, height: 40.0 }),
        (Point { x: 0.0, y: 0.0 }, Size { width: 70.0, height: 60.0 }),
    ];

    for (writing_mode, direction, expected) in [
        (WritingMode::HorizontalTb, Direction::Ltr, horizontal_ltr),
        (WritingMode::HorizontalTb, Direction::Rtl, horizontal_rtl),
        (WritingMode::VerticalLr, Direction::Ltr, vertical_lr_ltr),
        (WritingMode::VerticalLr, Direction::Rtl, vertical_lr_rtl),
        (WritingMode::VerticalRl, Direction::Ltr, vertical_rl_ltr),
        (WritingMode::VerticalRl, Direction::Rtl, vertical_rl_rtl),
        (WritingMode::SidewaysLr, Direction::Ltr, vertical_lr_rtl),
        (WritingMode::SidewaysLr, Direction::Rtl, vertical_lr_ltr),
        (WritingMode::SidewaysRl, Direction::Ltr, vertical_rl_ltr),
        (WritingMode::SidewaysRl, Direction::Rtl, vertical_rl_rtl),
    ] {
        assert_eq!(
            layout_two_by_two_grid(writing_mode, direction),
            expected,
            "grid flow for {writing_mode:?} {direction:?}"
        );
    }
}

#[test]
fn grid_item_start_alignment_follows_each_logical_axis() {
    for (writing_mode, direction, expected) in [
        (WritingMode::HorizontalTb, Direction::Ltr, Point { x: 0.0, y: 0.0 }),
        (WritingMode::HorizontalTb, Direction::Rtl, Point { x: 80.0, y: 0.0 }),
        (WritingMode::VerticalLr, Direction::Ltr, Point { x: 0.0, y: 0.0 }),
        (WritingMode::VerticalLr, Direction::Rtl, Point { x: 0.0, y: 90.0 }),
        (WritingMode::VerticalRl, Direction::Ltr, Point { x: 80.0, y: 0.0 }),
        (WritingMode::VerticalRl, Direction::Rtl, Point { x: 80.0, y: 90.0 }),
        (WritingMode::SidewaysLr, Direction::Ltr, Point { x: 0.0, y: 90.0 }),
        (WritingMode::SidewaysLr, Direction::Rtl, Point { x: 0.0, y: 0.0 }),
        (WritingMode::SidewaysRl, Direction::Ltr, Point { x: 80.0, y: 0.0 }),
        (WritingMode::SidewaysRl, Direction::Rtl, Point { x: 80.0, y: 90.0 }),
    ] {
        assert_eq!(
            layout_start_aligned_grid_item(writing_mode, direction),
            expected,
            "start alignment for {writing_mode:?} {direction:?}"
        );
    }
}

#[test]
fn grid_percentages_gaps_and_padding_use_logical_axis_sizes() {
    for (writing_mode, direction) in [
        (WritingMode::HorizontalTb, Direction::Ltr),
        (WritingMode::HorizontalTb, Direction::Rtl),
        (WritingMode::VerticalLr, Direction::Ltr),
        (WritingMode::VerticalLr, Direction::Rtl),
        (WritingMode::VerticalRl, Direction::Ltr),
        (WritingMode::VerticalRl, Direction::Rtl),
        (WritingMode::SidewaysLr, Direction::Ltr),
        (WritingMode::SidewaysLr, Direction::Rtl),
        (WritingMode::SidewaysRl, Direction::Ltr),
        (WritingMode::SidewaysRl, Direction::Rtl),
    ] {
        assert_eq!(
            layout_percentage_grid_with_padding(writing_mode, direction),
            expected_percentage_grid_with_padding(writing_mode, direction),
            "percentage grid for {writing_mode:?} {direction:?}"
        );
    }
}

#[test]
fn explicit_grid_lines_follow_logical_start_edges() {
    let outer_size = Size { width: 100.0, height: 100.0 };
    for (writing_mode, direction) in [
        (WritingMode::HorizontalTb, Direction::Ltr),
        (WritingMode::HorizontalTb, Direction::Rtl),
        (WritingMode::VerticalLr, Direction::Ltr),
        (WritingMode::VerticalLr, Direction::Rtl),
        (WritingMode::VerticalRl, Direction::Ltr),
        (WritingMode::VerticalRl, Direction::Rtl),
        (WritingMode::SidewaysLr, Direction::Ltr),
        (WritingMode::SidewaysLr, Direction::Rtl),
        (WritingMode::SidewaysRl, Direction::Ltr),
        (WritingMode::SidewaysRl, Direction::Rtl),
    ] {
        let converter = WritingDirection::new(writing_mode, direction).converter(outer_size);
        let physical_size = converter.to_physical_size(LogicalSize { inline_size: 40.0, block_size: 30.0 });
        let expected = (
            converter.to_physical_point(LogicalOffset { inline_offset: 0.0, block_offset: 0.0 }, physical_size),
            physical_size,
        );
        assert_eq!(
            layout_explicit_first_grid_cell(writing_mode, direction),
            expected,
            "explicit first cell for {writing_mode:?} {direction:?}"
        );
    }
}

#[test]
fn absolute_grid_lines_use_logical_track_offsets() {
    let outer_size = Size { width: 100.0, height: 100.0 };
    let child_size = Size { width: 10.0, height: 8.0 };
    for (writing_mode, direction) in [
        (WritingMode::HorizontalTb, Direction::Ltr),
        (WritingMode::HorizontalTb, Direction::Rtl),
        (WritingMode::VerticalLr, Direction::Ltr),
        (WritingMode::VerticalLr, Direction::Rtl),
        (WritingMode::VerticalRl, Direction::Ltr),
        (WritingMode::VerticalRl, Direction::Rtl),
        (WritingMode::SidewaysLr, Direction::Ltr),
        (WritingMode::SidewaysLr, Direction::Rtl),
        (WritingMode::SidewaysRl, Direction::Ltr),
        (WritingMode::SidewaysRl, Direction::Rtl),
    ] {
        let converter = WritingDirection::new(writing_mode, direction).converter(outer_size);
        let expected = (
            converter.to_physical_point(LogicalOffset { inline_offset: 40.0, block_offset: 30.0 }, child_size),
            child_size,
        );
        assert_eq!(
            layout_absolute_second_grid_cell(writing_mode, direction),
            expected,
            "absolute second cell for {writing_mode:?} {direction:?}"
        );
    }
}

#[test]
fn grid_baseline_groups_follow_item_line_directions() {
    let mut tree = TaffyTree::<()>::new();
    let major =
        tree.new_leaf(Style { size: Size { width: length(20.0), height: length(20.0) }, ..Style::default() }).unwrap();
    tree.set_writing_mode(major, WritingMode::VerticalRl).unwrap();

    let minor =
        tree.new_leaf(Style { size: Size { width: length(30.0), height: length(20.0) }, ..Style::default() }).unwrap();
    tree.set_writing_mode(minor, WritingMode::VerticalLr).unwrap();

    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![length(50.0), length(50.0)],
                grid_template_rows: vec![length(100.0)],
                align_items: Some(AlignItems::BASELINE),
                ..Style::default()
            },
            &[major, minor],
        )
        .unwrap();
    tree.set_writing_mode(container, WritingMode::VerticalRl).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    // Opposite vertical line directions form distinct sharing groups. The
    // major group aligns to block-start and the minor group to block-end.
    assert_eq!(tree.unrounded_layout(major).location, Point { x: 80.0, y: 0.0 });
    assert_eq!(tree.unrounded_layout(minor).location, Point { x: 0.0, y: 50.0 });
}

#[test]
fn grid_column_baseline_groups_align_in_the_inline_axis() {
    let mut tree = TaffyTree::<()>::new();
    let major =
        tree.new_leaf(Style { size: Size { width: length(20.0), height: length(20.0) }, ..Style::default() }).unwrap();
    tree.set_writing_mode(major, WritingMode::VerticalLr).unwrap();

    let minor =
        tree.new_leaf(Style { size: Size { width: length(30.0), height: length(20.0) }, ..Style::default() }).unwrap();
    tree.set_writing_mode(minor, WritingMode::VerticalRl).unwrap();

    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: length(100.0) },
                grid_template_columns: vec![length(100.0)],
                grid_template_rows: vec![length(50.0), length(50.0)],
                justify_items: Some(AlignItems::BASELINE),
                ..Style::default()
            },
            &[major, minor],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.unrounded_layout(major).location, Point { x: 0.0, y: 0.0 });
    assert_eq!(tree.unrounded_layout(minor).location, Point { x: 70.0, y: 50.0 });
}

#[test]
fn grid_baseline_shims_are_isolated_per_sharing_group() {
    let mut tree = TaffyTree::<()>::new();
    let mut item = |width, writing_mode| {
        let node = tree
            .new_leaf(Style { size: Size { width: length(width), height: length(20.0) }, ..Style::default() })
            .unwrap();
        tree.set_writing_mode(node, writing_mode).unwrap();
        node
    };
    let major_narrow = item(20.0, WritingMode::VerticalRl);
    let major_wide = item(40.0, WritingMode::VerticalRl);
    let minor_wide = item(30.0, WritingMode::VerticalLr);
    let minor_narrow = item(10.0, WritingMode::VerticalLr);

    let container = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: length(200.0) },
                grid_template_columns: vec![length(50.0); 4],
                grid_template_rows: vec![length(100.0)],
                align_items: Some(AlignItems::BASELINE),
                ..Style::default()
            },
            &[major_narrow, major_wide, minor_wide, minor_narrow],
        )
        .unwrap();
    tree.set_writing_mode(container, WritingMode::VerticalRl).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.unrounded_layout(major_narrow).location, Point { x: 70.0, y: 0.0 });
    assert_eq!(tree.unrounded_layout(major_wide).location, Point { x: 60.0, y: 50.0 });
    assert_eq!(tree.unrounded_layout(minor_wide).location, Point { x: 0.0, y: 100.0 });
    assert_eq!(tree.unrounded_layout(minor_narrow).location, Point { x: 10.0, y: 150.0 });
}
