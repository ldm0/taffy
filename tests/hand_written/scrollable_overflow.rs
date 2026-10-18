//! The scrollable overflow rectangle captures start-side (negative) overflow as well as
//! end-side overflow. Boxes wholly in the unreachable scrollable overflow region are
//! excluded for scroll containers, while boxes only partially in the unreachable region
//! contribute their whole border box.

use taffy::geometry::Point;
use taffy::prelude::*;
use taffy::style::{Direction, Display, Overflow, Position};
use taffy_test_helpers::new_test_tree;

const CONTAINER: f32 = 200.0;
const CHILD: f32 = 50.0;

fn container_with_absolute_child(
    display: Display,
    overflow: Overflow,
    direction: Direction,
    inset_left: Option<f32>,
    inset_right: Option<f32>,
) -> Layout {
    let mut tree = new_test_tree();
    let child = tree
        .new_leaf(Style {
            position: Position::Absolute,
            inset: Rect {
                left: inset_left.map(length).unwrap_or(auto()),
                right: inset_right.map(length).unwrap_or(auto()),
                top: length(0.0),
                bottom: auto(),
            },
            size: Size { width: length(CHILD), height: length(CHILD) },
            ..Default::default()
        })
        .unwrap();
    let node = tree
        .new_with_children(
            Style {
                display,
                direction,
                position: Position::Relative,
                size: Size { width: length(CONTAINER), height: length(CONTAINER) },
                overflow: Point { x: overflow, y: overflow },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(node, Size::MAX_CONTENT).unwrap();
    *tree.layout(node).unwrap()
}

#[test]
fn wholly_unreachable_boxes_are_excluded_for_scroll_containers() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // Child entirely before the scroll origin (x from -100 to -50)
        let layout = container_with_absolute_child(display, Overflow::Scroll, Direction::Ltr, Some(-100.0), None);

        assert_eq!(layout.scrollable_overflow_rect, Rect::ZERO, "{display:?}");
        assert_eq!(layout.scroll_width(), 0.0, "{display:?}");
    }
}

#[test]
fn wholly_unreachable_boxes_contribute_to_non_scroll_containers() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // A box that is not a scroll container has no unreachable region of its own, so
        // start-side overflow is captured in its scrollable overflow rect.
        let layout = container_with_absolute_child(display, Overflow::Visible, Direction::Ltr, Some(-100.0), None);

        assert_eq!(layout.scrollable_overflow_rect.left, -100.0, "{display:?}");
    }
}

#[test]
fn partially_unreachable_boxes_contribute_their_whole_border_box() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // Child straddling the scroll origin (x from -25 to 25): its whole border box is
        // part of the scrollable overflow area, even though the negative part is unreachable.
        let layout = container_with_absolute_child(display, Overflow::Scroll, Direction::Ltr, Some(-25.0), None);

        assert_eq!(layout.scrollable_overflow_rect.left, -25.0, "{display:?}");
        assert_eq!(layout.scrollable_overflow_rect.right, 25.0, "{display:?}");
        // Only the reachable extent counts towards the scroll width
        assert_eq!(layout.scroll_width(), 0.0, "{display:?}");
    }
}

#[test]
fn end_side_overflow_is_captured_and_scrollable() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // Child extending past the end edge (x from 200 to 250)
        let layout = container_with_absolute_child(display, Overflow::Scroll, Direction::Ltr, Some(CONTAINER), None);

        assert_eq!(layout.scrollable_overflow_rect.right, CONTAINER + CHILD, "{display:?}");
        assert_eq!(layout.scroll_width(), CHILD, "{display:?}");
    }
}

#[test]
fn absolute_only_containers_include_scroll_container_end_padding() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let mut tree = new_test_tree();
        let abs = tree
            .new_leaf(Style {
                position: Position::Absolute,
                inset: Rect { left: length(0.0), right: auto(), top: length(0.0), bottom: auto() },
                size: Size { width: length(5.0), height: length(5.0) },
                ..Default::default()
            })
            .unwrap();
        let node = tree
            .new_with_children(
                Style {
                    display,
                    position: Position::Relative,
                    overflow: Point { x: Overflow::Scroll, y: Overflow::Scroll },
                    padding: Rect { left: length(0.0), right: length(20.0), top: length(0.0), bottom: length(30.0) },
                    size: Size { width: length(CONTAINER), height: length(CONTAINER) },
                    ..Default::default()
                },
                &[abs],
            )
            .unwrap();

        tree.compute_layout(node, Size::MAX_CONTENT).unwrap();
        let layout = tree.layout(node).unwrap();

        assert_eq!(layout.scrollable_overflow_rect.right, 20.0, "{display:?}");
        assert_eq!(layout.scrollable_overflow_rect.bottom, 30.0, "{display:?}");
    }
}

