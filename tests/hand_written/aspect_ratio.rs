use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::ResolvedAspectRatio;

#[test]
fn resolved_aspect_ratio_rejects_invalid_values() {
    for ratio in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(ResolvedAspectRatio::new(ratio, BoxSizing::ContentBox), None);
    }
}

#[test]
fn block_ratio_automatic_minimum_keeps_the_content_height() {
    let root = TestNode::container(
        Display::Block,
        Style { size: Size { width: length(200.0), height: auto() }, ..Style::default() },
        Rect::ZERO,
    );
    let child = TestNode::container(
        Display::Block,
        Style {
            box_sizing: BoxSizing::BorderBox,
            size: Size { width: length(100.0), height: auto() },
            aspect_ratio: Some(2.0),
            padding: Rect::length(10.0),
            border: Rect::length(10.0),
            ..Style::default()
        },
        Rect::ZERO,
    );
    let mut tree = TestTree::new(root, child);
    tree.nodes.push(TestNode::leaf(
        Style { size: Size { width: auto(), height: length(60.0) }, ..Style::default() },
        Size::ZERO,
    ));
    tree.nodes[1].children.push(2);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(1).size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn block_ratio_transfers_do_not_override_an_explicit_width_or_minimum() {
    let root = TestNode::container(
        Display::Block,
        Style { size: Size { width: length(200.0), height: auto() }, ..Style::default() },
        Rect::ZERO,
    );
    let child = TestNode::container(
        Display::Block,
        Style {
            size: Size { width: length(100.0), height: auto() },
            min_size: Size { width: auto(), height: length(100.0) },
            max_size: Size { width: length(150.0), height: auto() },
            aspect_ratio: Some(2.0),
            ..Style::default()
        },
        Rect::ZERO,
    );
    let mut tree = TestTree::new(root, child);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(1).size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn flex_ratio_transfers_leave_explicit_axes_independent() {
    for horizontal in [false, true] {
        let root = TestNode::container(
            Display::Flex,
            Style { size: Size::from_lengths(40.0, 40.0), flex_direction: FlexDirection::Column, ..Style::default() },
            Rect::ZERO,
        );
        let child = TestNode::leaf(
            Style {
                size: if horizontal {
                    Size { width: length(30.0), height: auto() }
                } else {
                    Size { width: auto(), height: length(30.0) }
                },
                max_size: if horizontal {
                    Size { width: auto(), height: length(10.0) }
                } else {
                    Size { width: length(10.0), height: auto() }
                },
                min_size: Size { width: length(0.0), height: length(0.0) },
                aspect_ratio: Some(1.0),
                item_is_replaced: true,
                ..Style::default()
            },
            Size { width: 16.0, height: 16.0 },
        );
        let mut tree = TestTree::new(root, child);
        tree.compute(Size::MAX_CONTENT);
        let expected = if horizontal { Size { width: 30.0, height: 10.0 } } else { Size { width: 10.0, height: 30.0 } };
        assert_eq!(tree.layout(1).size, expected, "horizontal={horizontal}");
    }
}

#[test]
fn resolved_aspect_ratio_sizing_box_flows_through_block_flex_and_grid_items() {
    let edges = Rect { left: length(5.0), right: length(5.0), top: length(5.0), bottom: length(5.0) };
    let ratio_child = |sizing_box| {
        let mut node = TestNode::leaf(
            Style {
                box_sizing: BoxSizing::BorderBox,
                size: Size { width: length(100.0), height: auto() },
                padding: edges,
                border: edges,
                // Deliberately disagree with the node-level used ratio. This
                // proves each algorithm queries the integration seam instead
                // of reconstructing the ratio from Style.
                aspect_ratio: Some(4.0),
                align_self: Some(AlignSelf::FLEX_START),
                justify_self: Some(AlignSelf::FLEX_START),
                ..Style::default()
            },
            Size::ZERO,
        );
        node.resolved_aspect_ratio = ResolvedAspectRatio::new(2.0, sizing_box);
        node
    };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        let root = TestNode::container(
            display,
            Style { size: Size { width: length(400.0), height: length(400.0) }, ..Style::default() },
            Rect::ZERO,
        );
        let mut tree = TestTree::new(root, ratio_child(BoxSizing::ContentBox));
        tree.nodes.push(ratio_child(BoxSizing::BorderBox));
        tree.nodes[0].children.push(2);
        tree.compute(Size::MAX_CONTENT);

        assert_eq!(tree.layout(1).size, Size { width: 100.0, height: 60.0 }, "{display:?} content-box ratio");
        assert_eq!(tree.layout(2).size, Size { width: 100.0, height: 50.0 }, "{display:?} border-box ratio");
    }
}
