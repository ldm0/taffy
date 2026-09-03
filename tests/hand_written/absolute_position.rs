#[cfg(all(test, feature = "block_layout", feature = "flexbox"))]
mod absolute_position {
    use taffy::prelude::*;
    use taffy::style::Direction;
    use taffy::Point;
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

    fn ratio_only_replaced_measure(
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        _node_id: NodeId,
        _context: Option<&mut TestNodeContext>,
        _style: &Style,
    ) -> Size<f32> {
        let width = known_dimensions.width.unwrap_or(match available_space.width {
            AvailableSpace::Definite(width) => width,
            AvailableSpace::MinContent | AvailableSpace::MaxContent => 0.0,
        });
        Size { width, height: known_dimensions.height.unwrap_or(width) }
    }

    #[test]
    fn absolute_replaced_leaf_owns_ratio_only_sizing_against_the_imcb() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            let mut tree = new_test_tree();
            tree.disable_rounding();
            let absolute = tree
                .new_leaf_with_context(
                    Style {
                        display: Display::Block,
                        position: Position::Absolute,
                        item_is_replaced: true,
                        box_sizing: BoxSizing::ContentBox,
                        aspect_ratio: Some(1.0),
                        inset: Rect { left: length(20.0), right: auto(), top: length(30.0), bottom: auto() },
                        margin: Rect::length(5.0),
                        border: Rect::length(10.0),
                        ..Style::default()
                    },
                    TestNodeContext::zero(),
                )
                .unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        display: container_display,
                        size: Size { width: length(220.0), height: length(190.0) },
                        ..Style::default()
                    },
                    &[absolute],
                )
                .unwrap();

            tree.compute_layout_with_measure(root, Size::MAX_CONTENT, ratio_only_replaced_measure).unwrap();

            let layout = tree.layout(absolute).unwrap();
            assert_eq!(layout.location, Point { x: 25.0, y: 35.0 }, "{container_display:?}");
            assert_eq!(layout.size, Size { width: 190.0, height: 190.0 }, "{container_display:?}");
        }
    }

    fn absolute_intrinsic_block_constraint(
        container_display: Display,
        preferred_block_size: Dimension,
        min_block_size: Dimension,
        max_block_size: Dimension,
        block_insets: Line<LengthPercentageAuto>,
        aspect_ratio: Option<f32>,
    ) -> Layout {
        let mut tree = new_test_tree();
        tree.disable_rounding();
        let content = tree
            .new_leaf(Style { size: Size { width: length(40.0), height: length(80.0) }, ..Default::default() })
            .unwrap();
        let absolute = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    position: Position::Absolute,
                    size: Size { width: length(100.0), height: preferred_block_size },
                    min_size: Size { width: auto(), height: min_block_size },
                    max_size: Size { width: auto(), height: max_block_size },
                    inset: Rect { top: block_insets.start, bottom: block_insets.end, ..Rect::auto() },
                    aspect_ratio,
                    ..Default::default()
                },
                &[content],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: container_display,
                    size: Size { width: length(300.0), height: length(200.0) },
                    ..Default::default()
                },
                &[absolute],
            )
            .unwrap();

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        *tree.layout(absolute).unwrap()
    }

    #[test]
    fn absolute_intrinsic_preferred_block_size_uses_the_content_contribution() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            let layout = absolute_intrinsic_block_constraint(
                container_display,
                Dimension::max_content(),
                auto(),
                auto(),
                Line::AUTO,
                None,
            );
            assert_eq!(layout.size, Size { width: 100.0, height: 80.0 });
        }
    }

    #[test]
    fn absolute_min_content_block_constraint_clamps_a_definite_preferred_size() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            let layout = absolute_intrinsic_block_constraint(
                container_display,
                length(0.0),
                Dimension::max_content(),
                auto(),
                Line::AUTO,
                None,
            );
            assert_eq!(layout.size, Size { width: 100.0, height: 80.0 });
        }
    }

    #[test]
    fn absolute_max_content_block_constraint_clamps_a_definite_preferred_size() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            let layout = absolute_intrinsic_block_constraint(
                container_display,
                length(160.0),
                auto(),
                Dimension::min_content(),
                Line::AUTO,
                None,
            );
            assert_eq!(layout.size, Size { width: 100.0, height: 80.0 });
        }
    }

    #[test]
    fn absolute_intrinsic_maximum_clamps_inset_stretch() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            let layout = absolute_intrinsic_block_constraint(
                container_display,
                auto(),
                auto(),
                Dimension::max_content(),
                Line { start: length(10.0), end: length(10.0) },
                None,
            );
            assert_eq!(layout.size, Size { width: 100.0, height: 80.0 });
        }
    }

    #[test]
    fn absolute_aspect_ratio_observes_the_content_based_automatic_minimum() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            let layout =
                absolute_intrinsic_block_constraint(container_display, auto(), auto(), auto(), Line::AUTO, Some(2.0));
            assert_eq!(layout.size, Size { width: 100.0, height: 80.0 });
        }
    }

    fn absolute_percentage_child_block_size(
        container_display: Display,
        preferred_block_size: Dimension,
        block_insets: Line<LengthPercentageAuto>,
        block_margins: Line<LengthPercentageAuto>,
        content_block_size: f32,
    ) -> (Layout, Layout) {
        let mut tree = new_test_tree();
        tree.disable_rounding();
        let content = tree
            .new_leaf(Style {
                size: Size { width: length(40.0), height: length(content_block_size) },
                ..Default::default()
            })
            .unwrap();
        let percentage_child = tree
            .new_with_children(
                Style { size: Size { width: auto(), height: percent(1.0) }, ..Default::default() },
                &[content],
            )
            .unwrap();
        let absolute = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    position: Position::Absolute,
                    size: Size { width: length(100.0), height: preferred_block_size },
                    inset: Rect {
                        left: length(0.0),
                        right: length(0.0),
                        top: block_insets.start,
                        bottom: block_insets.end,
                    },
                    margin: Rect {
                        left: length(0.0),
                        right: length(0.0),
                        top: block_margins.start,
                        bottom: block_margins.end,
                    },
                    ..Default::default()
                },
                &[percentage_child],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: container_display,
                    size: Size { width: length(300.0), height: length(200.0) },
                    ..Default::default()
                },
                &[absolute],
            )
            .unwrap();

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        (*tree.layout(absolute).unwrap(), *tree.layout(percentage_child).unwrap())
    }

    #[test]
    fn absolute_intrinsic_block_size_keeps_percentage_children_indefinite() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            for (preferred, expected) in
                [(Dimension::fit_content(), 80.0), (Dimension::max_content(), 60.0), (Dimension::min_content(), 40.0)]
            {
                let (absolute, percentage_child) = absolute_percentage_child_block_size(
                    container_display,
                    preferred,
                    Line { start: length(0.0), end: length(0.0) },
                    Line { start: length(0.0), end: length(0.0) },
                    expected,
                );
                assert_eq!(absolute.size.height, expected, "{container_display:?} {preferred:?}");
                assert_eq!(percentage_child.size.height, expected, "{container_display:?} {preferred:?}");
            }
        }
    }

    #[test]
    fn absolute_inset_stretch_remains_definite_for_percentage_children() {
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            for preferred in [auto(), Dimension::stretch()] {
                let (absolute, percentage_child) = absolute_percentage_child_block_size(
                    container_display,
                    preferred,
                    Line { start: length(0.0), end: length(0.0) },
                    Line { start: length(0.0), end: length(0.0) },
                    80.0,
                );
                assert_eq!(absolute.size.height, 200.0, "{container_display:?} {preferred:?}");
                assert_eq!(percentage_child.size.height, 200.0, "{container_display:?} {preferred:?}");
            }
        }
    }

    #[test]
    fn absolute_authored_stretch_uses_the_inset_modified_containing_block() {
        let zero_margins = Line { start: length(0.0), end: length(0.0) };
        for container_display in [Display::Block, Display::Flex, Display::Grid] {
            for (label, insets, margins, expected_y, expected_height) in [
                ("static", Line::AUTO, zero_margins, 0.0, 200.0),
                ("start", Line { start: length(10.0), end: auto() }, zero_margins, 10.0, 190.0),
                ("end", Line { start: auto(), end: length(10.0) }, zero_margins, 0.0, 190.0),
                ("both", Line { start: length(10.0), end: length(20.0) }, zero_margins, 10.0, 170.0),
                (
                    "margins",
                    Line { start: length(10.0), end: auto() },
                    Line { start: length(7.0), end: length(11.0) },
                    17.0,
                    172.0,
                ),
            ] {
                let (absolute, percentage_child) = absolute_percentage_child_block_size(
                    container_display,
                    Dimension::stretch(),
                    insets,
                    margins,
                    80.0,
                );
                assert_eq!(absolute.location.y, expected_y, "{container_display:?} {label}");
                assert_eq!(absolute.size.height, expected_height, "{container_display:?} {label}");
                assert_eq!(percentage_child.size.height, expected_height, "{container_display:?} {label}");
            }
        }
    }

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
    fn absolute_auto_margins_center_a_box_wider_than_the_remaining_free_space() {
        let (tree, absolute) =
            absolute_box(Direction::Ltr, Size { width: 1440.0, height: 100.0 }, Size { width: 975.0, height: 20.0 });

        let layout = tree.layout(absolute).unwrap();
        assert_eq!(layout.location.x, 232.5);
        assert_eq!(layout.margin.left, 232.5);
        assert_eq!(layout.margin.right, 232.5);
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
