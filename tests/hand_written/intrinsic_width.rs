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
    tree.compute_layout_with_measure_output(
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
                max_size: Size { width: Dimension::length(200.0).into(), height: Dimension::auto().into() },
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
                min_size: Size { width: Dimension::max_content().into(), height: Dimension::auto().into() },
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
                max_size: Size { width: Dimension::min_content().into(), height: Dimension::auto().into() },
                ..Default::default()
            },
        ),
        160.0,
    );
}

#[test]
fn auto_root_width_obeys_an_intrinsic_maximum_without_a_parent_supplied_width() {
    let mut tree = new_test_tree();
    let node = tree
        .new_leaf_with_context(
            Style {
                max_size: Size { width: Dimension::min_content().into(), height: Dimension::auto().into() },
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    tree.compute_layout_with_measure_output(node, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(node).unwrap().size.width, 160.0);
}

#[test]
fn auto_block_root_retains_stretching_before_intrinsic_bounds_are_applied() {
    for (minimum, maximum, expected) in [
        (Dimension::min_content(), Dimension::auto(), 300.0),
        (Dimension::max_content(), Dimension::auto(), 300.0),
        (Dimension::auto(), Dimension::stretch(), 300.0),
        (Dimension::auto(), Dimension::min_content(), 160.0),
        (Dimension::auto(), Dimension::max_content(), 240.0),
        (Dimension::max_content(), Dimension::min_content(), 240.0),
    ] {
        let mut tree = new_test_tree();
        let node = tree
            .new_leaf_with_context(
                Style {
                    display: Display::Block,
                    min_size: Size { width: minimum.into(), height: auto() },
                    max_size: Size { width: maximum.into(), height: auto() },
                    ..Default::default()
                },
                text_context(),
            )
            .unwrap();
        tree.compute_layout_with_measure_output(
            node,
            Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::MaxContent },
            test_measure_function,
        )
        .unwrap();
        assert_eq!(tree.layout(node).unwrap().size.width, expected, "{minimum:?} / {maximum:?}");
    }
}

#[test]
fn auto_block_width_obeys_an_intrinsic_maximum() {
    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                max_size: Size { width: Dimension::min_content().into(), height: Dimension::auto().into() },
                ..Default::default()
            },
        ),
        160.0,
    );
}

#[test]
fn auto_block_width_with_an_intrinsic_minimum_can_still_fill_available_space() {
    for minimum in [Dimension::min_content(), Dimension::max_content()] {
        assert_eq!(
            layout_child(
                Display::Block,
                Style {
                    min_size: Size { width: minimum.into(), height: Dimension::auto().into() },
                    ..Default::default()
                },
            ),
            300.0,
        );
    }
}

