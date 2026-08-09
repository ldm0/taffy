#[cfg(all(test, feature = "block_layout", feature = "flexbox"))]
mod positioned_constraints {
    use taffy::prelude::*;
    use taffy::style::Direction;
    use taffy_test_helpers::{new_test_tree, TestNodeContext};

    fn absolute_box(
        direction: Direction,
        containing_size: Size<f32>,
        child_size: Size<f32>,
    ) -> (TaffyTree<TestNodeContext>, NodeId) {
        let mut tree = new_test_tree();
        tree.disable_rounding();
        let absolute = tree
            .new_leaf(Style {
                display: Display::Block,
                position: Position::Absolute,
                size: child_size.map(length),
                inset: Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: length(0.0) },
                margin: Rect { left: auto(), right: auto(), top: auto(), bottom: auto() },
                ..Default::default()
            })
            .unwrap();
        let root = tree
            .new_with_children(
                Style { display: Display::Block, direction, size: containing_size.map(length), ..Default::default() },
                &[absolute],
            )
            .unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        (tree, absolute)
    }

    #[test]
    fn absolute_auto_margins_center_a_box_with_positive_free_space() {
        let (tree, absolute) =
            absolute_box(Direction::Ltr, Size { width: 1440.0, height: 100.0 }, Size { width: 975.0, height: 20.0 });

        let layout = tree.layout(absolute).unwrap();
        assert_eq!(layout.location.x, 232.5);
        assert_eq!((layout.margin.left, layout.margin.right), (232.5, 232.5));
    }

    #[test]
    fn overflowing_absolute_auto_margins_preserve_the_inline_start_edge() {
        let (ltr, ltr_absolute) =
            absolute_box(Direction::Ltr, Size { width: 100.0, height: 100.0 }, Size { width: 150.0, height: 20.0 });
        let ltr_layout = ltr.layout(ltr_absolute).unwrap();
        assert_eq!(ltr_layout.location.x, 0.0);
        assert_eq!((ltr_layout.margin.left, ltr_layout.margin.right), (0.0, -50.0));

        let (rtl, rtl_absolute) =
            absolute_box(Direction::Rtl, Size { width: 100.0, height: 100.0 }, Size { width: 150.0, height: 20.0 });
        let rtl_layout = rtl.layout(rtl_absolute).unwrap();
        assert_eq!(rtl_layout.location.x, -50.0);
        assert_eq!((rtl_layout.margin.left, rtl_layout.margin.right), (-50.0, 0.0));
    }

    #[test]
    fn overflowing_absolute_block_axis_auto_margins_share_negative_space() {
        let (tree, absolute) =
            absolute_box(Direction::Ltr, Size { width: 100.0, height: 100.0 }, Size { width: 20.0, height: 120.0 });

        let layout = tree.layout(absolute).unwrap();
        assert_eq!(layout.location.y, -10.0);
        assert_eq!((layout.margin.top, layout.margin.bottom), (-10.0, -10.0));
    }
}
