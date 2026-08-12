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

    fn layout_vertical_absolute_text(
        parent_display: Display,
        inline_size: Dimension,
        containing_height: f32,
    ) -> Layout {
        let mut tree = new_test_tree();
        let absolute = tree
            .new_leaf_with_context(
                Style {
                    display: Display::Block,
                    position: Position::Absolute,
                    size: Size { width: auto(), height: inline_size },
                    inset: Rect { left: length(0.0), right: auto(), top: length(0.0), bottom: auto() },
                    ..Default::default()
                },
                TestNodeContext::ahem_text(WRAPPABLE_400.to_owned(), WritingMode::Vertical),
            )
            .unwrap();
        tree.set_writing_mode(absolute, taffy::WritingMode::VerticalLr).unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: parent_display,
                    size: Size::from_lengths(100.0, containing_height),
                    ..Default::default()
                },
                &[absolute],
            )
            .unwrap();

        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(containing_height) },
            test_measure_function,
        )
        .unwrap();
        *tree.layout(absolute).unwrap()
    }

    #[test]
    fn absolute_intrinsic_inline_size_follows_vertical_writing_mode() {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            let min_content = layout_vertical_absolute_text(display, Dimension::min_content(), 236.0);
            assert_eq!(min_content.size, Size { width: 40.0, height: 100.0 }, "{display:?} min-content");

            let max_content = layout_vertical_absolute_text(display, Dimension::max_content(), 236.0);
            assert_eq!(max_content.size, Size { width: 10.0, height: 400.0 }, "{display:?} max-content");

            let fit_content = layout_vertical_absolute_text(display, Dimension::fit_content(), 236.0);
            assert_eq!(fit_content.size, Size { width: 20.0, height: 236.0 }, "{display:?} fit-content");
        }
    }

    #[test]
    fn orthogonal_absolute_margins_follow_the_positioned_boxes_logical_axes() {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            let mut tree = new_test_tree();
            tree.disable_rounding();
            let absolute = tree
                .new_leaf(Style {
                    display: Display::Block,
                    position: Position::Absolute,
                    size: Size::from_lengths(40.0, 20.0),
                    inset: Rect { left: length(10.0), right: length(10.0), top: length(10.0), bottom: length(10.0) },
                    margin: Rect { left: auto(), right: auto(), top: auto(), bottom: auto() },
                    ..Default::default()
                })
                .unwrap();
            tree.set_writing_mode(absolute, taffy::WritingMode::VerticalLr).unwrap();
            let root = tree
                .new_with_children(
                    Style { display, size: Size::from_lengths(100.0, 80.0), ..Default::default() },
                    &[absolute],
                )
                .unwrap();

            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

            let layout = tree.layout(absolute).unwrap();
            assert_eq!(layout.location, Point { x: 30.0, y: 30.0 }, "{display:?}");
            assert_eq!(layout.margin, Rect { left: 20.0, right: 20.0, top: 20.0, bottom: 20.0 }, "{display:?}");
        }
    }

    fn layout_orthogonal_absolute_overflow(
        display: Display,
        container_direction: Direction,
        child_direction: Direction,
        writing_mode: taffy::WritingMode,
        size: Size<f32>,
    ) -> Layout {
        let mut tree = new_test_tree();
        tree.disable_rounding();
        let absolute = tree
            .new_leaf(Style {
                display: Display::Block,
                position: Position::Absolute,
                direction: child_direction,
                size: size.map(length),
                inset: Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: length(0.0) },
                margin: Rect { left: auto(), right: auto(), top: auto(), bottom: auto() },
                ..Default::default()
            })
            .unwrap();
        tree.set_writing_mode(absolute, writing_mode).unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display,
                    direction: container_direction,
                    size: Size::from_lengths(100.0, 100.0),
                    ..Default::default()
                },
                &[absolute],
            )
            .unwrap();

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        *tree.layout(absolute).unwrap()
    }

    #[test]
    fn orthogonal_absolute_inline_overflow_shares_containing_block_direction_space() {
        for writing_mode in [taffy::WritingMode::VerticalLr, taffy::WritingMode::VerticalRl] {
            for container_direction in [Direction::Ltr, Direction::Rtl] {
                for child_direction in [Direction::Ltr, Direction::Rtl] {
                    for display in [Display::Block, Display::Flex, Display::Grid] {
                        let layout = layout_orthogonal_absolute_overflow(
                            display,
                            container_direction,
                            child_direction,
                            writing_mode,
                            Size { width: 20.0, height: 120.0 },
                        );
                        assert_eq!(
                            layout.location,
                            Point { x: 40.0, y: -10.0 },
                            "{display:?} {writing_mode:?} {container_direction:?} {child_direction:?}"
                        );
                        assert_eq!(
                            layout.margin,
                            Rect { left: 40.0, right: 40.0, top: -10.0, bottom: -10.0 },
                            "{display:?} {writing_mode:?} {container_direction:?} {child_direction:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn orthogonal_absolute_block_overflow_uses_containing_inline_dominance() {
        for writing_mode in [taffy::WritingMode::VerticalLr, taffy::WritingMode::VerticalRl] {
            for container_direction in [Direction::Ltr, Direction::Rtl] {
                for child_direction in [Direction::Ltr, Direction::Rtl] {
                    for display in [Display::Block, Display::Flex, Display::Grid] {
                        let layout = layout_orthogonal_absolute_overflow(
                            display,
                            container_direction,
                            child_direction,
                            writing_mode,
                            Size { width: 120.0, height: 20.0 },
                        );
                        let (x, left, right) = match container_direction {
                            Direction::Ltr => (0.0, 0.0, -20.0),
                            Direction::Rtl => (-20.0, -20.0, 0.0),
                        };
                        assert_eq!(
                            layout.location,
                            Point { x, y: 40.0 },
                            "{display:?} {writing_mode:?} {container_direction:?} {child_direction:?}"
                        );
                        assert_eq!(
                            layout.margin,
                            Rect { left, right, top: 40.0, bottom: 40.0 },
                            "{display:?} {writing_mode:?} {container_direction:?} {child_direction:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn absolute_overconstraint_uses_containing_blocks_physical_start_sides() {
        for writing_mode in [
            taffy::WritingMode::HorizontalTb,
            taffy::WritingMode::VerticalLr,
            taffy::WritingMode::VerticalRl,
            taffy::WritingMode::SidewaysLr,
            taffy::WritingMode::SidewaysRl,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                for display in [Display::Block, Display::Flex, Display::Grid] {
                    let mut tree = new_test_tree();
                    tree.disable_rounding();
                    let absolute = tree
                        .new_leaf(Style {
                            display: Display::Block,
                            position: Position::Absolute,
                            direction,
                            size: Size::from_lengths(120.0, 120.0),
                            inset: Rect {
                                left: length(0.0),
                                right: length(0.0),
                                top: length(0.0),
                                bottom: length(0.0),
                            },
                            ..Default::default()
                        })
                        .unwrap();
                    tree.set_writing_mode(absolute, writing_mode).unwrap();
                    let root = tree
                        .new_with_children(
                            Style { display, direction, size: Size::from_lengths(100.0, 100.0), ..Default::default() },
                            &[absolute],
                        )
                        .unwrap();
                    tree.set_writing_mode(root, writing_mode).unwrap();

                    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

                    let expected = Point {
                        x: if writing_mode.is_axis_flow_reversed(taffy::AbsoluteAxis::Horizontal, direction) {
                            -20.0
                        } else {
                            0.0
                        },
                        y: if writing_mode.is_axis_flow_reversed(taffy::AbsoluteAxis::Vertical, direction) {
                            -20.0
                        } else {
                            0.0
                        },
                    };
                    assert_eq!(
                        tree.layout(absolute).unwrap().location,
                        expected,
                        "{display:?} {writing_mode:?} {direction:?}"
                    );
                }
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
