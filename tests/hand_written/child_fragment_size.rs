//! A custom formatting context can have a structural minimum independent of
//! CSS preferred/min/max size (for example, HTML's rendered fieldset legend).
//! Positioning and publication must use the final child fragment consistently.

use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{Direction, LogicalSize, Point, WritingMode};

const MODES: [WritingMode; 5] = [
    WritingMode::HorizontalTb,
    WritingMode::VerticalRl,
    WritingMode::VerticalLr,
    WritingMode::SidewaysRl,
    WritingMode::SidewaysLr,
];

fn custom_child(mode: WritingMode, position: Position, block_floor: f32) -> TestNode {
    let mut child = TestNode::leaf(
        Style {
            position,
            size: mode.to_physical(LogicalSize { inline_size: length(30.0), block_size: length(0.0) }),
            ..Style::default()
        },
        Size::ZERO,
    );
    child.writing_mode = mode;
    child.fragment_size_floor = Some(mode.to_physical(LogicalSize { inline_size: 30.0, block_size: block_floor }));
    child
}

fn container(mode: WritingMode, direction: Direction, display: Display) -> TestNode {
    let mut root = TestNode::container(
        display,
        Style {
            direction,
            size: Size { width: length(100.0), height: length(100.0) },
            grid_template_columns: vec![length(30.0), length(70.0)],
            grid_template_rows: vec![length(20.0), length(80.0)],
            ..Style::default()
        },
        Rect::ZERO,
    );
    root.writing_mode = mode;
    root
}

#[test]
fn absolute_insets_and_margins_use_the_final_child_fragment_size() {
    for mode in MODES {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for display in [Display::Block, Display::Flex, Display::Grid] {
                let mut child = custom_child(mode, Position::Absolute, 40.0);
                child.style.inset = Rect { left: auto(), right: length(10.0), top: auto(), bottom: length(12.0) };
                child.style.margin =
                    Rect { left: length(2.0), right: length(4.0), top: length(6.0), bottom: length(8.0) };
                let expected_size = child.fragment_size_floor.unwrap();
                let mut tree = TestTree::new(container(mode, direction, display), child);
                tree.compute(Size::MAX_CONTENT);
                let layout = tree.layout(1);
                assert_eq!(layout.size, expected_size, "{mode:?}/{direction:?}/{display:?}");
                assert_eq!(
                    layout.location,
                    Point { x: 100.0 - 10.0 - 4.0 - expected_size.width, y: 100.0 - 12.0 - 8.0 - expected_size.height },
                    "{mode:?}/{direction:?}/{display:?}"
                );
                assert_eq!(layout.margin, Rect { left: 2.0, right: 4.0, top: 6.0, bottom: 8.0 });
            }
        }
    }
}

#[test]
fn flex_absolute_alignment_uses_the_final_child_fragment_size() {
    for mode in MODES {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for own_safe_alignment in [false, true] {
                let mut root = container(mode, direction, Display::Flex);
                root.style.justify_content = Some(JustifyContent::CENTER);
                root.style.align_items = Some(AlignItems::SAFE_CENTER);
                let mut child = custom_child(mode, Position::Absolute, 140.0);
                if own_safe_alignment {
                    child.style.align_self = Some(AlignSelf::SAFE_CENTER);
                }
                let size = child.fragment_size_floor.unwrap();
                let mut tree = TestTree::new(root, child);
                tree.compute(Size::MAX_CONTENT);
                assert_eq!(tree.layout(1).size, size, "{mode:?}/{direction:?}");
                // The parent contributes the static edge but not its safety.
                // Only the child's own safe alignment falls back to block-start.
                let block_offset = if own_safe_alignment { 0.0 } else { -20.0 };
                let expected = match mode {
                    WritingMode::HorizontalTb => Point { x: 35.0, y: block_offset },
                    WritingMode::VerticalRl | WritingMode::SidewaysRl => Point { x: -40.0 - block_offset, y: 35.0 },
                    WritingMode::VerticalLr | WritingMode::SidewaysLr => Point { x: block_offset, y: 35.0 },
                };
                assert_eq!(tree.layout(1).location, expected, "{mode:?}/{direction:?}");
            }
        }
    }
}

#[test]
fn absolute_auto_margins_use_the_final_child_fragment_size() {
    for mode in MODES {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for display in [Display::Block, Display::Flex, Display::Grid] {
                let mut child = custom_child(mode, Position::Absolute, 40.0);
                child.style.inset =
                    Rect { left: length(10.0), right: length(10.0), top: length(10.0), bottom: length(10.0) };
                child.style.margin = Rect::auto();
                child.style.max_size = mode.to_physical(LogicalSize { inline_size: auto(), block_size: length(10.0) });
                let size = child.fragment_size_floor.unwrap();
                let mut tree = TestTree::new(container(mode, direction, display), child);
                tree.compute(Size::MAX_CONTENT);
                let layout = tree.layout(1);
                assert_eq!(layout.size, size, "{mode:?}/{direction:?}/{display:?}");
                assert_eq!(
                    layout.location,
                    Point { x: (100.0 - size.width) / 2.0, y: (100.0 - size.height) / 2.0 },
                    "{mode:?}/{direction:?}/{display:?}"
                );
                assert_eq!(layout.margin.left, (80.0 - size.width) / 2.0);
                assert_eq!(layout.margin.right, layout.margin.left);
                assert_eq!(layout.margin.top, (80.0 - size.height) / 2.0);
                assert_eq!(layout.margin.bottom, layout.margin.top);
            }
        }
    }
}

#[test]
fn grid_area_alignment_uses_the_final_child_fragment_size() {
    for mode in MODES {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for position in [Position::Relative, Position::Absolute] {
                let root = container(mode, direction, Display::Grid);
                let mut child = custom_child(mode, position, 40.0);
                child.style.grid_column = Line { start: line(2), end: line(3) };
                child.style.grid_row = Line { start: line(2), end: line(3) };
                child.style.justify_self = Some(JustifySelf::END);
                child.style.align_self = Some(AlignSelf::END);
                let size = child.fragment_size_floor.unwrap();
                let mut tree = TestTree::new(root, child);
                tree.compute(Size::MAX_CONTENT);
                assert_eq!(tree.layout(1).size, size, "{mode:?}/{direction:?}/{position:?}");
                let inline_reversed = (direction == Direction::Rtl) != (mode == WritingMode::SidewaysLr);
                let inline_offset = if inline_reversed { 0.0 } else { 70.0 };
                let expected = match mode {
                    WritingMode::HorizontalTb => Point { x: inline_offset, y: 60.0 },
                    WritingMode::VerticalRl | WritingMode::SidewaysRl => Point { x: 0.0, y: inline_offset },
                    WritingMode::VerticalLr | WritingMode::SidewaysLr => Point { x: 60.0, y: inline_offset },
                };
                assert_eq!(tree.layout(1).location, expected, "{mode:?}/{direction:?}/{position:?}");
            }
        }
    }
}
