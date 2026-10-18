//! Percentage-dependent block children whose containing block is intrinsically sized.
#[cfg(test)]
mod tests {
    use taffy::prelude::*;
    use taffy::{Float, TaffyTree};
    use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

    fn block_style() -> Style {
        Style { display: Display::Block, ..Default::default() }
    }

    fn fixed_zero_root() -> Style {
        Style { display: Display::Block, size: Size { width: length(0.0), height: auto() }, ..Default::default() }
    }

    fn floated_block() -> Style {
        Style { display: Display::Block, float: Float::Left, ..Default::default() }
    }

    fn layout(taffy: &mut TaffyTree<TestNodeContext>, root: NodeId) {
        taffy.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    }

    #[test]
    fn percentage_preferred_width_is_reresolved_after_intrinsic_parent_width() {
        let mut taffy = new_test_tree();
        let atom = taffy.new_leaf_with_context(block_style(), TestNodeContext::fixed(60.0, 20.0)).unwrap();
        let inner = taffy
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: percent(0.5), height: auto() },
                    ..Default::default()
                },
                &[atom],
            )
            .unwrap();
        let outer = taffy.new_with_children(floated_block(), &[inner]).unwrap();
        let root = taffy.new_with_children(fixed_zero_root(), &[outer]).unwrap();

        layout(&mut taffy, root);

        assert_eq!(taffy.layout(outer).unwrap().size.width, 60.0);
        assert_eq!(taffy.layout(inner).unwrap().size.width, 30.0);
    }

    #[test]
    fn measured_content_contribution_is_clamped_by_min_width() {
        let mut taffy = new_test_tree();
        let atom = taffy.new_leaf_with_context(block_style(), TestNodeContext::fixed(60.0, 20.0)).unwrap();
        let inner = taffy
            .new_with_children(
                Style {
                    display: Display::Block,
                    min_size: Size { width: length(160.0), height: auto() },
                    ..Default::default()
                },
                &[atom],
            )
            .unwrap();
        let outer = taffy.new_with_children(floated_block(), &[inner]).unwrap();
        let root = taffy.new_with_children(fixed_zero_root(), &[outer]).unwrap();

        layout(&mut taffy, root);

        assert_eq!(taffy.layout(outer).unwrap().size.width, 160.0);
        assert_eq!(taffy.layout(inner).unwrap().size.width, 160.0);
    }

    #[test]
    fn used_block_height_can_remain_indefinite_for_inflow_percentages() {
        use crate::hand_written::test_tree::{TestNode, TestTree};

        for indefinite in [false, true] {
            let mut root = TestNode::container(
                Display::Block,
                Style {
                    position: Position::Relative,
                    size: Size { width: length(100.0), height: length(120.0) },
                    padding: Rect::length(5.0),
                    border: Rect::length(5.0),
                    ..Style::default()
                },
                Rect::ZERO,
            );
            root.indefinite_block_percentage_basis = indefinite;
            let child = TestNode::leaf(
                Style { size: Size { width: length(10.0), height: percent(0.5) }, ..Style::default() },
                Size { width: 10.0, height: 10.0 },
            );
            let mut tree = TestTree::new(root, child.clone());
            let mut absolute = child;
            absolute.style.position = Position::Absolute;
            tree.nodes.push(absolute);
            tree.nodes[0].children.push(2);
            tree.nodes.push(TestNode::leaf(
                Style { position: Position::Absolute, inset: Rect::length(0.0), ..Style::default() },
                Size::ZERO,
            ));
            tree.nodes[0].children.push(3);

            tree.compute(Size::MAX_CONTENT);

            assert_eq!(tree.layout(0).size.height, 120.0);
            assert_eq!(tree.layout(1).size.height, if indefinite { 10.0 } else { 50.0 });
            // Positioned descendants resolve against the final padding box.
            assert_eq!(tree.layout(2).size.height, 55.0);
            assert_eq!(tree.layout(3).size.height, 110.0);
        }
    }
}
