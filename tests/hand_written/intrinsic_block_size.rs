use taffy::prelude::*;
use taffy::{Overflow, Point, WritingMode};
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

fn layout_subject_with_content_size(
    subject_style: Style,
    writing_mode: WritingMode,
    content_size: Size<f32>,
) -> Size<f32> {
    let mut tree = new_test_tree();
    let content = tree
        .new_leaf(Style {
            size: Size::from_lengths(content_size.width, content_size.height),
            flex_shrink: 0.0,
            ..Default::default()
        })
        .unwrap();
    let subject = tree.new_with_children(subject_style, &[content]).unwrap();
    tree.set_writing_mode(subject, writing_mode).unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size::from_lengths(300.0, 300.0), ..Default::default() },
            &[subject],
        )
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(300.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(subject).unwrap().size
}

fn layout_subject_in_formatting_context(
    parent_display: Display,
    subject_style: Style,
    writing_mode: WritingMode,
    content_size: Size<f32>,
) -> Size<f32> {
    let mut tree = new_test_tree();
    let content = tree
        .new_leaf(Style {
            size: Size::from_lengths(content_size.width, content_size.height),
            flex_shrink: 0.0,
            ..Default::default()
        })
        .unwrap();
    let subject = tree.new_with_children(subject_style, &[content]).unwrap();
    tree.set_writing_mode(subject, writing_mode).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: parent_display,
                size: Size::from_lengths(300.0, 300.0),
                align_items: Some(AlignItems::START),
                justify_items: Some(AlignItems::START),
                ..Default::default()
            },
            &[subject],
        )
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(300.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(subject).unwrap().size
}

fn layout_subject(subject_style: Style, writing_mode: WritingMode) -> Size<f32> {
    layout_subject_with_content_size(subject_style, writing_mode, Size { width: 100.0, height: 100.0 })
}

fn layout_ratio_subject(
    display: Display,
    preferred_block_size: Dimension,
    min_block_size: Dimension,
    max_block_size: Dimension,
    ratio: f32,
    content_block_size: f32,
) -> f32 {
    layout_subject_with_content_size(
        Style {
            display,
            size: Size { width: Dimension::length(100.0), height: preferred_block_size },
            min_size: Size { width: Dimension::auto(), height: min_block_size },
            max_size: Size { width: Dimension::auto(), height: max_block_size },
            aspect_ratio: Some(ratio),
            flex_shrink: 0.0,
            ..Default::default()
        },
        WritingMode::HorizontalTb,
        Size { width: 100.0, height: content_block_size },
    )
    .height
}

fn layout_content_sized_box(
    display: Display,
    preferred_block_size: Dimension,
    min_block_size: Dimension,
    max_block_size: Dimension,
) -> f32 {
    layout_subject(
        Style {
            display,
            size: Size { width: Dimension::length(100.0), height: preferred_block_size },
            min_size: Size { width: Dimension::auto(), height: min_block_size },
            max_size: Size { width: Dimension::auto(), height: max_block_size },
            flex_shrink: 0.0,
            ..Default::default()
        },
        WritingMode::HorizontalTb,
    )
    .height
}

#[test]
fn intrinsic_block_sizes_resolve_from_content_across_formatting_contexts() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for preferred in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
            assert_eq!(
                layout_content_sized_box(display, preferred, Dimension::auto(), Dimension::auto()),
                100.0,
                "{display:?} preferred {preferred:?}",
            );
        }

        assert_eq!(
            layout_content_sized_box(display, Dimension::length(0.0), Dimension::max_content(), Dimension::auto(),),
            100.0,
            "{display:?} min-block-size max-content",
        );
        assert_eq!(
            layout_content_sized_box(display, Dimension::length(200.0), Dimension::auto(), Dimension::min_content(),),
            100.0,
            "{display:?} max-block-size min-content",
        );
    }
}

#[test]
fn aspect_ratio_resolves_explicit_intrinsic_block_constraints() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        assert_eq!(
            layout_ratio_subject(display, Dimension::min_content(), Dimension::auto(), Dimension::auto(), 4.0, 10.0,),
            25.0,
            "{display:?} preferred block size",
        );
        assert_eq!(
            layout_ratio_subject(
                display,
                Dimension::length(0.0),
                Dimension::min_content(),
                Dimension::auto(),
                4.0,
                50.0,
            ),
            25.0,
            "{display:?} minimum block size",
        );
        assert_eq!(
            layout_ratio_subject(
                display,
                Dimension::length(100.0),
                Dimension::auto(),
                Dimension::max_content(),
                4.0,
                50.0,
            ),
            25.0,
            "{display:?} maximum block size",
        );
    }
}

