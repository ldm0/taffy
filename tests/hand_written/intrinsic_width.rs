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

fn layout_absolute_ratio_item(container_display: Display, mut item_style: Style, content_width: f32) -> Size<f32> {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::length(content_width), height: Dimension::length(0.0) },
            ..Style::default()
        })
        .unwrap();
    item_style.display = Display::Block;
    item_style.position = Position::Absolute;
    item_style.inset.left = LengthPercentageAuto::length(0.0);
    item_style.inset.top = LengthPercentageAuto::length(0.0);
    let item = tree.new_with_children(item_style, &[content]).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: container_display,
                size: Size { width: Dimension::length(300.0), height: Dimension::length(300.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(item).unwrap().size
}

#[test]
fn absolute_intrinsic_width_properties_use_the_ratio_content_contribution() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let preferred = layout_absolute_ratio_item(
            display,
            Style {
                size: Size { width: Dimension::min_content(), height: Dimension::length(100.0) },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            0.0,
        );
        assert_eq!(preferred, Size { width: 100.0, height: 100.0 }, "{display:?} preferred");

        let minimum = layout_absolute_ratio_item(
            display,
            Style {
                size: Size { width: Dimension::auto(), height: Dimension::length(25.0) },
                min_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                aspect_ratio: Some(4.0),
                ..Style::default()
            },
            150.0,
        );
        assert_eq!(minimum, Size { width: 100.0, height: 25.0 }, "{display:?} minimum");
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

fn layout_ratio_intrinsic_item(container_display: Display, mut item_style: Style, content_width: f32) -> Size<f32> {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::length(content_width), height: Dimension::length(0.0) },
            ..Style::default()
        })
        .unwrap();
    item_style.display = Display::Block;
    item_style.flex_grow = 0.0;
    item_style.flex_shrink = 0.0;
    item_style.align_self = Some(AlignSelf::START);
    item_style.justify_self = Some(JustifySelf::START);
    let item = tree.new_with_children(item_style, &[content]).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: container_display,
                size: Size { width: Dimension::length(300.0), height: Dimension::length(300.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(item).unwrap().size
}

/// Regressions for WPT intrinsic-size-010, intrinsic-size-014 and
/// intrinsic-size-015. CSS sizing keywords request the `SizeType::Content`
/// contribution, which is ratio-dependent when the opposite preferred size
/// is definite. The raw min-intrinsic contribution remains a separate value
/// for the automatic minimum.
#[test]
fn intrinsic_width_properties_use_the_ratio_dependent_content_contribution() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let preferred = layout_ratio_intrinsic_item(
            display,
            Style {
                size: Size { width: Dimension::min_content(), height: Dimension::length(100.0) },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            0.0,
        );
        assert_eq!(preferred, Size { width: 100.0, height: 100.0 }, "{display:?} preferred");

        let minimum = layout_ratio_intrinsic_item(
            display,
            Style {
                size: Size { width: Dimension::auto(), height: Dimension::length(25.0) },
                min_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                aspect_ratio: Some(4.0),
                ..Style::default()
            },
            150.0,
        );
        assert_eq!(minimum, Size { width: 100.0, height: 25.0 }, "{display:?} minimum");

        let maximum = layout_ratio_intrinsic_item(
            display,
            Style {
                size: Size { width: Dimension::length(200.0), height: Dimension::length(25.0) },
                max_size: Size { width: Dimension::max_content(), height: Dimension::auto() },
                aspect_ratio: Some(4.0),
                ..Style::default()
            },
            150.0,
        );
        assert_eq!(maximum, Size { width: 100.0, height: 25.0 }, "{display:?} maximum");
    }
}

#[test]
fn ratio_dependent_content_contribution_uses_the_clamped_opposite_size() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let capped = layout_ratio_intrinsic_item(
            display,
            Style {
                size: Size { width: Dimension::min_content(), height: Dimension::length(300.0) },
                max_size: Size { width: Dimension::auto(), height: Dimension::length(25.0) },
                aspect_ratio: Some(4.0),
                ..Style::default()
            },
            0.0,
        );
        assert_eq!(capped, Size { width: 100.0, height: 25.0 }, "{display:?} maximum block constraint");

        let floored = layout_ratio_intrinsic_item(
            display,
            Style {
                size: Size { width: Dimension::min_content(), height: Dimension::length(10.0) },
                min_size: Size { width: Dimension::auto(), height: Dimension::length(25.0) },
                aspect_ratio: Some(4.0),
                ..Style::default()
            },
            0.0,
        );
        assert_eq!(floored, Size { width: 100.0, height: 25.0 }, "{display:?} minimum block constraint");
    }
}

