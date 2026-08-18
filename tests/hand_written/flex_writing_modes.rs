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
fn vertical_row_aligns_synthesized_baselines_on_the_logical_block_axis() {
    let mut tree = TaffyTree::<()>::new();
    let narrow = new_leaf(
        &mut tree,
        Style { size: Size { width: length(10.0), height: length(10.0) }, flex_shrink: 0.0, ..Style::default() },
        WritingMode::VerticalRl,
    );
    let wide = new_leaf(
        &mut tree,
        Style { size: Size { width: length(20.0), height: length(10.0) }, flex_shrink: 0.0, ..Style::default() },
        WritingMode::VerticalRl,
    );
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            align_items: Some(AlignItems::BASELINE),
            flex_direction: FlexDirection::Row,
            size: Size { width: length(30.0), height: length(20.0) },
            ..Style::default()
        },
        &[narrow, wide],
        WritingMode::VerticalRl,
    );

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(narrow).unwrap().location.x, 15.0);
    assert_eq!(tree.layout(wide).unwrap().location.x, 10.0);
}

#[test]
fn computed_font_baseline_controls_vertical_flex_synthesis_and_invalidates_cache() {
    let mut tree = TaffyTree::<()>::new();
    let wide = new_leaf(
        &mut tree,
        Style { size: Size { width: length(100.0), height: length(50.0) }, flex_shrink: 0.0, ..Style::default() },
        WritingMode::VerticalLr,
    );
    let narrow = new_leaf(
        &mut tree,
        Style { size: Size { width: length(50.0), height: length(50.0) }, flex_shrink: 0.0, ..Style::default() },
        WritingMode::VerticalLr,
    );
    let container = new_container(
        &mut tree,
        Style {
            display: Display::Flex,
            align_items: Some(AlignItems::BASELINE),
            size: Size { width: length(100.0), height: length(100.0) },
            ..Style::default()
        },
        &[wide, narrow],
        WritingMode::VerticalLr,
    );

    assert_eq!(tree.font_baseline(container).unwrap(), FontBaseline::Central);
    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.unrounded_layout(narrow).location, Point { x: 25.0, y: 50.0 });

    tree.set_font_baseline(container, FontBaseline::Alphabetic).unwrap();
    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.unrounded_layout(narrow).location, Point { x: 0.0, y: 50.0 });

    tree.clear_font_baseline(container).unwrap();
    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.unrounded_layout(narrow).location, Point { x: 25.0, y: 50.0 });
}

#[test]
fn flex_last_baseline_uses_each_items_last_fragment_baseline() {
    let mut tree = TaffyTree::<()>::new();
    let first_line =
        tree.new_leaf(Style { size: Size { width: length(10.0), height: length(10.0) }, ..Style::default() }).unwrap();
    let last_line =
        tree.new_leaf(Style { size: Size { width: length(10.0), height: length(20.0) }, ..Style::default() }).unwrap();
    let tall = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(40.0), height: auto() },
                padding: Rect { bottom: length(10.0), ..Rect::zero() },
                ..Style::default()
            },
            &[first_line, last_line],
        )
        .unwrap();
    let short_line =
        tree.new_leaf(Style { size: Size { width: length(10.0), height: length(10.0) }, ..Style::default() }).unwrap();
    let short = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(40.0), height: auto() },
                ..Style::default()
            },
            &[short_line],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                align_items: Some(AlignItems::LAST_BASELINE),
                size: Size { width: length(200.0), height: length(100.0) },
                ..Style::default()
            },
            &[tall, short],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.unrounded_layout(tall).size, Size { width: 40.0, height: 40.0 });
    assert_eq!(tree.unrounded_layout(short).size, Size { width: 40.0, height: 10.0 });
    assert_eq!(tree.unrounded_layout(tall).location, Point { x: 0.0, y: 60.0 });
    assert_eq!(tree.unrounded_layout(short).location, Point { x: 40.0, y: 80.0 });
}

#[test]
fn flex_first_and_last_baselines_use_distinct_sharing_groups() {
    let mut tree = TaffyTree::<()>::new();
    let first = tree
        .new_leaf(Style {
            align_self: Some(AlignSelf::BASELINE),
            size: Size { width: length(40.0), height: length(40.0) },
            ..Style::default()
        })
        .unwrap();
    let last = tree
        .new_leaf(Style {
            align_self: Some(AlignSelf::LAST_BASELINE),
            size: Size { width: length(40.0), height: length(10.0) },
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(200.0), height: length(100.0) },
                ..Style::default()
            },
            &[first, last],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.unrounded_layout(first).location, Point { x: 0.0, y: 0.0 });
    assert_eq!(tree.unrounded_layout(last).location, Point { x: 40.0, y: 90.0 });
}

