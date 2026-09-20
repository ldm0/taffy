use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{RequestedAxis, RunMode};

#[test]
fn grid_absolute_child_measurement_and_layout_exclude_margins_and_insets() {
    for (inset, expected) in [
        (Rect::auto(), Size { width: 90.0, height: 110.0 }),
        (Rect { left: length(10.0), top: length(15.0), ..Rect::auto() }, Size { width: 80.0, height: 95.0 }),
        (Rect { right: length(10.0), bottom: length(15.0), ..Rect::auto() }, Size { width: 80.0, height: 95.0 }),
        (
            Rect { left: length(10.0), right: length(20.0), top: length(15.0), bottom: length(25.0) },
            Size { width: 60.0, height: 70.0 },
        ),
        (Rect { left: length(-10.0), top: length(-15.0), ..Rect::auto() }, Size { width: 100.0, height: 125.0 }),
        (Rect { left: percent(0.1), top: percent(0.25), ..Rect::auto() }, Size { width: 80.0, height: 80.0 }),
        (Rect { left: length(110.0), top: length(130.0), ..Rect::auto() }, Size { width: 0.0, height: 0.0 }),
    ] {
        let root = TestNode::container(
            Display::Grid,
            Style { size: Size::from_lengths(100.0, 120.0), ..Style::default() },
            Rect::ZERO,
        );
        let child = TestNode::leaf(
            Style { position: Position::Absolute, inset, margin: Rect::length(5.0), ..Style::default() },
            Size { width: 20.0, height: 30.0 },
        );
        let mut tree = TestTree::new(root, child);
        tree.compute(Size::MAX_CONTENT);

        let inputs: Vec<_> =
            tree.nodes[1].layout_inputs.iter().filter(|input| input.axis == RequestedAxis::Both).collect();
        assert!(!inputs.is_empty());
        assert!(inputs.iter().any(|input| input.run_mode == RunMode::PerformLayout));
        if inset.right.is_auto() || inset.bottom.is_auto() {
            assert!(inputs.iter().any(|input| input.run_mode == RunMode::ComputeSize));
        }
        for input in inputs {
            assert_eq!(input.parent_size, Size { width: Some(100.0), height: Some(120.0) });
            assert_eq!(input.available_space, expected.map(AvailableSpace::Definite), "inset: {inset:?}");
        }
    }
}

#[test]
fn relative_grid_item_insets_do_not_reduce_available_space() {
    let root = TestNode::container(
        Display::Grid,
        Style { size: Size::from_lengths(100.0, 120.0), ..Style::default() },
        Rect::ZERO,
    );
    let child = TestNode::leaf(
        Style {
            inset: Rect { left: length(10.0), top: length(15.0), ..Rect::auto() },
            margin: Rect::length(5.0),
            ..Style::default()
        },
        Size { width: 20.0, height: 30.0 },
    );
    let mut tree = TestTree::new(root, child);
    tree.compute(Size::MAX_CONTENT);

    let input = tree.nodes[1].layout_inputs.iter().find(|input| input.run_mode == RunMode::PerformLayout).unwrap();
    assert_eq!(
        input.available_space,
        Size { width: AvailableSpace::Definite(90.0), height: AvailableSpace::Definite(110.0) }
    );
}
