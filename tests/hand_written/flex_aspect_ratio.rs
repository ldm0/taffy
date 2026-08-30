use taffy::prelude::*;
use taffy::style::Float;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

/// Regression for WPT css/css-flexbox/aspect-ratio-transferred-max-size.html.
///
/// A max-size transferred into the main axis constrains the hypothetical main
/// size, but it must not clamp the size produced by flexible-length resolution.
#[test]
fn transferred_max_size_does_not_clamp_the_flexed_main_size() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            max_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            aspect_ratio: Some(0.5),
            flex_basis: Dimension::length(0.0),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/aspect-ratio-intrinsic-size-003.html.
#[test]
fn auto_flex_basis_uses_a_definite_stretched_cross_size_through_aspect_ratio() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree.new_leaf(Style { aspect_ratio: Some(1.0), ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-023.html.
#[test]
fn flexed_replaced_main_size_transfers_through_the_preferred_ratio() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
                min_size: Size { width: Dimension::auto(), height: Dimension::length(0.0) },
                aspect_ratio: Some(1.0),
                item_is_replaced: true,
                flex_basis: Dimension::length(0.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                ..Style::default()
            },
            TestNodeContext::fixed(20.0, 50.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn direct_cross_size_wins_after_main_axis_flexing() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(50.0), height: Dimension::length(30.0) },
            aspect_ratio: Some(1.0),
            flex_basis: Dimension::length(0.0),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 30.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 30.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-054.html.
///
/// A preferred cross size transferred through `aspect-ratio` participates in
/// the content-based automatic minimum of a non-replaced flex item. It must
/// therefore keep the item from shrinking into a zero-width container.
#[test]
fn non_replaced_automatic_minimum_includes_the_transferred_size_suggestion() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::length(100.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn transferred_suggestion_sets_the_automatic_minimum_when_the_main_size_is_auto() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-045.html.
#[test]
fn automatic_row_stretch_cross_size_sets_the_transferred_minimum() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree.new_leaf(Style { aspect_ratio: Some(1.0), ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-046.html.
#[test]
fn automatic_column_stretch_cross_size_sets_the_transferred_minimum() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree.new_leaf(Style { aspect_ratio: Some(1.0), ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: Dimension::length(100.0), height: Dimension::length(0.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

fn layout_column_ratio_item_with_block_content(item_style: Style, content_height: f32) -> (Size<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            size: Size { width: Dimension::auto(), height: Dimension::length(content_height) },
            ..Style::default()
        })
        .unwrap();
    let item = tree.new_with_children(item_style, &[content]).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                float: Float::Left,
                size: Size { width: Dimension::auto(), height: Dimension::length(1.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: Dimension::length(400.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[container],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    (tree.layout(container).unwrap().size, tree.layout(item).unwrap().size)
}

/// Regression for the min-width subtest in WPT
/// css/css-sizing/aspect-ratio/flex-aspect-ratio-026.html.
#[test]
fn column_intrinsic_cross_size_does_not_transfer_block_content_through_ratio() {
    let (container_size, item_size) = layout_column_ratio_item_with_block_content(
        Style {
            display: Display::Block,
            box_sizing: BoxSizing::BorderBox,
            min_size: Size { width: Dimension::length(25.0), height: Dimension::auto() },
            padding: Rect { left: LengthPercentage::length(15.0), top: LengthPercentage::length(10.0), ..Rect::zero() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        190.0,
    );

    assert_eq!(container_size.width, 25.0);
    assert_eq!(item_size, Size { width: 25.0, height: 200.0 });
}

/// Regression for the max-width subtest in WPT
/// css/css-sizing/aspect-ratio/flex-aspect-ratio-026.html.
#[test]
fn column_content_minimum_is_capped_by_the_transferred_cross_maximum() {
    let (container_size, item_size) = layout_column_ratio_item_with_block_content(
        Style {
            display: Display::Block,
            box_sizing: BoxSizing::BorderBox,
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            max_size: Size { width: Dimension::length(25.0), height: Dimension::auto() },
            padding: Rect { left: LengthPercentage::length(15.0), top: LengthPercentage::length(10.0), ..Rect::zero() },
            aspect_ratio: Some(1.0 / 8.0),
            ..Style::default()
        },
        500.0,
    );

    assert_eq!(container_size.width, 25.0);
    assert_eq!(item_size, Size { width: 25.0, height: 200.0 });
}

/// Regression for WPT css/css-flexbox/flexbox-min-height-auto-002a.html.
///
/// The flex item's definite width is its intrinsic inline contribution. Its
/// larger authored height must not transfer back through the image's intrinsic
/// ratio and widen the shrink-to-fit column flex container.
#[test]
fn replaced_definite_width_controls_a_column_flex_intrinsic_contribution() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                item_is_replaced: true,
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: Dimension::length(30.0), height: Dimension::length(100.0) },
                aspect_ratio: Some(1.0),
                border: Rect::length(2.0),
                ..Style::default()
            },
            // The generic test tree cannot dispatch to `compute_replaced_layout`
            // directly. Return the 100px ratio-transferred content probe that
            // the replaced sizing entry point produces for this style.
            TestNodeContext::fixed(100.0, 100.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                float: Float::Left,
                size: Size { width: Dimension::auto(), height: Dimension::length(1.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: Dimension::length(400.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[container],
        )
        .unwrap();

    tree.compute_layout_with_measure(parent, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 34.0, height: 1.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 34.0, height: 34.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-037.html.
///
/// Flexbox part E obtains the block-axis flex base size from the item's
/// fit-content inline size when a preferred aspect ratio is present.
#[test]
fn column_flex_base_size_transfers_the_fit_content_inline_size() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: Dimension::auto(), height: Dimension::length(0.0) },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::START),
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for the transferred maximum in WPT
/// css/css-sizing/aspect-ratio/flex-aspect-ratio-039.html.
#[test]
fn transferred_cross_maximum_clamps_the_hypothetical_main_size() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                max_size: Size { width: Dimension::auto(), height: Dimension::length(50.0) },
                aspect_ratio: Some(2.0),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(800.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 50.0 });
}

#[test]
fn column_automatic_minimum_transfers_the_preferred_cross_size() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::length(100.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: Dimension::length(100.0), height: Dimension::length(0.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn explicit_zero_minimum_disables_the_flex_content_based_automatic_minimum() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::length(100.0) },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 0.0, height: 100.0 });
}

#[test]
fn non_replaced_automatic_minimum_uses_the_larger_content_and_transferred_suggestion() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(150.0), height: Dimension::length(10.0) },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 150.0, height: 100.0 });
}

#[test]
fn replaced_automatic_minimum_uses_the_smaller_content_and_transferred_suggestion() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                aspect_ratio: Some(1.0),
                item_is_replaced: true,
                ..Style::default()
            },
            TestNodeContext::fixed(150.0, 150.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/flex-item-compressible-001.html.
///
/// The percentage preferred width is definite for final layout, but its
/// percentage part resolves against zero while Flexbox builds the replaced
/// item's specified-size suggestion. The item's 240px natural width therefore
/// does not prevent it from shrinking into the remaining 100px.
#[test]
fn replaced_percentage_size_is_compressible_in_a_flex_automatic_minimum() {
    let mut tree = new_test_tree();
    let spacer = tree
        .new_leaf(Style { flex_basis: Dimension::length(200.0), flex_grow: 0.0, flex_shrink: 0.0, ..Style::default() })
        .unwrap();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::percent(1.0), height: Dimension::auto() },
                item_is_replaced: true,
                ..Style::default()
            },
            TestNodeContext::fixed(240.0, 20.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(300.0), height: Dimension::length(40.0) },
                ..Style::default()
            },
            &[spacer, item],
        )
        .unwrap();

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(spacer).unwrap().size.width, 200.0);
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 40.0 });
}

