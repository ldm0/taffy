use taffy::prelude::*;
use taffy::style::Float;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

fn text_context() -> TestNodeContext {
    TestNodeContext::ahem_text("aaaaaaaaaaaaaaaa\u{200b}bbbbbbbb".to_owned(), WritingMode::Horizontal)
}

fn layout_child(container_display: Display, mut child_style: Style) -> f32 {
    let mut tree = new_test_tree();
    child_style.display = Display::Block;
    child_style.flex_grow = 0.0;
    child_style.flex_shrink = 0.0;
    let child = tree.new_leaf_with_context(child_style, text_context()).unwrap();
    let root = tree
        .new_with_children(
            Style { display: container_display, size: Size::from_lengths(300.0, 70.0), ..Default::default() },
            &[child],
        )
        .unwrap();
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(70.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(child).unwrap().size.width
}

#[test]
fn intrinsic_width_keywords_apply_in_block_flex_and_grid() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        assert_eq!(
            layout_child(
                display,
                Style {
                    size: Size { width: Dimension::min_content(), height: Dimension::length(20.0) },
                    ..Default::default()
                },
            ),
            160.0,
            "{display:?} min-content",
        );
        assert_eq!(
            layout_child(
                display,
                Style {
                    size: Size { width: Dimension::max_content(), height: Dimension::length(20.0) },
                    ..Default::default()
                },
            ),
            240.0,
            "{display:?} max-content",
        );
    }
}

#[test]
fn fit_content_clamps_to_available_width() {
    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                size: Size { width: Dimension::fit_content(), height: Dimension::length(20.0) },
                max_size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
                ..Default::default()
            },
        ),
        200.0,
    );
}

#[test]
fn intrinsic_min_and_max_clamp_preferred_width() {
    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                size: Size::from_lengths(100.0, 20.0),
                min_size: Size { width: Dimension::max_content(), height: Dimension::auto() },
                ..Default::default()
            },
        ),
        240.0,
    );
    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                size: Size::from_lengths(300.0, 20.0),
                max_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                ..Default::default()
            },
        ),
        160.0,
    );
}

#[test]
fn fit_content_and_stretch_apply_as_min_and_max_constraints() {
    for (constraint, expected) in [(Dimension::fit_content(), 240.0), (Dimension::stretch(), 300.0)] {
        assert_eq!(
            layout_child(
                Display::Block,
                Style {
                    size: Size::from_lengths(100.0, 20.0),
                    min_size: Size { width: constraint, height: Dimension::auto() },
                    ..Default::default()
                },
            ),
            expected,
            "min-width {constraint:?}",
        );
        assert_eq!(
            layout_child(
                Display::Block,
                Style {
                    size: Size::from_lengths(400.0, 20.0),
                    max_size: Size { width: constraint, height: Dimension::auto() },
                    ..Default::default()
                },
            ),
            expected,
            "max-width {constraint:?}",
        );
    }
}

fn layout_absolute_child(container_display: Display, container_width: f32, width: Dimension) -> Layout {
    let mut tree = new_test_tree();
    let child = tree
        .new_leaf_with_context(
            Style {
                display: Display::Block,
                position: Position::Absolute,
                size: Size { width, height: Dimension::length(20.0) },
                inset: Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: auto() },
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: container_display, size: Size::from_lengths(container_width, 70.0), ..Default::default() },
            &[child],
        )
        .unwrap();
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(container_width), height: AvailableSpace::Definite(70.0) },
        test_measure_function,
    )
    .unwrap();
    *tree.layout(child).unwrap()
}

#[test]
fn intrinsic_width_prevents_absolute_inset_stretch() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let layout = layout_absolute_child(display, 300.0, Dimension::min_content());
        assert_eq!(layout.size.width, 160.0, "{display:?} min-content");
        assert_eq!(layout.location.x, 0.0, "{display:?} inline start");
    }
}

#[test]
fn absolute_fit_content_uses_inset_constrained_available_width() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let layout = layout_absolute_child(display, 200.0, Dimension::fit_content());
        assert_eq!(layout.size.width, 200.0, "{display:?} fit-content");
    }
}