#[test]
fn replaced_intrinsic_block_constraints_determine_inline_parent_contributions() {
    for parent_display in [Display::Block, Display::Flex, Display::Grid] {
        for (preferred_block_size, min_block_size, max_block_size) in [
            (Dimension::length(0.0), Dimension::max_content(), Dimension::auto()),
            (Dimension::length(100.0), Dimension::auto(), Dimension::max_content()),
        ] {
            let mut tree = new_test_tree();
            tree.disable_rounding();
            let replaced = tree
                .new_leaf_with_context(
                    Style {
                        display: Display::Block,
                        item_is_replaced: true,
                        size: Size { width: Dimension::max_content(), height: preferred_block_size },
                        min_size: Size { width: Dimension::auto(), height: min_block_size },
                        max_size: Size { width: Dimension::auto(), height: max_block_size },
                        aspect_ratio: Some(1.0),
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                    TestNodeContext::fixed(50.0, 50.0),
                )
                .unwrap();
            let parent = tree
                .new_with_children(
                    Style {
                        display: parent_display,
                        size: Size { width: Dimension::max_content(), height: Dimension::auto() },
                        align_items: Some(AlignItems::START),
                        justify_items: Some(AlignItems::START),
                        ..Default::default()
                    },
                    &[replaced],
                )
                .unwrap();

            tree.compute_layout_with_measure(parent, Size::MAX_CONTENT, test_measure_function).unwrap();

            assert_eq!(
                tree.layout(parent).unwrap().size.width,
                50.0,
                "parent={parent_display:?}, preferred-block={preferred_block_size:?}",
            );
            assert_eq!(
                tree.layout(replaced).unwrap().size.width,
                50.0,
                "child of {parent_display:?}, preferred-block={preferred_block_size:?}",
            );
        }
    }
}

#[test]
fn automatic_minimum_floors_an_intrinsic_ratio_block_size_by_content() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for preferred in [Dimension::auto(), Dimension::min_content()] {
            assert_eq!(
                layout_ratio_subject(display, preferred, Dimension::auto(), Dimension::auto(), 10.0, 25.0,),
                25.0,
                "{display:?} preferred={preferred:?}",
            );
        }
    }
}

#[test]
fn ratio_resolved_block_size_keeps_collapsed_child_margins_out_of_its_intrinsic_size() {
    let mut tree = new_test_tree();
    tree.disable_rounding();

    let first = tree
        .new_leaf(Style {
            size: Size::from_lengths(100.0, 25.0),
            margin: Rect { bottom: length(-200.0), ..Rect::zero() },
            ..Default::default()
        })
        .unwrap();
    let empty = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(100.0), height: auto() },
            margin: Rect { top: length(50.0), bottom: length(200.0), ..Rect::zero() },
            ..Default::default()
        })
        .unwrap();
    let ratio = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                aspect_ratio: Some(2.0),
                ..Default::default()
            },
            &[empty],
        )
        .unwrap();
    let last = tree.new_leaf(Style { size: Size::from_lengths(100.0, 25.0), ..Default::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[first, ratio, last],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    let geometry = [root, first, ratio, empty, last].map(|node| {
        let layout = tree.layout(node).unwrap();
        [layout.location.x, layout.location.y, layout.size.width, layout.size.height]
    });
    assert_eq!(
        geometry,
        [
            [0.0, 0.0, 100.0, 100.0],
            [0.0, 0.0, 100.0, 25.0],
            [0.0, 25.0, 100.0, 50.0],
            [0.0, 0.0, 100.0, 0.0],
            [0.0, 75.0, 100.0, 25.0],
        ]
    );
}

#[test]
fn intrinsic_preferred_block_sizes_behave_as_auto_for_end_margin_collapse() {
    for preferred in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
        let mut tree = new_test_tree();
        tree.disable_rounding();

        let content = tree
            .new_leaf(Style {
                display: Display::Block,
                size: Size::from_lengths(100.0, 100.0),
                margin: Rect { bottom: length(100.0), ..Rect::zero() },
                ..Default::default()
            })
            .unwrap();
        let subject = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: length(100.0), height: preferred },
                    ..Default::default()
                },
                &[content],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: length(100.0), height: auto() },
                    ..Default::default()
                },
                &[subject],
            )
            .unwrap();

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(subject).unwrap().size.height, 100.0, "preferred={preferred:?}");
        assert_eq!(tree.layout(content).unwrap().size.height, 100.0, "preferred={preferred:?}");
    }
}