/// Regression for WPT
/// css/css-flexbox/flex-minimum-width-flex-items-013.html.
///
/// The authored main size supplies the specified-size suggestion, but it does
/// not replace the content-size suggestion. A replaced item first chooses the
/// smaller content/transferred suggestion, then caps that result by its
/// specified-size suggestion.
#[test]
fn replaced_stretched_cross_size_limits_the_automatic_minimum_before_authored_main_size() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::length(999.0), height: Dimension::auto() },
                aspect_ratio: Some(2.0),
                item_is_replaced: true,
                ..Style::default()
            },
            TestNodeContext::fixed(300.0, 150.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(50.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 50.0 });
}

/// Regression for WPT css/css-flexbox/flex-aspect-ratio-img-column-011.html.
///
/// The preferred width supplies the specified-size suggestion, but a replaced
/// item's content-size suggestion still comes from its natural content. The
/// automatic minimum is therefore `min(100px, 10px)`, allowing the item to
/// shrink to the 10px flex container.
#[test]
fn replaced_natural_content_suggestion_is_independent_of_the_preferred_main_size() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                aspect_ratio: Some(1.0),
                item_is_replaced: true,
                ..Style::default()
            },
            TestNodeContext::fixed(10.0, 10.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(10.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.width, 10.0);
}

#[test]
fn definite_main_maximum_caps_the_flex_content_based_automatic_minimum() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::length(100.0) },
            max_size: Size { width: Dimension::length(80.0), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(0.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 80.0, height: 100.0 });
}