#[test]
fn fit_content_and_stretch_apply_as_min_and_max_constraints() {
    for (constraint, expected) in [(Dimension::fit_content(), 240.0), (Dimension::stretch(), 300.0)] {
        assert_eq!(
            layout_child(
                Display::Block,
                Style {
                    size: Size::from_lengths(100.0, 20.0),
                    min_size: Size { width: constraint.into(), height: Dimension::auto().into() },
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
                    max_size: Size { width: constraint.into(), height: Dimension::auto().into() },
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
    tree.compute_layout_with_measure_output(
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
    tree.compute_layout_with_measure_output(
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
        tree.compute_layout_with_measure_output(
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
    tree.compute_layout_with_measure_output(grid, Size::MAX_CONTENT, test_measure_function).unwrap();
    (tree.layout(grid).unwrap().size.width, tree.layout(item).unwrap().size.width)
}

#[test]
fn intrinsic_grid_tracks_include_explicit_intrinsic_item_widths() {
    assert_eq!(intrinsic_grid_track_width(Dimension::min_content()), (160.0, 160.0),);
    assert_eq!(intrinsic_grid_track_width(Dimension::max_content()), (240.0, 240.0),);
}

#[test]
fn auto_grid_tracks_include_intrinsic_item_minimum_widths() {
    for (minimum, expected) in [(Dimension::min_content(), 160.0), (Dimension::max_content(), 240.0)] {
        let (item, next_column) = grid_track_with_intrinsic_minimum(Dimension::auto(), minimum);
        assert_eq!(item, expected);
        assert_eq!(next_column, expected, "the auto track must include the authored minimum");
    }
}

#[test]
fn intrinsic_grid_item_minimum_is_applied_before_measuring_its_row_height() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: length(0.0), height: auto() },
                min_size: Size { width: Dimension::max_content().into(), height: auto() },
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    let next = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(200.0), height: auto() },
                grid_template_columns: vec![auto(), length(100.0)],
                ..Default::default()
            },
            &[item, next],
        )
        .unwrap();
    tree.compute_layout_with_measure_output(grid, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 240.0, height: 10.0 });
    assert_eq!(tree.layout(next).unwrap().location.x, 240.0);
    assert_eq!(tree.layout(grid).unwrap().size.height, 10.0);
}

#[test]
fn auto_grid_tracks_do_not_shrink_intrinsic_minimums_to_a_smaller_preferred_width() {
    for (minimum, expected) in [(Dimension::min_content(), 160.0), (Dimension::max_content(), 240.0)] {
        let (item, next_column) = grid_track_with_intrinsic_minimum(Dimension::length(0.0), minimum);
        assert_eq!(item, expected);
        assert_eq!(next_column, expected, "the minimum must also constrain the track contribution");
    }
}

fn grid_track_with_intrinsic_minimum(width: Dimension, min_width: Dimension) -> (f32, f32) {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width, height: Dimension::length(20.0) },
                min_size: Size { width: min_width.into(), height: Dimension::auto().into() },
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    let next = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
                grid_template_columns: vec![auto(), length(100.0)],
                ..Default::default()
            },
            &[item, next],
        )
        .unwrap();
    tree.compute_layout_with_measure_output(grid, Size::MAX_CONTENT, test_measure_function).unwrap();
    (tree.layout(item).unwrap().size.width, tree.layout(next).unwrap().location.x)
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
    tree.compute_layout_with_measure_output(
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
                min_size: Size { width: Dimension::length(0.0).into(), height: Dimension::auto().into() },
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

#[test]
fn content_flex_basis_includes_percentage_padding_from_the_containing_block() {
    for container_width in [240.0, 300.0, 400.0] {
        for percentage in [0.0, 0.05, 0.1, 0.2] {
            for box_sizing in [BoxSizing::ContentBox, BoxSizing::BorderBox] {
                let mut tree = new_test_tree();
                let item = tree
                    .new_leaf_with_context(
                        Style {
                            box_sizing,
                            padding: Rect {
                                left: percent(percentage),
                                right: percent(percentage),
                                top: length(0.0),
                                bottom: length(0.0),
                            },
                            ..Default::default()
                        },
                        TestNodeContext::ahem_text("aaaa\u{200b}bbbb\u{200b}cccc".into(), WritingMode::Horizontal),
                    )
                    .unwrap();
                let flex = tree
                    .new_with_children(
                        Style { size: Size { width: length(container_width), height: auto() }, ..Default::default() },
                        &[item],
                    )
                    .unwrap();
                tree.compute_layout_with_measure_output(flex, Size::MAX_CONTENT, test_measure_function).unwrap();
                let size = tree.layout(item).unwrap().size;
                assert_eq!(size.width, 120.0 + 2.0 * percentage * container_width);
                assert_eq!(size.height, 10.0);
            }
        }
    }
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
    tree.compute_layout_with_measure_output(
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

#[test]
fn block_stretch_bounds_consume_margins_once() {
    for minimum in [false, true] {
        let mut style = Style {
            size: Size { width: length(if minimum { 100.0 } else { 400.0 }), height: auto() },
            margin: Rect { left: length(10.0), right: length(15.0), top: auto(), bottom: auto() },
            ..Default::default()
        };
        if minimum {
            style.min_size.width = Dimension::stretch().into();
        } else {
            style.max_size.width = Dimension::stretch().into();
        }
        assert_eq!(layout_child(Display::Block, style), 275.0, "minimum={minimum}");
    }
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

/// Auto row contributions must use the wrapped height after a definite
/// percentage maximum clamps a max-content preferred inline size.
#[test]
fn grid_max_content_with_percentage_maximum_contributes_wrapped_height() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::max_content(), height: auto() },
                max_size: Size { width: percent(1.0), height: auto() },
                justify_self: Some(AlignSelf::CENTER),
                ..Default::default()
            },
            text_context(),
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(200.0), height: auto() },
                grid_template_columns: vec![minmax(length(0.0), fr(1.0))],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();
    tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 200.0, height: 20.0 });
    assert_eq!(tree.layout(root).unwrap().size.height, 20.0);
}

#[test]
fn intrinsic_child_bounds_constrain_parent_content_width() {
    for (width, min_width, max_width, parent_width) in [
        (
            length(600.0),
            taffy::LengthPercentageAuto::auto(),
            taffy::LengthPercentageAuto::min_content(),
            Dimension::min_content(),
        ),
        (
            length(10.0),
            taffy::LengthPercentageAuto::fit_content(),
            taffy::LengthPercentageAuto::auto(),
            Dimension::fit_content(),
        ),
    ] {
        let mut tree = new_test_tree();
        let child = tree
            .new_leaf_with_context(
                Style {
                    display: Display::Block,
                    size: Size { width, height: auto() },
                    min_size: Size { width: min_width, height: auto() },
                    max_size: Size { width: max_width, height: auto() },
                    ..Style::default()
                },
                TestNodeContext::ahem_text("XXXXXXXXXX".to_owned(), WritingMode::Horizontal),
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: parent_width, height: auto() },
                    ..Style::default()
                },
                &[child],
            )
            .unwrap();
        tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
        assert_eq!(tree.layout(root).unwrap().size.width, 100.0);
        assert_eq!(tree.layout(child).unwrap().size.width, 100.0);
    }
}

