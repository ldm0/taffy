use taffy::prelude::*;
use taffy::{Direction, Point, WritingMode};

fn new_leaf(tree: &mut TaffyTree<()>, style: Style, writing_mode: WritingMode) -> NodeId {
    let node = tree.new_leaf(style).unwrap();
    tree.set_writing_mode(node, writing_mode).unwrap();
    node
}

fn new_container(tree: &mut TaffyTree<()>, style: Style, children: &[NodeId], writing_mode: WritingMode) -> NodeId {
    let node = tree.new_with_children(style, children).unwrap();
    tree.set_writing_mode(node, writing_mode).unwrap();
    node
}

fn layout_four_items(
    writing_mode: WritingMode,
    direction: Direction,
    flex_direction: FlexDirection,
    flex_wrap: FlexWrap,
) -> [Point<f32>; 4] {
    let mut tree = TaffyTree::<()>::new();
    let item_style =
        Style { size: Size { width: length(20.0), height: length(15.0) }, flex_shrink: 0.0, ..Style::default() };
    let items = [
        new_leaf(&mut tree, item_style.clone(), writing_mode),
        new_leaf(&mut tree, item_style.clone(), writing_mode),
        new_leaf(&mut tree, item_style.clone(), writing_mode),
        new_leaf(&mut tree, item_style, writing_mode),
    ];
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            direction,
            flex_direction,
            flex_wrap,
            size: Size { width: length(40.0), height: length(30.0) },
            ..Style::default()
        },
        &items,
        writing_mode,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    items.map(|item| tree.layout(item).unwrap().location)
}

#[test]
fn vertical_row_flow_follows_inline_direction_and_block_progression() {
    assert_eq!(
        layout_four_items(WritingMode::VerticalRl, Direction::Ltr, FlexDirection::Row, FlexWrap::Wrap),
        [Point { x: 20.0, y: 0.0 }, Point { x: 20.0, y: 15.0 }, Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 15.0 },]
    );
    assert_eq!(
        layout_four_items(WritingMode::VerticalLr, Direction::Ltr, FlexDirection::Row, FlexWrap::Wrap),
        [Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 15.0 }, Point { x: 20.0, y: 0.0 }, Point { x: 20.0, y: 15.0 },]
    );
    assert_eq!(
        layout_four_items(WritingMode::VerticalRl, Direction::Rtl, FlexDirection::Row, FlexWrap::Wrap),
        [Point { x: 20.0, y: 15.0 }, Point { x: 20.0, y: 0.0 }, Point { x: 0.0, y: 15.0 }, Point { x: 0.0, y: 0.0 },]
    );
}

#[test]
fn vertical_column_flow_follows_block_axis() {
    assert_eq!(
        layout_four_items(WritingMode::VerticalRl, Direction::Ltr, FlexDirection::Column, FlexWrap::Wrap),
        [Point { x: 20.0, y: 0.0 }, Point { x: 0.0, y: 0.0 }, Point { x: 20.0, y: 15.0 }, Point { x: 0.0, y: 15.0 },]
    );
}

#[test]
fn sideways_lr_row_flow_uses_bottom_to_top_inline_progression() {
    assert_eq!(
        layout_four_items(WritingMode::SidewaysLr, Direction::Ltr, FlexDirection::Row, FlexWrap::Wrap),
        [Point { x: 0.0, y: 15.0 }, Point { x: 0.0, y: 0.0 }, Point { x: 20.0, y: 15.0 }, Point { x: 20.0, y: 0.0 },]
    );
}

#[test]
fn vertical_rl_flex_flow_matrix_matches_css_logical_start_edges() {
    let cases = [
        (FlexDirection::Row, FlexWrap::Wrap, [(20.0, 0.0), (20.0, 15.0), (0.0, 0.0), (0.0, 15.0)]),
        (FlexDirection::Row, FlexWrap::WrapReverse, [(0.0, 0.0), (0.0, 15.0), (20.0, 0.0), (20.0, 15.0)]),
        (FlexDirection::RowReverse, FlexWrap::Wrap, [(20.0, 15.0), (20.0, 0.0), (0.0, 15.0), (0.0, 0.0)]),
        (FlexDirection::RowReverse, FlexWrap::WrapReverse, [(0.0, 15.0), (0.0, 0.0), (20.0, 15.0), (20.0, 0.0)]),
        (FlexDirection::Column, FlexWrap::Wrap, [(20.0, 0.0), (0.0, 0.0), (20.0, 15.0), (0.0, 15.0)]),
        (FlexDirection::Column, FlexWrap::WrapReverse, [(20.0, 15.0), (0.0, 15.0), (20.0, 0.0), (0.0, 0.0)]),
        (FlexDirection::ColumnReverse, FlexWrap::Wrap, [(0.0, 0.0), (20.0, 0.0), (0.0, 15.0), (20.0, 15.0)]),
        (FlexDirection::ColumnReverse, FlexWrap::WrapReverse, [(0.0, 15.0), (20.0, 15.0), (0.0, 0.0), (20.0, 0.0)]),
    ];

    for (direction, wrap, expected) in cases {
        let expected = expected.map(|(x, y)| Point { x, y });
        assert_eq!(
            layout_four_items(WritingMode::VerticalRl, Direction::Ltr, direction, wrap),
            expected,
            "unexpected layout for {direction:?} {wrap:?}"
        );
    }
}