fn nested_overflow_layout(display: Display, child_overflow: Point<Overflow>) -> Layout {
    let mut tree = new_test_tree();
    // Grandchild overflowing the child in the y axis (extends to y=300 in a 200-high child)
    let grandchild = tree
        .new_leaf(Style {
            position: Position::Absolute,
            inset: Rect { left: length(0.0), right: auto(), top: length(CONTAINER), bottom: auto() },
            size: Size { width: length(CHILD), height: length(CHILD) },
            ..Default::default()
        })
        .unwrap();
    let child = tree
        .new_with_children(
            Style {
                display,
                position: Position::Relative,
                size: Size { width: length(CONTAINER), height: length(CONTAINER) },
                overflow: child_overflow,
                ..Default::default()
            },
            &[grandchild],
        )
        .unwrap();
    let node = tree
        .new_with_children(
            Style { display, size: Size { width: length(CONTAINER), height: length(CONTAINER) }, ..Default::default() },
            &[child],
        )
        .unwrap();

    tree.compute_layout(node, Size::MAX_CONTENT).unwrap();
    *tree.layout(node).unwrap()
}

#[test]
fn scroll_container_in_one_axis_clips_both_axes() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // `Hidden`/`Scroll` in either axis makes the child a scroll container, which clips
        // overflow in both axes: the grandchild's y overflow must not propagate.
        let layout = nested_overflow_layout(display, Point { x: Overflow::Hidden, y: Overflow::Visible });

        assert_eq!(layout.scrollable_overflow_rect.bottom, CONTAINER, "{display:?}");
    }
}

#[test]
fn clip_in_one_axis_does_not_clip_the_other() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // `Clip` does not establish a scroll container: overflow in the other (visible) axis
        // still propagates.
        let layout = nested_overflow_layout(display, Point { x: Overflow::Clip, y: Overflow::Visible });

        assert_eq!(layout.scrollable_overflow_rect.bottom, CONTAINER + CHILD, "{display:?}");
    }
}

#[test]
fn wholly_unreachable_boxes_are_excluded_for_rtl_scroll_containers() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // In RTL the scroll origin is the top-right corner: a child entirely past the
        // *right* edge is wholly unreachable.
        let layout = container_with_absolute_child(display, Overflow::Scroll, Direction::Rtl, None, Some(-100.0));

        assert_eq!(layout.scrollable_overflow_rect, Rect::ZERO, "{display:?}");
        assert_eq!(layout.scroll_width(), 0.0, "{display:?}");
    }
}

#[test]
fn end_side_overflow_is_captured_for_rtl_scroll_containers() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        // In RTL, overflow past the *left* edge is reachable end-side overflow.
        let layout = container_with_absolute_child(display, Overflow::Scroll, Direction::Rtl, Some(-CHILD), None);

        assert_eq!(layout.scrollable_overflow_rect.right, CONTAINER + CHILD, "{display:?}");
        assert_eq!(layout.scroll_width(), CHILD, "{display:?}");
    }
}

#[test]
fn flex_scroll_origins_follow_reversed_main_and_cross_flows() {
    use taffy::WritingMode;
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for flex_direction in
                [FlexDirection::Row, FlexDirection::RowReverse, FlexDirection::Column, FlexDirection::ColumnReverse]
            {
                for wrap in [FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse] {
                    let mut tree = TaffyTree::<()>::new();
                    let items: Vec<_> = (0..3)
                        .map(|_| {
                            tree.new_leaf(Style {
                                min_size: Size { width: length(110.0), height: length(110.0) },
                                ..Style::default()
                            })
                            .unwrap()
                        })
                        .collect();
                    let root = tree
                        .new_with_children(
                            Style {
                                display: Display::Flex,
                                direction,
                                flex_direction,
                                flex_wrap: wrap,
                                align_items: AlignItems::START,
                                gap: Size { width: length(10.0), height: length(10.0) },
                                size: Size::from_lengths(100.0, 100.0),
                                overflow: Point { x: Overflow::Scroll, y: Overflow::Scroll },
                                ..Style::default()
                            },
                            &items,
                        )
                        .unwrap();
                    tree.set_writing_mode(root, mode).unwrap();
                    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                    let main_horizontal = matches!(flex_direction, FlexDirection::Row | FlexDirection::RowReverse)
                        == mode.is_horizontal();
                    let long_width = main_horizontal == (wrap == FlexWrap::NoWrap);
                    let rect = tree.layout(root).unwrap().scrollable_overflow_rect;
                    let label = format!("{mode:?}/{direction:?}/{flex_direction:?}/{wrap:?}");
                    // The overflowing item's short axis must remain 110px.
                    // This is the previously passing assertion lost by filtering
                    // negative main-axis contributions in the new shared pass.
                    assert_eq!(if long_width { rect.bottom } else { rect.right }, 110.0, "{label}");
                    if wrap == FlexWrap::NoWrap {
                        assert_eq!(if long_width { rect.right } else { rect.bottom }, 350.0, "{label}");
                    }
                }
            }
        }
    }
}
