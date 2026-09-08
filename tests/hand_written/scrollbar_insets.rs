use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{Direction, Point, WritingMode};

#[test]
fn absolute_insets_do_not_reverse_physical_scrollbar_edges_with_flex_axes() {
    // Border and gutters delimit the physical padding-box containing block.
    // Flex direction/wrapping only affects static positions, not left/top or
    // right/bottom. Unequal gutters expose accidental edge swaps on either axis.
    let gutters = Rect { left: 7.0, right: 11.0, top: 13.0, bottom: 17.0 };
    let border = Rect { left: length(3.0), right: length(5.0), top: length(2.0), bottom: length(4.0) };
    let margin = Rect { left: length(4.0), right: length(8.0), top: length(2.0), bottom: length(6.0) };
    let fixed_size = Size { width: length(40.0), height: length(20.0) };
    let cases = [
        (
            Rect { left: length(11.0), right: auto(), top: length(7.0), bottom: auto() },
            fixed_size,
            margin,
            Point { x: 25.0, y: 24.0 },
            Size { width: 40.0, height: 20.0 },
        ),
        (
            Rect { left: auto(), right: length(13.0), top: auto(), bottom: length(9.0) },
            fixed_size,
            margin,
            Point { x: 123.0, y: 104.0 },
            Size { width: 40.0, height: 20.0 },
        ),
        (
            Rect { left: percent(0.25), right: auto(), top: percent(0.25), bottom: auto() },
            fixed_size,
            margin,
            Point { x: 57.5, y: 48.0 },
            Size { width: 40.0, height: 20.0 },
        ),
        (
            Rect { left: length(0.0), right: auto(), top: length(0.0), bottom: auto() },
            Size { width: percent(1.0), height: percent(1.0) },
            Rect::zero(),
            Point { x: 10.0, y: 15.0 },
            Size { width: 174.0, height: 124.0 },
        ),
    ];
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for flex_direction in
                [FlexDirection::Row, FlexDirection::RowReverse, FlexDirection::Column, FlexDirection::ColumnReverse]
            {
                for flex_wrap in [FlexWrap::Wrap, FlexWrap::WrapReverse] {
                    for (inset, size, margin, expected_location, expected_size) in cases {
                        let mut root = TestNode::container(
                            Display::Flex,
                            Style {
                                direction,
                                flex_direction,
                                flex_wrap,
                                size: Size { width: length(200.0), height: length(160.0) },
                                border,
                                padding: Rect {
                                    left: length(9.0),
                                    right: length(5.0),
                                    top: length(3.0),
                                    bottom: length(7.0),
                                },
                                ..Style::default()
                            },
                            gutters,
                        );
                        root.writing_mode = mode;
                        let mut child = TestNode::leaf(
                            Style { position: Position::Absolute, direction, inset, size, margin, ..Style::default() },
                            Size::ZERO,
                        );
                        child.writing_mode = mode;
                        let mut tree = TestTree::new(root, child);
                        tree.compute(Size::MAX_CONTENT);
                        let context = format!("{mode:?} {direction:?} {flex_direction:?} {flex_wrap:?} {inset:?}");
                        assert_eq!(tree.layout(1).location, expected_location, "{context}");
                        assert_eq!(tree.layout(1).size, expected_size, "{context}");
                    }
                }
            }
        }
    }
}

fn percentage_child() -> TestNode {
    TestNode::leaf(Style { size: Size { width: percent(1.0), height: percent(1.0) }, ..Style::default() }, Size::ZERO)
}

fn overflowing_child() -> TestNode {
    TestNode::leaf(
        Style { size: Size { width: length(400.0), height: length(200.0) }, flex_shrink: 0.0, ..Style::default() },
        Size::ZERO,
    )
}

fn absolutely_positioned_overflowing_child() -> TestNode {
    TestNode::leaf(
        Style {
            position: Position::Absolute,
            inset: Rect { left: length(0.0), right: auto(), top: length(0.0), bottom: auto() },
            size: Size { width: length(400.0), height: length(200.0) },
            ..Style::default()
        },
        Size::ZERO,
    )
}

fn definite_container(display: Display, insets: Rect<f32>) -> TestNode {
    TestNode::container(
        display,
        Style { size: Size { width: length(200.0), height: length(100.0) }, ..Style::default() },
        insets,
    )
}

#[test]
fn physical_insets_participate_in_block_flex_and_grid_layout() {
    let insets = Rect { left: 15.0, right: 15.0, top: 0.0, bottom: 15.0 };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        let mut tree = TestTree::new(definite_container(display, insets), percentage_child());
        tree.compute(Size::MAX_CONTENT);

        let container = tree.layout(0);
        let child = tree.layout(1);
        assert_eq!(container.size, Size { width: 200.0, height: 100.0 }, "{display:?}");
        assert_eq!(container.scrollbar_size, Size { width: 30.0, height: 15.0 }, "{display:?}");
        assert_eq!(child.location, Point { x: 15.0, y: 0.0 }, "{display:?}");
        assert_eq!(child.size, Size { width: 170.0, height: 85.0 }, "{display:?}");
    }
}