#[test]
fn zero_intrinsic_preferred_block_sizes_allow_margin_collapse_through() {
    for preferred in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
        let mut tree = new_test_tree();
        tree.disable_rounding();

        let first = tree
            .new_leaf(Style {
                display: Display::Block,
                size: Size::from_lengths(50.0, 10.0),
                margin: Rect { bottom: length(10.0), ..Rect::zero() },
                ..Default::default()
            })
            .unwrap();
        let subject = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: length(50.0), height: preferred },
                    margin: Rect { top: length(10.0), bottom: length(10.0), ..Rect::zero() },
                    ..Default::default()
                },
                &[],
            )
            .unwrap();
        let last = tree
            .new_leaf(Style {
                display: Display::Block,
                size: Size::from_lengths(50.0, 10.0),
                margin: Rect { top: length(10.0), ..Rect::zero() },
                ..Default::default()
            })
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: length(50.0), height: auto() },
                    ..Default::default()
                },
                &[first, subject, last],
            )
            .unwrap();

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        let geometry = [root, first, subject, last].map(|node| {
            let layout = tree.layout(node).unwrap();
            [layout.location.x, layout.location.y, layout.size.width, layout.size.height]
        });
        assert_eq!(
            geometry,
            [[0.0, 0.0, 50.0, 30.0], [0.0, 0.0, 50.0, 10.0], [0.0, 20.0, 50.0, 0.0], [0.0, 20.0, 50.0, 10.0],],
            "preferred={preferred:?}",
        );
    }
}

#[test]
fn automatic_minimum_is_capped_by_the_authored_maximum() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for preferred in [Dimension::auto(), Dimension::min_content()] {
            assert_eq!(
                layout_ratio_subject(display, preferred, Dimension::auto(), Dimension::length(15.0), 10.0, 25.0,),
                15.0,
                "{display:?} preferred={preferred:?}",
            );
        }
    }
}

#[test]
fn transferred_maximum_does_not_cap_the_automatic_minimum() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let horizontal = layout_subject_with_content_size(
            Style {
                display,
                size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
                max_size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                aspect_ratio: Some(2.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::HorizontalTb,
            Size { width: 100.0, height: 100.0 },
        );
        assert_eq!(horizontal, Size { width: 100.0, height: 100.0 }, "{display:?} horizontal");

        let vertical = layout_subject_with_content_size(
            Style {
                display,
                size: Size { width: Dimension::auto(), height: Dimension::length(200.0) },
                max_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                aspect_ratio: Some(0.5),
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::VerticalRl,
            Size { width: 100.0, height: 100.0 },
        );
        assert_eq!(vertical, Size { width: 100.0, height: 100.0 }, "{display:?} vertical");
    }
}

#[test]
fn inline_transferred_maximum_does_not_cap_the_automatic_minimum() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let size = layout_subject_with_content_size(
            Style {
                display,
                size: Size { width: Dimension::auto(), height: Dimension::length(200.0) },
                max_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                aspect_ratio: Some(0.5),
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::HorizontalTb,
            Size { width: 100.0, height: 100.0 },
        );
        assert_eq!(size, Size { width: 100.0, height: 100.0 }, "{display:?}");
    }
}

#[test]
fn automatic_minimum_order_is_shared_by_parent_formatting_contexts() {
    for parent_display in [Display::Block, Display::Flex, Display::Grid] {
        let inline = layout_subject_in_formatting_context(
            parent_display,
            Style {
                display: Display::Block,
                size: Size { width: Dimension::auto(), height: Dimension::length(200.0) },
                max_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                aspect_ratio: Some(0.5),
                ..Default::default()
            },
            WritingMode::HorizontalTb,
            Size { width: 100.0, height: 100.0 },
        );
        assert_eq!(inline, Size { width: 100.0, height: 100.0 }, "{parent_display:?} inline");

        let block = layout_subject_in_formatting_context(
            parent_display,
            Style {
                display: Display::Block,
                size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
                max_size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                aspect_ratio: Some(2.0),
                ..Default::default()
            },
            WritingMode::HorizontalTb,
            Size { width: 100.0, height: 100.0 },
        );
        assert_eq!(block, Size { width: 100.0, height: 100.0 }, "{parent_display:?} block");
    }
}