#[test]
fn orthogonal_flex_item_intrinsic_maximum_caps_its_flex_basis() {
    let mut tree = new_test_tree();
    let child = tree
        .new_leaf_with_context(
            Style {
                display: Display::Block,
                flex_basis: length(200.0),
                max_size: Size { width: taffy::LengthPercentageAuto::min_content(), height: auto() },
                ..Style::default()
            },
            TestNodeContext::ahem_text("XXXXXXXXXX".to_owned(), WritingMode::Horizontal),
        )
        .unwrap();
    tree.set_writing_mode(child, taffy::WritingMode::VerticalRl).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(200.0), height: length(100.0) },
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(child).unwrap().size.width, 100.0);
}

#[test]
fn wrapped_column_stretch_cross_size_uses_definite_container_width_during_measurement() {
    for wider_sibling in [false, true] {
        let mut tree = new_test_tree();
        let item_style = Style {
            size: Size { width: Dimension::stretch(), height: length(75.0) },
            border: Rect::length(3.0),
            box_sizing: BoxSizing::ContentBox,
            ..Style::default()
        };
        let first = tree.new_leaf_with_context(item_style.clone(), TestNodeContext::fixed(20.0, 20.0)).unwrap();
        let second = tree.new_leaf_with_context(item_style, TestNodeContext::fixed(20.0, 20.0)).unwrap();
        let mut items = vec![first];
        if wider_sibling {
            items.push(
                tree.new_leaf(Style {
                    size: Size { width: length(150.0), height: auto() },
                    border: Rect::length(3.0),
                    box_sizing: BoxSizing::ContentBox,
                    ..Style::default()
                })
                .unwrap(),
            );
        }
        items.push(second);
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    flex_wrap: FlexWrap::Wrap,
                    align_content: AlignContent::START,
                    size: Size { width: length(100.0), height: length(100.0) },
                    ..Style::default()
                },
                &items,
            )
            .unwrap();
        tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
        assert_eq!(tree.layout(first).unwrap().size.width, if wider_sibling { 156.0 } else { 100.0 });
        assert_eq!(tree.layout(second).unwrap().size.width, 100.0);
        assert_eq!(tree.layout(second).unwrap().location.x, if wider_sibling { 156.0 } else { 100.0 });
    }
}

#[test]
fn zero_flex_grow_does_not_add_a_positive_intrinsic_flex_fraction() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            size: Size { width: length(200.0), height: auto() },
            flex_basis: length(0.0),
            flex_grow: 0.0,
            ..Style::default()
        })
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size { width: auto(), height: length(100.0) }, ..Style::default() },
            &[child],
        )
        .unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size.width, 0.0);
}

#[test]
fn definite_main_stretch_caps_the_automatic_minimum() {
    let mut tree = new_test_tree();
    let child = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::stretch(), height: auto() },
                margin: Rect::length(5.0),
                padding: Rect::length(2.0),
                border: Rect::length(3.0),
                ..Style::default()
            },
            TestNodeContext::fixed(100.0, 20.0),
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(100.0), height: length(100.0) },
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    tree.compute_layout_with_measure_output(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(child).unwrap().size.width, 90.0);
}

#[test]
fn orthogonal_flex_basis_keywords_measure_the_item_block_axis() {
    for basis in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
        let mut tree = TaffyTree::<()>::new();
        let floats: Vec<_> = (0..2)
            .map(|_| {
                tree.new_leaf(Style { float: Float::Left, size: Size::from_lengths(50.0, 50.0), ..Style::default() })
                    .unwrap()
            })
            .collect();
        let child = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    flex_basis: basis,
                    flex_shrink: 0.0,
                    min_size: Size { width: length(0.0), height: length(0.0) },
                    ..Style::default()
                },
                &floats,
            )
            .unwrap();
        tree.set_writing_mode(child, taffy::WritingMode::VerticalRl).unwrap();
        let root = tree
            .new_with_children(
                Style { display: Display::Flex, size: Size::from_lengths(75.0, 75.0), ..Style::default() },
                &[child],
            )
            .unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(child).unwrap().size.width, 100.0, "{basis:?}");
    }
}