#[test]
fn leading_physical_insets_do_not_create_phantom_scrollable_overflow() {
    let insets = Rect { left: 7.0, right: 11.0, top: 13.0, bottom: 17.0 };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut root = definite_container(display, insets);
            root.style.direction = direction;
            let mut tree = TestTree::new(root, percentage_child());
            tree.compute(Size::MAX_CONTENT);

            let container = tree.layout(0);
            assert_eq!(container.content_size, Size { width: 182.0, height: 70.0 }, "{display:?} {direction:?}");
            assert_eq!(container.scroll_width(), 0.0, "{display:?} {direction:?}");
            assert_eq!(container.scroll_height(), 0.0, "{display:?} {direction:?}");
        }
    }
}

#[test]
fn physical_insets_reduce_the_scrollport_without_inflating_overflow_content() {
    let insets = Rect { left: 7.0, right: 11.0, top: 13.0, bottom: 17.0 };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut root = definite_container(display, insets);
            root.style.direction = direction;
            let mut tree = TestTree::new(root, overflowing_child());
            tree.compute(Size::MAX_CONTENT);

            let container = tree.layout(0);
            assert_eq!(container.content_size, Size { width: 400.0, height: 200.0 }, "{display:?} {direction:?}");
            assert_eq!(container.scroll_width(), 218.0, "{display:?} {direction:?}");
            assert_eq!(container.scroll_height(), 130.0, "{display:?} {direction:?}");
        }
    }
}

#[test]
fn absolute_overflow_is_also_relative_to_the_inner_scrollport_origin() {
    let insets = Rect { left: 7.0, right: 11.0, top: 13.0, bottom: 17.0 };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        // Keep one in-flow item so this regression isolates the positioned
        // contribution origin from Grid's separate empty-in-flow behavior.
        let mut tree = TestTree::new(definite_container(display, insets), percentage_child());
        tree.nodes.push(absolutely_positioned_overflowing_child());
        tree.nodes[0].children.push(2);
        tree.compute(Size::MAX_CONTENT);

        let container = tree.layout(0);
        assert_eq!(container.content_size, Size { width: 400.0, height: 200.0 }, "{display:?}");
        assert_eq!(container.scroll_width(), 218.0, "{display:?}");
        assert_eq!(container.scroll_height(), 130.0, "{display:?}");
    }
}

#[test]
fn physical_insets_compose_with_padding_border_and_content_box_sizing() {
    let insets = Rect { left: 7.0, right: 11.0, top: 13.0, bottom: 17.0 };
    let edge = Rect { left: length(3.0), right: length(5.0), top: length(2.0), bottom: length(4.0) };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        let root = TestNode::container(
            display,
            Style {
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: length(200.0), height: length(100.0) },
                padding: edge,
                border: edge,
                ..Style::default()
            },
            insets,
        );
        let mut tree = TestTree::new(root, percentage_child());
        tree.compute(Size::MAX_CONTENT);

        let child = tree.layout(1);
        assert_eq!(child.location, Point { x: 13.0, y: 17.0 }, "{display:?}");
        assert_eq!(child.size, Size { width: 182.0, height: 70.0 }, "{display:?}");
    }
}

#[test]
fn physical_insets_participate_in_auto_and_min_max_block_sizing() {
    let insets = Rect { left: 0.0, right: 0.0, top: 9.0, bottom: 11.0 };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        let root = TestNode::container(
            display,
            Style {
                size: Size { width: length(100.0), height: auto() },
                min_size: Size { width: auto(), height: length(50.0) },
                max_size: Size { width: auto(), height: length(55.0) },
                ..Style::default()
            },
            insets,
        );
        let child = TestNode::leaf(
            Style { size: Size { width: length(10.0), height: length(20.0) }, ..Style::default() },
            Size::ZERO,
        );
        let mut tree = TestTree::new(root, child);
        tree.compute(Size::MAX_CONTENT);

        assert_eq!(tree.layout(0).size.height, 50.0, "{display:?}");
        assert_eq!(tree.layout(1).location.y, 9.0, "{display:?}");
    }
}

#[test]
fn physical_insets_apply_after_outer_aspect_ratio_resolution() {
    let insets = Rect { left: 15.0, right: 15.0, top: 15.0, bottom: 15.0 };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        let root = TestNode::container(
            display,
            Style { size: Size { width: length(200.0), height: auto() }, aspect_ratio: Some(2.0), ..Style::default() },
            insets,
        );
        let mut tree = TestTree::new(root, percentage_child());
        tree.compute(Size::MAX_CONTENT);

        assert_eq!(tree.layout(0).size, Size { width: 200.0, height: 100.0 }, "{display:?}");
        assert_eq!(tree.layout(1).location, Point { x: 15.0, y: 15.0 }, "{display:?}");
        assert_eq!(tree.layout(1).size, Size { width: 170.0, height: 70.0 }, "{display:?}");
    }
}