fn layout_root_width(width: Dimension, available_width: f32) -> f32 {
    let mut tree = new_test_tree();
    let root = tree
        .new_leaf_with_context(
            Style {
                display: Display::Block,
                size: Size { width, height: Dimension::length(20.0) },
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(available_width), height: AvailableSpace::Definite(70.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(root).unwrap().size.width
}

#[test]
fn block_root_only_stretches_an_auto_preferred_width() {
    assert_eq!(layout_root_width(Dimension::min_content(), 300.0), 160.0);
    assert_eq!(layout_root_width(Dimension::max_content(), 300.0), 240.0);
    assert_eq!(layout_root_width(Dimension::fit_content(), 200.0), 200.0);
    assert_eq!(layout_root_width(Dimension::stretch(), 300.0), 300.0);
    assert_eq!(layout_root_width(Dimension::auto(), 300.0), 300.0);
}

#[test]
fn intrinsic_root_inline_size_respects_numeric_limits() {
    use taffy::{LogicalSize, WritingMode as LayoutWritingMode};
    for mode in [LayoutWritingMode::HorizontalTb, LayoutWritingMode::VerticalRl, LayoutWritingMode::VerticalLr] {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            for width in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
                for (min, max, expected) in [
                    (auto(), length(100.0), 100.0),
                    (length(300.0), auto(), 300.0),
                    (length(120.0), length(100.0), 120.0),
                    (auto(), percent(0.25), 100.0),
                ] {
                    for box_sizing in [BoxSizing::ContentBox, BoxSizing::BorderBox] {
                        let mut tree = new_test_tree();
                        let item = tree
                            .new_leaf(Style {
                                size: mode
                                    .to_physical(LogicalSize { inline_size: length(250.0), block_size: length(20.0) }),
                                ..Default::default()
                            })
                            .unwrap();
                        let root = tree
                            .new_with_children(
                                Style {
                                    display,
                                    box_sizing,
                                    size: mode.to_physical(LogicalSize { inline_size: width, block_size: auto() }),
                                    min_size: mode.to_physical(LogicalSize { inline_size: min, block_size: auto() }),
                                    max_size: mode.to_physical(LogicalSize { inline_size: max, block_size: auto() }),
                                    padding: Rect {
                                        left: length(5.0),
                                        right: length(5.0),
                                        top: length(5.0),
                                        bottom: length(5.0),
                                    },
                                    border: Rect {
                                        left: length(2.0),
                                        right: length(2.0),
                                        top: length(2.0),
                                        bottom: length(2.0),
                                    },
                                    ..Default::default()
                                },
                                &[item],
                            )
                            .unwrap();
                        for node in [root, item] {
                            tree.set_writing_mode(node, mode).unwrap();
                        }
                        tree.compute_layout(
                            root,
                            Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::Definite(400.0) },
                        )
                        .unwrap();
                        let expected = expected + if box_sizing == BoxSizing::ContentBox { 14.0 } else { 0.0 };
                        assert_eq!(
                            mode.to_logical(tree.layout(root).unwrap().size).inline_size,
                            expected,
                            "{mode:?} {display:?} {box_sizing:?} {width:?} min={min:?} max={max:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn intrinsic_limits_apply_to_numeric_root_preferred_sizes() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for (width, min, max) in [(100.0, Dimension::max_content(), auto()), (300.0, auto(), Dimension::min_content())]
        {
            let mut tree = new_test_tree();
            let item = tree.new_leaf(Style { size: Size::from_lengths(250.0, 20.0), ..Default::default() }).unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        display,
                        size: Size::from_lengths(width, 20.0),
                        min_size: Size { width: min, height: auto() },
                        max_size: Size { width: max, height: auto() },
                        ..Default::default()
                    },
                    &[item],
                )
                .unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(root).unwrap().size.width, 250.0, "{display:?} min={min:?} max={max:?}");
        }
    }
}

#[test]
fn intrinsic_input_resolution_preserves_parent_assigned_inline_sizes() {
    use super::test_tree::{TestNode, TestTree};
    use taffy::{LayoutInput, RunMode, SizingMode, WritingMode as LayoutWritingMode};
    for mode in [LayoutWritingMode::HorizontalTb, LayoutWritingMode::VerticalRl, LayoutWritingMode::VerticalLr] {
        let node = TestNode::container(
            Display::Grid,
            Style {
                min_size: Size { width: Dimension::min_content(), height: Dimension::min_content() },
                max_size: Size::from_lengths(50.0, 50.0),
                ..Default::default()
            },
            Rect::ZERO,
        );
        let item = TestNode::leaf(Style::default(), Size { width: 200.0, height: 200.0 });
        let mut tree = TestTree::new(node, item);
        tree.nodes[0].writing_mode = mode;
        let inputs = LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::InherentSize,
            known_dimensions: Size { width: Some(100.0), height: Some(100.0) },
            ..LayoutInput::HIDDEN
        };
        let resolved = taffy::resolve_intrinsic_inline_inputs_with_provenance(&mut tree, NodeId::from(0_usize), inputs);
        assert_eq!(resolved.inputs, inputs, "{mode:?}");
        assert!(!resolved.depends_on_block_constraints);
        assert!(!resolved.applied_aspect_ratio);
    }
}

#[test]
fn definite_opposite_size_transfers_before_intrinsic_width_measurement() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let mut tree = new_test_tree();
        let text = tree.new_leaf_with_context(Style::default(), text_context()).unwrap();
        let child = tree
            .new_with_children(
                Style {
                    display,
                    size: Size { width: Dimension::min_content(), height: Dimension::length(20.0) },
                    aspect_ratio: Some(20.0),
                    ..Default::default()
                },
                &[text],
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style { display: Display::Block, size: Size::from_lengths(300.0, 70.0), ..Default::default() },
                &[child],
            )
            .unwrap();
        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(70.0) },
            test_measure_function,
        )
        .unwrap();
        assert_eq!(tree.layout(child).unwrap().size.width, 400.0, "{display:?}");
    }
}