fn nested_flex_reference_offset(flex_direction: FlexDirection, alignment: AlignItems) -> f32 {
    let mut tree = TaffyTree::<()>::new();
    let reference =
        tree.new_leaf(Style { size: Size { width: length(20.0), height: length(20.0) }, ..Style::default() }).unwrap();
    let small = tree
        .new_leaf(Style {
            size: Size { width: length(10.0), height: length(10.0) },
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let large = tree
        .new_leaf(Style {
            size: Size { width: length(30.0), height: length(30.0) },
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let nested = tree
        .new_with_children(
            Style { display: Display::Flex, flex_direction, flex_shrink: 0.0, ..Style::default() },
            &[small, large],
        )
        .unwrap();
    let outer = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                align_items: Some(alignment),
                size: Size { width: length(200.0), height: length(100.0) },
                ..Style::default()
            },
            &[reference, nested],
        )
        .unwrap();

    tree.compute_layout(outer, Size::MAX_CONTENT).unwrap();
    tree.unrounded_layout(reference).location.y
}

#[test]
fn reversed_flex_flow_exports_fallback_baselines_from_its_flow_endpoints() {
    let cases = [
        (FlexDirection::Row, AlignItems::BASELINE, 0.0),
        (FlexDirection::RowReverse, AlignItems::BASELINE, 10.0),
        (FlexDirection::Column, AlignItems::BASELINE, 0.0),
        (FlexDirection::ColumnReverse, AlignItems::BASELINE, 10.0),
        (FlexDirection::Row, AlignItems::LAST_BASELINE, 80.0),
        (FlexDirection::RowReverse, AlignItems::LAST_BASELINE, 60.0),
        (FlexDirection::Column, AlignItems::LAST_BASELINE, 80.0),
        (FlexDirection::ColumnReverse, AlignItems::LAST_BASELINE, 80.0),
    ];

    for (direction, alignment, expected) in cases {
        assert_eq!(
            nested_flex_reference_offset(direction, alignment),
            expected,
            "unexpected {alignment:?} fallback exported by {direction:?}"
        );
    }
}

#[test]
fn flex_wrap_reverse_flips_the_last_baseline_group_once() {
    let mut tree = TaffyTree::<()>::new();
    let tall =
        tree.new_leaf(Style { size: Size { width: length(40.0), height: length(40.0) }, ..Style::default() }).unwrap();
    let short =
        tree.new_leaf(Style { size: Size { width: length(40.0), height: length(10.0) }, ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_wrap: FlexWrap::WrapReverse,
                align_items: Some(AlignItems::LAST_BASELINE),
                size: Size { width: length(200.0), height: length(100.0) },
                ..Style::default()
            },
            &[tall, short],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.unrounded_layout(tall).location, Point { x: 0.0, y: 0.0 });
    assert_eq!(tree.unrounded_layout(short).location, Point { x: 40.0, y: 30.0 });
}

#[test]
fn vertical_column_justify_left_and_right_keep_their_physical_edges() {
    for writing_mode in
        [WritingMode::VerticalRl, WritingMode::VerticalLr, WritingMode::SidewaysRl, WritingMode::SidewaysLr]
    {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for flex_direction in [FlexDirection::Column, FlexDirection::ColumnReverse] {
                for position in [Position::Relative, Position::Absolute] {
                    for (justify_content, expected_x) in [(JustifyContent::LEFT, 0.0), (JustifyContent::RIGHT, 12.0)] {
                        let mut tree = TaffyTree::<()>::new();
                        let child = new_leaf(
                            &mut tree,
                            Style {
                                position,
                                size: Size { width: length(8.0), height: length(6.0) },
                                flex_shrink: 0.0,
                                ..Style::default()
                            },
                            writing_mode,
                        );
                        let container = new_container(
                            &mut tree,
                            Style {
                                display: Display::Flex,
                                direction,
                                flex_direction,
                                justify_content: Some(justify_content),
                                size: Size { width: length(20.0), height: length(16.0) },
                                ..Style::default()
                            },
                            &[child],
                            writing_mode,
                        );

                        tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

                        assert_eq!(
                            tree.layout(child).unwrap().location.x,
                            expected_x,
                            "{writing_mode:?} {direction:?} {flex_direction:?} {position:?} {justify_content:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn vertical_row_content_alignment_distinguishes_logical_and_physical_edges() {
    for writing_mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for flex_direction in [FlexDirection::Row, FlexDirection::RowReverse] {
                for position in [Position::Relative, Position::Absolute] {
                    for (justify_content, expected_y) in [
                        (JustifyContent::LEFT, 0.0),
                        (JustifyContent::RIGHT, 10.0),
                        (JustifyContent::START, if direction == Direction::Rtl { 10.0 } else { 0.0 }),
                        (JustifyContent::END, if direction == Direction::Rtl { 0.0 } else { 10.0 }),
                    ] {
                        let mut tree = TaffyTree::<()>::new();
                        let child = new_leaf(
                            &mut tree,
                            Style {
                                position,
                                size: Size { width: length(8.0), height: length(6.0) },
                                flex_shrink: 0.0,
                                ..Style::default()
                            },
                            writing_mode,
                        );
                        let container = new_container(
                            &mut tree,
                            Style {
                                display: Display::Flex,
                                direction,
                                flex_direction,
                                justify_content: Some(justify_content),
                                size: Size { width: length(20.0), height: length(16.0) },
                                ..Style::default()
                            },
                            &[child],
                            writing_mode,
                        );

                        tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

                        assert_eq!(
                            tree.layout(child).unwrap().location.y,
                            expected_y,
                            "{writing_mode:?} {direction:?} {flex_direction:?} {position:?} {justify_content:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn vertical_main_axis_justify_left_and_right_fall_back_to_block_start() {
    for flex_direction in [FlexDirection::Column, FlexDirection::ColumnReverse] {
        for position in [Position::Relative, Position::Absolute] {
            for justify_content in [JustifyContent::LEFT, JustifyContent::RIGHT] {
                let mut tree = TaffyTree::<()>::new();
                let child = new_leaf(
                    &mut tree,
                    Style {
                        position,
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
                        flex_direction,
                        justify_content: Some(justify_content),
                        size: Size { width: length(20.0), height: length(16.0) },
                        ..Style::default()
                    },
                    &[child],
                    WritingMode::HorizontalTb,
                );

                tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

                assert_eq!(
                    tree.layout(child).unwrap().location.y,
                    0.0,
                    "{flex_direction:?} {position:?} {justify_content:?}"
                );
            }
        }
    }
}

#[test]
fn absolute_flex_last_baseline_uses_its_end_fallback() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            position: Position::Absolute,
            align_self: Some(AlignSelf::LAST_BASELINE),
            size: Size { width: length(20.0), height: length(10.0) },
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(100.0), height: length(100.0) },
                ..Style::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.unrounded_layout(child).location, Point { x: 0.0, y: 90.0 });
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

#[test]
fn flex_auto_position_uses_the_physical_padding_box_across_writing_directions() {
    for writing_mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut tree = TaffyTree::<()>::new();
            let child = new_leaf(
                &mut tree,
                Style {
                    position: Position::Absolute,
                    size: Size { width: percent(1.0), height: percent(1.0) },
                    ..Style::default()
                },
                writing_mode,
            );
            let container = new_container(
                &mut tree,
                Style {
                    display: Display::Flex,
                    direction,
                    box_sizing: BoxSizing::ContentBox,
                    size: Size { width: length(100.0), height: length(100.0) },
                    border: Rect { left: length(20.0), right: length(10.0), top: length(5.0), bottom: length(15.0) },
                    ..Style::default()
                },
                &[child],
                writing_mode,
            );

            tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

            assert_eq!(
                tree.layout(container).unwrap().size,
                Size { width: 130.0, height: 120.0 },
                "container size for {writing_mode:?} {direction:?}"
            );
            assert_eq!(
                tree.layout(child).unwrap().size,
                Size { width: 100.0, height: 100.0 },
                "child size for {writing_mode:?} {direction:?}"
            );
            assert_eq!(
                tree.layout(child).unwrap().location,
                Point { x: 20.0, y: 5.0 },
                "child location for {writing_mode:?} {direction:?}"
            );
        }
    }
}

#[test]
fn vertical_rtl_absolute_static_position_reverses_only_for_authored_flex_reverse() {
    for writing_mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for (flex_direction, justify_content, expected_y) in [
            (FlexDirection::Row, JustifyContent::FLEX_START, 40.0),
            (FlexDirection::Row, JustifyContent::FLEX_END, 0.0),
            (FlexDirection::Row, JustifyContent::SPACE_BETWEEN, 40.0),
            (FlexDirection::RowReverse, JustifyContent::FLEX_START, 0.0),
            (FlexDirection::RowReverse, JustifyContent::FLEX_END, 40.0),
            (FlexDirection::RowReverse, JustifyContent::SPACE_BETWEEN, 0.0),
        ] {
            let mut tree = TaffyTree::<()>::new();
            let child = new_leaf(
                &mut tree,
                Style {
                    position: Position::Absolute,
                    size: Size { width: length(10.0), height: length(40.0) },
                    ..Style::default()
                },
                writing_mode,
            );
            let container = new_container(
                &mut tree,
                Style {
                    display: Display::Flex,
                    direction: Direction::Rtl,
                    flex_direction,
                    justify_content: Some(justify_content),
                    size: Size { width: length(20.0), height: length(80.0) },
                    ..Style::default()
                },
                &[child],
                writing_mode,
            );

            tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

            assert_eq!(
                tree.layout(child).unwrap().location.y,
                expected_y,
                "{writing_mode:?} {flex_direction:?} {justify_content:?}"
            );
        }
    }
}
