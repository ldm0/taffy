//! Anonymous block wrappers forward a percentage basis without acquiring its used height.

use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{Cache, LayoutInput, RunMode, SizingMode, SizingPurpose};

fn block(style: Style) -> TestNode {
    TestNode::container(Display::Block, style, Rect::ZERO)
}

fn anonymous() -> TestNode {
    let mut node = block(Style::default());
    node.anonymous_block = true;
    node
}

fn tree(child: TestNode) -> TestTree {
    let root = block(Style {
        box_sizing: BoxSizing::ContentBox,
        size: Size { width: length(240.0), height: length(200.0) },
        padding: Rect::length(10.0),
        border: Rect::length(5.0),
        ..Style::default()
    });
    let mut tree = TestTree::new(root, anonymous());
    tree.nodes[1].children.push(2);
    tree.nodes.push(child);
    tree
}

#[test]
fn anonymous_wrapper_keeps_content_height_and_uses_parent_content_box_percentages() {
    let child = TestNode::leaf(
        Style {
            box_sizing: BoxSizing::ContentBox,
            size: Size { width: length(100.0), height: percent(0.5) },
            border: Rect::length(10.0),
            ..Style::default()
        },
        Size::ZERO,
    );
    let mut tree = tree(child);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(2).size, Size { width: 120.0, height: 120.0 });
    assert_eq!(tree.layout(1).size, Size { width: 240.0, height: 120.0 });
}

#[test]
fn nested_anonymous_wrappers_forward_but_ordinary_auto_blocks_stop_the_basis() {
    for middle_is_anonymous in [true, false] {
        let mut tree = tree(anonymous());
        tree.nodes[2].anonymous_block = middle_is_anonymous;
        tree.nodes[2].children.push(3);
        tree.nodes.push(TestNode::leaf(
            Style { size: Size { width: length(40.0), height: percent(0.5) }, ..Style::default() },
            Size { width: 40.0, height: 12.0 },
        ));
        tree.compute(Size::MAX_CONTENT);
        let height = if middle_is_anonymous { 100.0 } else { 12.0 };
        assert_eq!(tree.layout(3).size.height, height);
        assert_eq!(tree.layout(2).size.height, height);
        assert_eq!(tree.layout(1).size.height, height);
    }
}

#[test]
fn anonymous_wrapper_forwards_percentage_min_max_constraints() {
    let child = TestNode::leaf(
        Style {
            size: Size { width: length(40.0), height: length(300.0) },
            min_size: Size { width: auto(), height: percent(0.75) },
            max_size: Size { width: auto(), height: percent(0.5) },
            ..Style::default()
        },
        Size::ZERO,
    );
    let mut tree = tree(child);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(2).size.height, 150.0, "minimum wins over maximum");
    assert_eq!(tree.layout(1).size.height, 150.0);
}

#[test]
fn relative_insets_use_the_forwarded_basis_without_changing_flow_height() {
    let child = TestNode::leaf(
        Style {
            size: Size { width: length(40.0), height: length(40.0) },
            inset: Rect { top: percent(0.5), ..Rect::auto() },
            ..Style::default()
        },
        Size::ZERO,
    );
    let mut tree = tree(child);
    tree.compute(Size::MAX_CONTENT);
    assert_eq!(tree.layout(2).location.y, 100.0);
    assert_eq!(tree.layout(1).size.height, 40.0);
}

#[test]
fn forwarded_percentage_height_contributes_to_intrinsic_width_through_a_ratio() {
    let child = TestNode::leaf(
        Style { size: Size { width: auto(), height: percent(0.5) }, aspect_ratio: Some(2.0), ..Style::default() },
        Size::ZERO,
    );
    let mut tree = TestTree::new(anonymous(), child);
    // The embedding supplies a definite containing-block height while asking
    // this wrapper for its intrinsic width. No used wrapper width is fixed.
    let output = tree.compute_child_layout(
        NodeId::from(0_usize),
        LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            sizing_purpose: SizingPurpose::IntrinsicContribution,
            parent_size: Size { width: None, height: Some(200.0) },
            ..LayoutInput::HIDDEN
        },
    );
    assert_eq!(output.size, Size { width: 200.0, height: 100.0 });
}

#[test]
fn forwarded_block_constraint_changes_invalidate_measurements() {
    let child = TestNode::leaf(
        Style { size: Size { width: length(40.0), height: percent(0.5) }, ..Style::default() },
        Size::ZERO,
    );
    let mut tree = tree(child);
    let mut inputs = LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: SizingMode::InherentSize,
        sizing_purpose: SizingPurpose::IntrinsicContribution,
        known_dimensions: Size { width: Some(240.0), height: None },
        parent_size: Size { width: Some(240.0), height: Some(200.0) },
        ..LayoutInput::HIDDEN
    };
    let output = tree.compute_child_layout(NodeId::from(1_usize), inputs);
    assert_eq!(output.size.height, 100.0);
    let mut cache = Cache::new();
    cache.store(&inputs, output);
    assert!(cache.get(&inputs).is_some());
    inputs.parent_size.height = Some(320.0);
    assert!(cache.get(&inputs).is_none());
    assert_eq!(tree.compute_child_layout(NodeId::from(1_usize), inputs).size.height, 160.0);
}
