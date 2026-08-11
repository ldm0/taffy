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
        taffy.compute_layout_with_measure(root, Size::MAX_CONTENT, test_measure_function).unwrap();
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
}
