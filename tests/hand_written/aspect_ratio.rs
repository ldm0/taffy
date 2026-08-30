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