#[test]
fn explicit_intrinsic_minimum_does_not_enable_the_automatic_minimum() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        assert_eq!(
            layout_ratio_subject(display, Dimension::auto(), Dimension::min_content(), Dimension::auto(), 4.0, 50.0,),
            25.0,
            "{display:?}",
        );
    }
}

#[test]
fn scroll_containers_do_not_apply_the_aspect_ratio_automatic_minimum() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for preferred in [Dimension::auto(), Dimension::min_content()] {
            let size = layout_subject_with_content_size(
                Style {
                    display,
                    size: Size { width: Dimension::length(100.0), height: preferred },
                    aspect_ratio: Some(10.0),
                    overflow: Point { x: Overflow::Scroll, y: Overflow::Scroll },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                WritingMode::HorizontalTb,
                Size { width: 100.0, height: 25.0 },
            );
            assert_eq!(size.height, 10.0, "{display:?} preferred={preferred:?}");
        }
    }
}

#[test]
fn intrinsic_ratio_block_size_uses_the_selected_sizing_box() {
    for (box_sizing, expected) in [(BoxSizing::ContentBox, 70.0), (BoxSizing::BorderBox, 50.0)] {
        let size = layout_subject_with_content_size(
            Style {
                display: Display::Block,
                box_sizing,
                size: Size { width: Dimension::length(100.0), height: Dimension::max_content() },
                aspect_ratio: Some(2.0),
                padding: Rect::length(10.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::HorizontalTb,
            Size::ZERO,
        );
        assert_eq!(size.height, expected, "{box_sizing:?}");
    }
}

#[test]
fn intrinsic_ratio_block_size_follows_vertical_writing_modes() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let size = layout_subject_with_content_size(
            Style {
                display,
                size: Size { width: Dimension::max_content(), height: Dimension::length(100.0) },
                aspect_ratio: Some(2.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::VerticalRl,
            Size { width: 10.0, height: 100.0 },
        );
        assert_eq!(size.width, 200.0, "{display:?}");
    }
}

#[test]
fn grid_explicit_block_stretch_precedes_preferred_ratio_transfer() {
    fn layout_item(align_self: Option<AlignSelf>) -> Size<f32> {
        let mut tree = new_test_tree();
        let content = tree.new_leaf(Style { size: Size::from_lengths(100.0, 25.0), ..Default::default() }).unwrap();
        let item = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                    aspect_ratio: Some(10.0),
                    align_self,
                    ..Default::default()
                },
                &[content],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Grid,
                    size: Size::from_lengths(100.0, 100.0),
                    grid_template_columns: vec![length(100.0)],
                    grid_template_rows: vec![length(100.0)],
                    ..Default::default()
                },
                &[item],
            )
            .unwrap();

        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(100.0) },
            test_measure_function,
        )
        .unwrap();
        tree.layout(item).unwrap().size
    }

    assert_eq!(layout_item(None).height, 25.0, "normal alignment remains content-sized");
    assert_eq!(layout_item(Some(AlignSelf::STRETCH)).height, 100.0, "explicit stretch wins before ratio transfer",);
}

#[test]
fn intrinsic_block_constraints_follow_a_column_flex_main_axis() {
    for (preferred, min, max) in [
        (Dimension::max_content(), Dimension::auto(), Dimension::auto()),
        (Dimension::length(0.0), Dimension::max_content(), Dimension::auto()),
        (Dimension::length(200.0), Dimension::auto(), Dimension::min_content()),
    ] {
        let size = layout_subject(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: Dimension::length(100.0), height: preferred },
                min_size: Size { width: Dimension::auto(), height: min },
                max_size: Size { width: Dimension::auto(), height: max },
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::HorizontalTb,
        );
        assert_eq!(size.height, 100.0, "preferred={preferred:?} min={min:?} max={max:?}");
    }
}