#[test]
fn vertical_row_uses_column_gap_on_its_inline_main_axis() {
    let mut tree = TaffyTree::<()>::new();
    let item_style =
        Style { size: Size { width: length(10.0), height: length(10.0) }, flex_shrink: 0.0, ..Style::default() };
    let first = new_leaf(&mut tree, item_style.clone(), WritingMode::VerticalLr);
    let second = new_leaf(&mut tree, item_style, WritingMode::VerticalLr);
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            gap: Size { width: length(5.0), height: length(17.0) },
            size: Size { width: length(30.0), height: length(40.0) },
            ..Style::default()
        },
        &[first, second],
        WritingMode::VerticalLr,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(first).unwrap().location.y, 0.0);
    assert_eq!(tree.layout(second).unwrap().location.y, 15.0);
}

#[test]
fn vertical_child_box_percentages_use_containing_inline_size() {
    let mut tree = TaffyTree::<()>::new();
    let child = new_leaf(
        &mut tree,
        Style {
            size: Size { width: length(40.0), height: length(40.0) },
            padding: Rect { left: percent(0.1), ..Rect::zero() },
            ..Style::default()
        },
        WritingMode::VerticalLr,
    );
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            size: Size { width: length(100.0), height: length(200.0) },
            ..Style::default()
        },
        &[child],
        WritingMode::VerticalLr,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().padding.left, 20.0);
}

#[test]
fn orthogonal_nested_flex_uses_parent_inline_size_for_its_own_percentages() {
    let mut tree = TaffyTree::<()>::new();
    let grandchild = tree
        .new_leaf(Style {
            size: Size { width: length(10.0), height: length(10.0) },
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let child = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            size: Size { width: length(80.0), height: length(40.0) },
            padding: Rect { left: percent(0.1), ..Rect::zero() },
            ..Style::default()
        },
        &[grandchild],
        WritingMode::HorizontalTb,
    );
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            size: Size { width: length(100.0), height: length(200.0) },
            ..Style::default()
        },
        &[child],
        WritingMode::VerticalLr,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().padding.left, 20.0);
    assert_eq!(tree.layout(grandchild).unwrap().location.x, 20.0);
}

#[test]
fn vertical_flex_absolute_child_uses_container_inline_percentage_basis() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            position: Position::Absolute,
            size: Size { width: length(40.0), height: length(40.0) },
            padding: Rect { left: percent(0.1), ..Rect::zero() },
            ..Style::default()
        })
        .unwrap();
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            size: Size { width: length(100.0), height: length(200.0) },
            ..Style::default()
        },
        &[child],
        WritingMode::VerticalLr,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().padding.left, 20.0);
}

#[test]
fn orthogonal_self_start_uses_the_items_physical_start_side() {
    let mut tree = TaffyTree::<()>::new();
    let start = new_leaf(
        &mut tree,
        Style {
            align_self: Some(AlignSelf::START),
            size: Size { width: length(8.0), height: length(6.0) },
            flex_shrink: 0.0,
            ..Style::default()
        },
        WritingMode::VerticalRl,
    );
    let self_start = new_leaf(
        &mut tree,
        Style {
            align_self: Some(AlignSelf::SELF_START),
            size: Size { width: length(8.0), height: length(6.0) },
            flex_shrink: 0.0,
            ..Style::default()
        },
        WritingMode::HorizontalTb,
    );
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            size: Size { width: length(16.0), height: length(20.0) },
            ..Style::default()
        },
        &[start, self_start],
        WritingMode::VerticalRl,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(start).unwrap().location.x, 8.0);
    assert_eq!(tree.layout(self_start).unwrap().location.x, 0.0);
}

#[test]
fn vertical_rtl_cross_axis_alignment_uses_logical_start_and_end() {
    let mut tree = TaffyTree::<()>::new();
    let start = new_leaf(
        &mut tree,
        Style {
            align_self: Some(AlignSelf::START),
            size: Size { width: length(6.0), height: length(8.0) },
            flex_shrink: 0.0,
            ..Style::default()
        },
        WritingMode::VerticalRl,
    );
    let end = new_leaf(
        &mut tree,
        Style {
            align_self: Some(AlignSelf::END),
            size: Size { width: length(6.0), height: length(8.0) },
            flex_shrink: 0.0,
            ..Style::default()
        },
        WritingMode::VerticalRl,
    );
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            direction: Direction::Rtl,
            flex_direction: FlexDirection::Column,
            size: Size { width: length(20.0), height: length(16.0) },
            ..Style::default()
        },
        &[start, end],
        WritingMode::VerticalRl,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(start).unwrap().location.y, 8.0);
    assert_eq!(tree.layout(end).unwrap().location.y, 0.0);
}

#[test]
fn absolute_start_remains_logical_when_wrap_reverse_moves_flex_start() {
    let mut tree = TaffyTree::<()>::new();
    let start = tree
        .new_leaf(Style {
            position: Position::Absolute,
            align_self: Some(AlignSelf::START),
            size: Size { width: length(6.0), height: length(8.0) },
            ..Style::default()
        })
        .unwrap();
    let flex_start = tree
        .new_leaf(Style {
            position: Position::Absolute,
            align_self: Some(AlignSelf::FLEX_START),
            size: Size { width: length(6.0), height: length(8.0) },
            ..Style::default()
        })
        .unwrap();
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            direction: Direction::Rtl,
            flex_direction: FlexDirection::Column,
            flex_wrap: FlexWrap::WrapReverse,
            size: Size { width: length(20.0), height: length(16.0) },
            ..Style::default()
        },
        &[start, flex_start],
        WritingMode::VerticalRl,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(start).unwrap().location.y, 8.0);
    assert_eq!(tree.layout(flex_start).unwrap().location.y, 0.0);
}
