#[cfg(test)]
mod caching {
    use taffy::prelude::*;
    use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

    const NODE_CONTEXT: TestNodeContext = TestNodeContext::fixed(50.0, 50.0);

    #[test]
    fn measure_count_flexbox() {
        let mut taffy = new_test_tree();

        let leaf = taffy.new_leaf_with_context(Style::default(), NODE_CONTEXT).unwrap();

        let mut node = taffy.new_with_children(Style::DEFAULT, &[leaf]).unwrap();
        for _ in 0..100 {
            node = taffy.new_with_children(Style::DEFAULT, &[node]).unwrap();
        }

        taffy.compute_layout_with_measure(node, Size::MAX_CONTENT, test_measure_function).unwrap();

        // Intrinsic and final flex passes now retain distinct definite-size
        // provenance. Four additional leaf measurements are the bounded cost
        // of preventing those semantically different spaces from aliasing.
        assert_eq!(taffy.get_node_context_mut(leaf).unwrap().count, 11);
    }

    #[test]
    #[cfg(feature = "grid")]
    fn measure_count_grid() {
        let mut taffy = new_test_tree();

        let style = || Style { display: Display::Grid, ..Default::default() };
        let leaf = taffy.new_leaf_with_context(style(), NODE_CONTEXT).unwrap();

        let mut node = taffy.new_with_children(Style::DEFAULT, &[leaf]).unwrap();
        for _ in 0..100 {
            node = taffy.new_with_children(Style::DEFAULT, &[node]).unwrap();
        }

        taffy.compute_layout_with_measure(node, Size::MAX_CONTENT, test_measure_function).unwrap();
        // The Grid leaf is nested below default flex containers, so it crosses
        // the same intrinsic/final definite-size boundaries as the flex case.
        assert_eq!(taffy.get_node_context_mut(leaf).unwrap().count, 11);
    }
}