#[test]
fn intrinsic_block_constraints_follow_vertical_writing_modes() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let minimum = layout_subject(
            Style {
                display,
                size: Size::from_lengths(0.0, 100.0),
                min_size: Size { width: Dimension::max_content(), height: Dimension::auto() },
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::VerticalLr,
        );
        assert_eq!(minimum.width, 100.0, "{display:?} min-block-size");

        let maximum = layout_subject(
            Style {
                display,
                size: Size::from_lengths(200.0, 100.0),
                max_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::VerticalRl,
        );
        assert_eq!(maximum.width, 100.0, "{display:?} max-block-size");
    }
}

#[test]
fn intrinsic_block_constraints_include_the_selected_box_edges() {
    let mut tree = new_test_tree();
    let content = tree.new_leaf(Style { size: Size::from_lengths(50.0, 100.0), ..Default::default() }).unwrap();
    let subject = tree
        .new_with_children(
            Style {
                display: Display::Block,
                box_sizing: BoxSizing::ContentBox,
                size: Size::from_lengths(50.0, 0.0),
                min_size: Size { width: Dimension::auto(), height: Dimension::max_content() },
                padding: Rect::length(10.0),
                ..Default::default()
            },
            &[content],
        )
        .unwrap();

    tree.compute_layout_with_measure(subject, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(subject).unwrap().size.height, 120.0);
}

fn layout_absolute_content_sized(
    display: Display,
    preferred_block_size: Dimension,
    aspect_ratio: Option<f32>,
    min_block_size: Dimension,
    block_end_inset_is_auto: bool,
) -> Layout {
    let mut tree = new_test_tree();
    let content = tree.new_leaf(Style { size: Size::from_lengths(20.0, 90.0), ..Default::default() }).unwrap();
    let subject = tree
        .new_with_children(
            Style {
                display: Display::Block,
                position: Position::Absolute,
                size: Size { width: Dimension::length(20.0), height: preferred_block_size },
                min_size: Size { width: Dimension::auto(), height: min_block_size },
                aspect_ratio,
                inset: Rect {
                    left: length(0.0),
                    right: auto(),
                    top: percent(0.5),
                    bottom: if block_end_inset_is_auto { auto() } else { length(0.0) },
                },
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let root = tree
        .new_with_children(Style { display, size: Size::from_lengths(100.0, 100.0), ..Default::default() }, &[subject])
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(100.0) },
        test_measure_function,
    )
    .unwrap();
    *tree.layout(subject).unwrap()
}

#[test]
fn absolute_fit_content_block_size_is_content_sized_not_inset_stretched() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let layout = layout_absolute_content_sized(display, Dimension::fit_content(), None, Dimension::auto(), false);
        assert_eq!(layout.size.height, 90.0, "{display:?}");
        assert_eq!(layout.location.y, 50.0, "{display:?}");
    }
}

#[test]
fn absolute_intrinsic_block_size_uses_ratio_and_automatic_minimum_semantics() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let automatic_minimum =
            layout_absolute_content_sized(display, Dimension::fit_content(), Some(2.0), Dimension::auto(), false);
        assert_eq!(automatic_minimum.size.height, 90.0, "{display:?} automatic minimum");

        let explicit_minimum =
            layout_absolute_content_sized(display, Dimension::fit_content(), Some(2.0), Dimension::length(0.0), false);
        assert_eq!(explicit_minimum.size.height, 10.0, "{display:?} explicit minimum");

        let automatic_size =
            layout_absolute_content_sized(display, Dimension::auto(), Some(2.0), Dimension::auto(), true);
        assert_eq!(automatic_size.size.height, 90.0, "{display:?} automatic size");
    }
}

#[test]
fn grid_item_intrinsic_block_size_collapses_internal_empty_block_margins() {
    let mut tree = new_test_tree();
    let margin = Rect { top: length(50.0), bottom: length(50.0), ..Rect::zero() };
    let inner = tree.new_leaf(Style { display: Display::Block, margin, ..Default::default() }).unwrap();
    let outer = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                margin,
                ..Default::default()
            },
            &[inner],
        )
        .unwrap();
    let cell = tree.new_with_children(Style { display: Display::Block, ..Default::default() }, &[outer]).unwrap();
    let grid = tree
        .new_with_children(
            Style { display: Display::Grid, size: Size { width: length(100.0), height: auto() }, ..Default::default() },
            &[cell],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(cell).unwrap().size.height, 50.0);
}
