#[cfg(all(test, feature = "block_layout", feature = "flexbox"))]
mod absolute_position {
    use taffy::prelude::*;
    use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

    struct Fixture {
        tree: TaffyTree<TestNodeContext>,
        absolute: NodeId,
        percentage_block: NodeId,
        flex: NodeId,
        text: NodeId,
    }

    fn layout_fixture(
        parent_display: Display,
        containing_width: f32,
        absolute_style: Style,
        text_content: &str,
    ) -> Fixture {
        let mut tree = new_test_tree();
        let text = tree
            .new_leaf_with_context(
                Style::default(),
                TestNodeContext::ahem_text(text_content.to_owned(), WritingMode::Horizontal),
            )
            .unwrap();
        let flex = tree.new_with_children(Style { display: Display::Flex, ..Default::default() }, &[text]).unwrap();
        let percentage_block = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: percent(1.0), height: auto() },
                    ..Default::default()
                },
                &[flex],
            )
            .unwrap();
        let absolute = tree
            .new_with_children(
                Style { display: Display::Block, position: Position::Absolute, ..absolute_style },
                &[percentage_block],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: parent_display,
                    size: Size { width: length(containing_width), height: length(100.0) },
                    ..Default::default()
                },
                &[absolute],
            )
            .unwrap();

        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, test_measure_function).unwrap();

        Fixture { tree, absolute, percentage_block, flex, text }
    }

    const WRAPPABLE_400: &str = "aaaaaaaaaa\u{200b}bbbbbbbbbb\u{200b}cccccccccc\u{200b}dddddddddd";

    #[test]
    fn auto_width_absolute_block_shrinks_nested_flex_content_to_available_width() {
        let fixture = layout_fixture(Display::Block, 236.0, Style::default(), WRAPPABLE_400);

        assert_eq!(fixture.tree.layout(fixture.absolute).unwrap().size.width, 236.0);
        assert_eq!(fixture.tree.layout(fixture.percentage_block).unwrap().size.width, 236.0);
        assert_eq!(fixture.tree.layout(fixture.flex).unwrap().size.width, 236.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.width, 236.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.height, 20.0);
    }

    #[test]
    fn auto_width_absolute_flex_child_shrinks_nested_content_to_available_width() {
        let fixture = layout_fixture(Display::Flex, 236.0, Style::default(), WRAPPABLE_400);

        assert_eq!(fixture.tree.layout(fixture.absolute).unwrap().size.width, 236.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.width, 236.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.height, 20.0);
    }

    #[test]
    fn auto_width_absolute_block_uses_max_content_width_when_it_fits() {
        let fixture = layout_fixture(Display::Block, 500.0, Style::default(), WRAPPABLE_400);

        assert_eq!(fixture.tree.layout(fixture.absolute).unwrap().size.width, 400.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.width, 400.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.height, 10.0);
    }

    #[test]
    fn auto_width_absolute_block_preserves_min_content_overflow() {
        let fixture = layout_fixture(Display::Block, 50.0, Style::default(), WRAPPABLE_400);

        assert_eq!(fixture.tree.layout(fixture.absolute).unwrap().size.width, 100.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.width, 100.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.height, 40.0);
    }

    #[test]
    fn auto_width_absolute_block_subtracts_inset_and_margins_from_available_width() {
        let fixture = layout_fixture(
            Display::Block,
            236.0,
            Style {
                inset: Rect { left: length(36.0), right: auto(), top: auto(), bottom: auto() },
                margin: Rect { left: length(10.0), right: length(10.0), top: auto(), bottom: auto() },
                ..Default::default()
            },
            WRAPPABLE_400,
        );

        let absolute = fixture.tree.layout(fixture.absolute).unwrap();
        assert_eq!(absolute.location.x, 46.0);
        assert_eq!(absolute.size.width, 180.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.width, 180.0);
        assert_eq!(fixture.tree.layout(fixture.text).unwrap().size.height, 40.0);
    }
}