fn intrinsic_grid_track_width(item_width: Dimension) -> (f32, f32) {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style { size: Size { width: item_width, height: Dimension::length(20.0) }, ..Default::default() },
            text_context(),
        )
        .unwrap();
    let grid = tree.new_with_children(Style { display: Display::Grid, ..Default::default() }, &[item]).unwrap();
    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();
    (tree.layout(grid).unwrap().size.width, tree.layout(item).unwrap().size.width)
}

#[test]
fn intrinsic_grid_tracks_include_explicit_intrinsic_item_widths() {
    assert_eq!(intrinsic_grid_track_width(Dimension::min_content()), (160.0, 160.0),);
    assert_eq!(intrinsic_grid_track_width(Dimension::max_content()), (240.0, 240.0),);
}

fn flexible_intrinsic_item_width(mut item_style: Style, container_width: f32) -> f32 {
    let mut tree = new_test_tree();
    item_style.display = Display::Block;
    item_style.size.height = Dimension::length(20.0);
    let item = tree.new_leaf_with_context(item_style, text_context()).unwrap();
    let flex = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size::from_lengths(container_width, 70.0), ..Default::default() },
            &[item],
        )
        .unwrap();
    tree.compute_layout_with_measure(
        flex,
        Size { width: AvailableSpace::Definite(container_width), height: AvailableSpace::Definite(70.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(item).unwrap().size.width
}

#[test]
fn intrinsic_preferred_width_participates_in_flexing_without_becoming_final() {
    assert_eq!(
        flexible_intrinsic_item_width(
            Style {
                size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                flex_grow: 1.0,
                flex_shrink: 1.0,
                ..Default::default()
            },
            300.0,
        ),
        300.0,
    );
    assert_eq!(
        flexible_intrinsic_item_width(
            Style {
                size: Size { width: Dimension::max_content(), height: Dimension::auto() },
                min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
                flex_grow: 0.0,
                flex_shrink: 1.0,
                ..Default::default()
            },
            120.0,
        ),
        120.0,
    );
}

#[test]
fn flex_basis_content_ignores_an_intrinsic_preferred_main_size() {
    assert_eq!(
        flexible_intrinsic_item_width(
            Style {
                size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                flex_basis: Dimension::content(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            300.0,
        ),
        240.0,
    );
}

fn floated_keyword_width(width: Dimension) -> f32 {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                display: Display::Block,
                float: Float::Left,
                size: Size { width, height: Dimension::length(20.0) },
                margin: Rect { left: length(10.0), right: length(15.0), top: length(0.0), bottom: length(0.0) },
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size::from_lengths(200.0, 70.0), ..Default::default() },
            &[item],
        )
        .unwrap();
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(200.0), height: AvailableSpace::Definite(70.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(item).unwrap().size.width
}

#[test]
fn floated_intrinsic_and_stretch_widths_consume_margins_once() {
    assert_eq!(floated_keyword_width(Dimension::fit_content()), 175.0);
    assert_eq!(floated_keyword_width(Dimension::stretch()), 175.0);
}

#[cfg(feature = "parse")]
#[test]
fn intrinsic_dimension_keywords_parse_without_colliding_with_grid_fit_content() {
    use core::str::FromStr;

    for (css, expected) in [
        ("min-content", Dimension::min_content()),
        ("max-content", Dimension::max_content()),
        ("fit-content", Dimension::fit_content()),
        ("stretch", Dimension::stretch()),
        ("-webkit-fill-available", Dimension::stretch()),
    ] {
        assert_eq!(Dimension::from_str(css).unwrap(), expected, "{css}");
    }
}

#[cfg(feature = "serde")]
#[test]
fn intrinsic_dimension_tags_round_trip_through_serde() {
    for value in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content(), Dimension::stretch()] {
        let serialized = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<Dimension>(&serialized).unwrap(), value);
    }
}