#[test]
fn ratio_content_contribution_preserves_the_ratio_sizing_box() {
    let edges = Rect {
        left: LengthPercentage::length(10.0),
        right: LengthPercentage::length(10.0),
        top: LengthPercentage::length(10.0),
        bottom: LengthPercentage::length(10.0),
    };
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let border_box = layout_ratio_intrinsic_item(
            display,
            Style {
                box_sizing: BoxSizing::BorderBox,
                size: Size { width: Dimension::min_content(), height: Dimension::length(100.0) },
                padding: edges,
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            0.0,
        );
        assert_eq!(border_box, Size { width: 100.0, height: 100.0 }, "{display:?} border box");

        let content_box = layout_ratio_intrinsic_item(
            display,
            Style {
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: Dimension::min_content(), height: Dimension::length(100.0) },
                padding: edges,
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            0.0,
        );
        assert_eq!(content_box, Size { width: 120.0, height: 120.0 }, "{display:?} content box");
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

fn percentage_block_chain(tree: &mut TaffyTree<()>) -> (NodeId, NodeId) {
    let ratio_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::auto(), height: Dimension::percent(1.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        })
        .unwrap();
    let percentage_parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: Dimension::auto(), height: Dimension::percent(1.0) },
                ..Style::default()
            },
            &[ratio_child],
        )
        .unwrap();
    (percentage_parent, ratio_child)
}

fn containing_formatting_context(tree: &mut TaffyTree<()>, display: Display, child: NodeId) -> NodeId {
    tree.new_with_children(
        Style {
            display,
            size: Size { width: Dimension::length(640.0), height: Dimension::auto() },
            ..Style::default()
        },
        &[child],
    )
    .unwrap()
}

fn intrinsic_percentage_fixture(
    container_display: Display,
    target_position: Position,
    target_height: f32,
) -> (TaffyTree<()>, NodeId, NodeId, NodeId, NodeId) {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let (percentage_parent, ratio_child) = percentage_block_chain(&mut tree);
    let target = tree
        .new_with_children(
            Style {
                display: Display::Block,
                position: target_position,
                size: Size { width: Dimension::min_content(), height: Dimension::length(target_height) },
                ..Style::default()
            },
            &[percentage_parent],
        )
        .unwrap();
    let root = containing_formatting_context(&mut tree, container_display, target);
    (tree, root, target, percentage_parent, ratio_child)
}

/// Regression for WPT `css/css-sizing/aspect-ratio/intrinsic-size-006.html`.
#[test]
fn intrinsic_width_measurement_exposes_definite_block_geometry_to_descendant_percentages() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (mut tree, root, target, percentage_parent, ratio_child) =
            intrinsic_percentage_fixture(display, Position::Relative, 100.0);

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(target).unwrap().size, Size { width: 100.0, height: 100.0 }, "{display:?} target");
        assert_eq!(
            tree.layout(percentage_parent).unwrap().size,
            Size { width: 100.0, height: 100.0 },
            "{display:?} percentage parent",
        );
        assert_eq!(
            tree.layout(ratio_child).unwrap().size,
            Size { width: 100.0, height: 100.0 },
            "{display:?} ratio child",
        );
    }
}

#[test]
fn absolute_intrinsic_width_measurement_exposes_authored_block_geometry() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (mut tree, root, target, percentage_parent, ratio_child) =
            intrinsic_percentage_fixture(display, Position::Absolute, 100.0);

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(target).unwrap().size, Size { width: 100.0, height: 100.0 }, "{display:?} target");
        assert_eq!(
            tree.layout(percentage_parent).unwrap().size,
            Size { width: 100.0, height: 100.0 },
            "{display:?} percentage parent",
        );
        assert_eq!(
            tree.layout(ratio_child).unwrap().size,
            Size { width: 100.0, height: 100.0 },
            "{display:?} ratio child",
        );
    }
}

/// Regression for WPT `css/css-sizing/aspect-ratio/intrinsic-size-008.html`.
#[test]
fn intrinsic_width_dependency_remeasures_after_definite_block_size_changes() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (mut tree, root, target, _, _) = intrinsic_percentage_fixture(display, Position::Relative, 200.0);

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(
            tree.layout(target).unwrap().size,
            Size { width: 200.0, height: 200.0 },
            "{display:?} initial layout",
        );

        tree.set_style(
            target,
            Style {
                display: Display::Block,
                size: Size { width: Dimension::min_content(), height: Dimension::length(100.0) },
                ..Style::default()
            },
        )
        .unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(target).unwrap().size, Size { width: 100.0, height: 100.0 }, "{display:?} relayout",);
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
