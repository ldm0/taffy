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
fn parameterized_fit_content_clamps_authored_lengths_and_percentages() {
    for (limit, expected) in [(100.0, 160.0), (200.0, 200.0), (300.0, 240.0)] {
        assert_eq!(
            layout_child(
                Display::Block,
                Style {
                    size: Size {
                        width: Dimension::fit_content_function(LengthPercentage::length(limit)),
                        height: Dimension::length(20.0),
                    },
                    ..Default::default()
                },
            ),
            expected,
            "fit-content({limit}px)",
        );
    }

    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                size: Size {
                    width: Dimension::fit_content_function(LengthPercentage::percent(0.5)),
                    height: Dimension::length(20.0),
                },
                ..Default::default()
            },
        ),
        160.0,
    );
}

#[test]
fn parameterized_fit_content_applies_to_min_and_max_constraints() {
    let limit = Dimension::fit_content_function(LengthPercentage::length(200.0));
    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                size: Size::from_lengths(100.0, 20.0),
                min_size: Size { width: limit, height: Dimension::auto() },
                ..Default::default()
            },
        ),
        200.0,
    );
    assert_eq!(
        layout_child(
            Display::Block,
            Style {
                size: Size::from_lengths(300.0, 20.0),
                max_size: Size { width: limit, height: Dimension::auto() },
                ..Default::default()
            },
        ),
        200.0,
    );
}

#[test]
fn parameterized_fit_content_uses_the_selected_sizing_box() {
    for (box_sizing, expected) in [(BoxSizing::ContentBox, 220.0), (BoxSizing::BorderBox, 200.0)] {
        assert_eq!(
            layout_child(
                Display::Block,
                Style {
                    box_sizing,
                    size: Size {
                        width: Dimension::fit_content_function(LengthPercentage::length(200.0)),
                        height: Dimension::length(20.0),
                    },
                    padding: Rect::length(10.0),
                    ..Default::default()
                },
            ),
            expected,
            "{box_sizing:?}",
        );
    }
}

fn cyclic_fit_content_contribution(outer_width: Dimension, child_style: Style, child_context: TestNodeContext) -> f32 {
    let mut tree = new_test_tree();
    let child = tree.new_leaf_with_context(child_style, child_context).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: outer_width, height: Dimension::length(20.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(20.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(root).unwrap().size.width
}

#[test]
fn cyclic_fit_content_percentages_use_property_specific_intrinsic_fallbacks() {
    let fit_percent = Dimension::fit_content_function(LengthPercentage::percent(0.5));
    let preferred = Style { size: Size { width: fit_percent, height: Dimension::length(20.0) }, ..Default::default() };
    assert_eq!(cyclic_fit_content_contribution(Dimension::min_content(), preferred.clone(), text_context()), 160.0,);
    assert_eq!(cyclic_fit_content_contribution(Dimension::max_content(), preferred, text_context()), 240.0,);

    let minimum = Style {
        size: Size::from_lengths(50.0, 20.0),
        min_size: Size { width: fit_percent, height: Dimension::auto() },
        ..Default::default()
    };
    assert_eq!(cyclic_fit_content_contribution(Dimension::min_content(), minimum, text_context()), 160.0,);

    let maximum = Style {
        size: Size::from_lengths(200.0, 20.0),
        max_size: Size { width: fit_percent, height: Dimension::auto() },
        ..Default::default()
    };
    assert_eq!(
        cyclic_fit_content_contribution(
            Dimension::min_content(),
            maximum,
            TestNodeContext::ahem_text("aaaaaaaaaa".to_owned(), WritingMode::Horizontal),
        ),
        100.0,
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
fn block_child_resolves_auto_inline_policy_at_its_layout_boundary() {
    let mut tree = new_test_tree();
    let stretched = tree
        .new_leaf(Style {
            display: Display::Block,
            margin: Rect { left: length(10.0), right: length(20.0), top: length(0.0), bottom: length(0.0) },
            ..Default::default()
        })
        .unwrap();
    let ratio = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::auto(), height: Dimension::length(50.0) },
            aspect_ratio: Some(2.0),
            ..Default::default()
        })
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size::from_lengths(300.0, 200.0), ..Default::default() },
            &[stretched, ratio],
        )
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(200.0) },
        test_measure_function,
    )
    .unwrap();

    assert_eq!(tree.layout(stretched).unwrap().size.width, 270.0);
    assert_eq!(tree.layout(ratio).unwrap().size, Size { width: 100.0, height: 50.0 });
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

    for (css, expected) in [
        ("fit-content(100px)", Dimension::fit_content_function(LengthPercentage::length(100.0))),
        ("fit-content(50%)", Dimension::fit_content_function(LengthPercentage::percent(0.5))),
    ] {
        assert_eq!(Dimension::from_str(css).unwrap(), expected, "{css}");
    }
}

#[cfg(feature = "serde")]
#[test]
fn intrinsic_dimension_tags_round_trip_through_serde() {
    for value in [
        Dimension::min_content(),
        Dimension::max_content(),
        Dimension::fit_content(),
        Dimension::fit_content_function(LengthPercentage::length(100.0)),
        Dimension::fit_content_function(LengthPercentage::percent(0.5)),
        Dimension::stretch(),
    ] {
        let serialized = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<Dimension>(&serialized).unwrap(), value);
    }
}
