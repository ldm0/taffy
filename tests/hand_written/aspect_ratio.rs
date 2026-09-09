use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{
    LayoutInput, LogicalSize, RequestedAxis, ResolvedAspectRatio, RunMode, SizingMode, SizingPurpose, WritingMode,
};

#[test]
fn ratio_dependent_inline_minimum_is_resolved_before_root_size_becomes_fixed() {
    let mut tree = TaffyTree::<()>::new();
    let content =
        tree.new_leaf(Style { size: Size { width: length(100.0), height: length(20.0) }, ..Style::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: auto(), height: length(100.0) },
                aspect_ratio: Some(0.5),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    tree.compute_layout(root, Size { width: AvailableSpace::Definite(240.0), height: AvailableSpace::MaxContent })
        .unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn ratio_dependent_inline_minimum_preserves_parent_assigned_sizes() {
    let root = TestNode::container(
        Display::Block,
        Style { size: Size { width: auto(), height: length(100.0) }, aspect_ratio: Some(0.5), ..Style::default() },
        Rect::ZERO,
    );
    let content = TestNode::leaf(
        Style { size: Size { width: length(100.0), height: length(20.0) }, ..Style::default() },
        Size::ZERO,
    );
    let mut tree = TestTree::new(root, content);
    for (known_inline, expected) in [(None, 100.0), (Some(25.0), 25.0)] {
        let measured = tree.compute_child_size(
            NodeId::from(0_usize),
            LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode: SizingMode::InherentSize,
                sizing_purpose: SizingPurpose::IntrinsicContribution,
                axis: RequestedAxis::Horizontal,
                block_auto_behavior: taffy::AutoSizeBehavior::FitContent,
                known_dimensions: Size { width: known_inline, height: None },
                definite_dimensions: Size { width: known_inline, height: None },
                parent_size: Size { width: Some(240.0), height: None },
                parent_writing_mode: WritingMode::HorizontalTb,
                available_space: Size::MAX_CONTENT,
                block_margins_are_collapsible: Line::FALSE,
            },
        );
        assert_eq!(measured.size.width, expected, "known inline size {known_inline:?}");
    }
}

#[test]
fn ratio_dependent_inline_minimum_obeys_explicit_limits_and_overflow() {
    for (name, min, max, overflow, expected) in [
        ("auto", auto(), auto(), taffy::Overflow::Visible, 100.0),
        ("zero", length(0.0), auto(), taffy::Overflow::Visible, 50.0),
        ("minimum", length(120.0), auto(), taffy::Overflow::Visible, 120.0),
        ("maximum", auto(), length(75.0), taffy::Overflow::Visible, 75.0),
        ("small maximum", auto(), length(25.0), taffy::Overflow::Visible, 25.0),
        ("minimum wins", length(80.0), length(75.0), taffy::Overflow::Visible, 80.0),
        ("hidden", auto(), auto(), taffy::Overflow::Hidden, 50.0),
        ("clip", auto(), auto(), taffy::Overflow::Clip, 100.0),
    ] {
        let mut tree = TaffyTree::<()>::new();
        let content = tree
            .new_leaf(Style { size: Size { width: length(100.0), height: length(20.0) }, ..Style::default() })
            .unwrap();
        let ratio = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: auto(), height: length(100.0) },
                    min_size: Size { width: min, height: auto() },
                    max_size: Size { width: max, height: auto() },
                    overflow: taffy::Point { x: overflow, y: overflow },
                    aspect_ratio: Some(0.5),
                    ..Style::default()
                },
                &[content],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: length(240.0), height: auto() },
                    ..Style::default()
                },
                &[ratio],
            )
            .unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(ratio).unwrap().size, Size { width: expected, height: 100.0 }, "{name}");
    }
}

#[test]
fn ratio_dependent_inline_size_respects_the_automatic_content_minimum() {
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            let mut tree = TaffyTree::<()>::new();
            let content = tree
                .new_leaf(Style {
                    display: Display::Block,
                    size: mode.to_physical(LogicalSize { inline_size: length(100.0), block_size: length(20.0) }),
                    ..Style::default()
                })
                .unwrap();
            let ratio = tree
                .new_with_children(
                    Style {
                        display,
                        size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: length(100.0) }),
                        aspect_ratio: Some(if mode.is_horizontal() { 0.5 } else { 2.0 }),
                        ..Style::default()
                    },
                    &[content],
                )
                .unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        display: Display::Block,
                        size: mode.to_physical(LogicalSize { inline_size: length(240.0), block_size: length(180.0) }),
                        ..Style::default()
                    },
                    &[ratio],
                )
                .unwrap();
            for node in [root, ratio, content] {
                tree.set_writing_mode(node, mode).unwrap();
            }
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(ratio).unwrap().size, Size { width: 100.0, height: 100.0 }, "{mode:?} {display:?}");
        }
    }
}

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
