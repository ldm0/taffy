//! Sizing of absolutely positioned replaced (`item_is_replaced: true`) boxes.
//!
//! An `auto` width/height of an absolutely positioned replaced element resolves to its
//! intrinsic size rather than being stretched between its insets
//! (<https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-width>,
//! <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-height>).
#[cfg(test)]
mod absolute_replaced {
    use taffy::prelude::*;
    use taffy::{Point, TaffyTree};
    use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

    // A 100x50 image (height = width * 0.5)
    const IMAGE: TestNodeContext = TestNodeContext::aspect_ratio(100.0, 0.5);

    fn container() -> Style {
        Style {
            display: Display::Block,
            position: Position::Relative,
            size: Size { width: length(400.0), height: length(300.0) },
            ..Default::default()
        }
    }

    fn all_insets(value: f32) -> Rect<LengthPercentageAuto> {
        Rect { left: length(value), right: length(value), top: length(value), bottom: length(value) }
    }

    fn layout_child(child_style: Style) -> Layout {
        let mut taffy: TaffyTree<TestNodeContext> = new_test_tree();
        let child = taffy.new_leaf_with_context(child_style, IMAGE).unwrap();
        let root = taffy.new_with_children(container(), &[child]).unwrap();
        taffy.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
        *taffy.layout(child).unwrap()
    }

    #[test]
    fn auto_size_uses_intrinsic_size_despite_insets() {
        let layout = layout_child(Style {
            position: Position::Absolute,
            item_is_replaced: true,
            inset: all_insets(10.0),
            ..Default::default()
        });
        assert_eq!(layout.size, Size { width: 100.0, height: 50.0 });
        assert_eq!(layout.location, Point { x: 10.0, y: 10.0 });
    }

    #[test]
    fn auto_height_uses_aspect_ratio_despite_insets() {
        let layout = layout_child(Style {
            position: Position::Absolute,
            item_is_replaced: true,
            inset: all_insets(10.0),
            size: Size { width: length(200.0), height: auto() },
            ..Default::default()
        });
        assert_eq!(layout.size, Size { width: 200.0, height: 100.0 });
    }

    #[test]
    fn non_replaced_auto_size_is_stretched_between_insets() {
        let layout =
            layout_child(Style { position: Position::Absolute, inset: all_insets(10.0), ..Default::default() });
        assert_eq!(layout.size, Size { width: 380.0, height: 280.0 });
    }
}

#[test]
fn absolute_ratio_uses_the_tree_resolved_natural_ratio() {
    use super::test_tree::{TestNode, TestTree};
    use taffy::prelude::*;
    use taffy::ResolvedAspectRatio;
    let root = TestNode::container(
        Display::Block,
        Style {
            position: Position::Relative,
            size: Size { width: length(400.0), height: length(300.0) },
            ..Style::default()
        },
        Rect::ZERO,
    );
    let mut child = TestNode::leaf(
        Style {
            position: Position::Absolute,
            item_is_replaced: true,
            aspect_ratio: Some(10.0),
            size: Size { width: auto(), height: length(100.0) },
            ..Style::default()
        },
        Size { width: 200.0, height: 200.0 },
    );
    child.resolved_aspect_ratio = ResolvedAspectRatio::new(1.0, BoxSizing::ContentBox);
    let mut tree = TestTree::new(root, child);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(1).size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn absolute_ratio_resolves_auto_height_after_fit_content_width() {
    use super::test_tree::{TestNode, TestTree};
    use taffy::prelude::*;
    let root = TestNode::container(
        Display::Block,
        Style {
            position: Position::Relative,
            size: Size { width: length(400.0), height: length(100.0) },
            ..Style::default()
        },
        Rect::ZERO,
    );
    let child = TestNode::container(
        Display::Block,
        Style {
            position: Position::Absolute,
            aspect_ratio: Some(1.0),
            max_size: Size { width: auto(), height: length(100.0) },
            ..Style::default()
        },
        Rect::ZERO,
    );
    let mut tree = TestTree::new(root, child);
    tree.nodes.push(TestNode::leaf(
        Style {
            display: Display::Block,
            size: Size { width: length(200.0), height: length(20.0) },
            ..Style::default()
        },
        Size { width: 200.0, height: 20.0 },
    ));
    tree.nodes[1].children.push(2);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(1).size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn absolute_safe_alignment_follows_vertical_containing_block_start_edges() {
    use taffy::prelude::*;
    use taffy::{Direction, Point, WritingMode};
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for flex_direction in [FlexDirection::Row, FlexDirection::Column] {
                let mut tree = TaffyTree::<()>::new();
                let child = tree
                    .new_leaf(Style {
                        position: Position::Absolute,
                        box_sizing: BoxSizing::ContentBox,
                        size: Size { width: length(65.0), height: length(65.0) },
                        border: Rect::length(2.0),
                        align_self: Some(AlignSelf::SAFE_CENTER),
                        ..Style::default()
                    })
                    .unwrap();
                tree.set_writing_mode(child, mode).unwrap();
                let root = tree
                    .new_with_children(
                        Style {
                            display: Display::Flex,
                            position: Position::Relative,
                            direction,
                            flex_direction,
                            box_sizing: BoxSizing::ContentBox,
                            size: Size { width: length(50.0), height: length(50.0) },
                            border: Rect::length(3.0),
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                tree.set_writing_mode(root, mode).unwrap();
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                assert_eq!(
                    tree.layout(child).unwrap().location,
                    Point {
                        x: if mode == WritingMode::VerticalRl { -16.0 } else { 3.0 },
                        y: if direction == Direction::Rtl { -16.0 } else { 3.0 },
                    },
                    "{mode:?}/{direction:?}/{flex_direction:?}"
                );
            }
        }
    }
}

#[test]
fn absolute_stretch_height_is_definite_for_percentage_descendants() {
    use taffy::prelude::*;
    let mut tree = TaffyTree::<()>::new();
    let inner = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: auto(), height: percent(1.0) },
            ..Style::default()
        })
        .unwrap();
    let child = tree
        .new_with_children(
            Style {
                display: Display::Block,
                position: Position::Absolute,
                inset: Rect::length(0.0),
                size: Size { width: auto(), height: Dimension::stretch() },
                ..Style::default()
            },
            &[inner],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                position: Position::Relative,
                size: Size { width: length(100.0), height: length(120.0) },
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(child).unwrap().size.height, 120.0);
    assert_eq!(tree.layout(inner).unwrap().size.height, 120.0);
}
