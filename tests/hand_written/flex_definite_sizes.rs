//! Flex-specific definite-size rules for descendant percentage resolution.
//!
//! These regressions mirror the semantic cases in CSS Flexbox §9.9 and WPT
//! `percentage-heights-001.html` / `percentage-heights-018.html`.

use taffy::prelude::*;
use taffy::{Overflow, Point, WritingMode};
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

fn fixed_block(tree: &mut TaffyTree<()>, height: f32) -> NodeId {
    tree.new_leaf(Style {
        display: Display::Block,
        size: Size { width: length(100.0), height: length(height) },
        ..Default::default()
    })
    .unwrap()
}

fn percentage_block(tree: &mut TaffyTree<()>, percentage: f32) -> NodeId {
    tree.new_leaf(Style {
        display: Display::Block,
        size: Size { width: length(100.0), height: percent(percentage) },
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn indefinite_percentage_basis_overrides_an_authored_main_size() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let fixed = fixed_block(&mut tree, 100.0);
    let percentage = percentage_block(&mut tree, 1.0);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(100.0) },
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: percent(0.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                ..Default::default()
            },
            &[fixed, percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(item).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 0.0);
}

#[test]
fn definite_container_main_size_makes_the_post_flex_size_definite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: Dimension::content(),
                flex_grow: 1.0,
                ..Default::default()
            },
            &[percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: length(200.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 200.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 100.0);
}

#[test]
fn definite_flex_basis_makes_the_post_flex_size_definite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            &[percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 50.0);
}

/// Regression for WPT
/// `css/css-flexbox/flex-one-sets-flex-basis-to-zero-px.html`.
///
/// A definite zero basis produces a zero hypothetical main size even when the
/// item can grow. A zero percentage stays content-based while the container's
/// block size is indefinite. The container's real auto block size comes from
/// those hypothetical sizes, not from an intrinsic-contribution flex fraction.
#[test]
fn content_sized_column_uses_hypothetical_main_sizes_before_flexing() {
    let layout = |writing_mode: WritingMode, flex_basis: Dimension, flex_grow: f32| {
        let mut tree = new_test_tree();
        tree.disable_rounding();
        let item = tree
            .new_leaf_with_context(
                Style {
                    min_size: Size::from_lengths(0.0, 0.0),
                    flex_basis,
                    flex_grow,
                    flex_shrink: 1.0,
                    ..Default::default()
                },
                TestNodeContext::fixed(14.0, 14.0),
            )
            .unwrap();
        tree.set_writing_mode(item, writing_mode).unwrap();
        let size = match writing_mode {
            WritingMode::HorizontalTb => Size { width: length(100.0), height: auto() },
            WritingMode::VerticalLr | WritingMode::VerticalRl | WritingMode::SidewaysLr | WritingMode::SidewaysRl => {
                Size { width: auto(), height: length(100.0) }
            }
        };
        let container = tree
            .new_with_children(
                Style { display: Display::Flex, flex_direction: FlexDirection::Column, size, ..Default::default() },
                &[item],
            )
            .unwrap();
        tree.set_writing_mode(container, writing_mode).unwrap();
        let root = tree
            .new_with_children(Style { display: Display::Block, size, ..Default::default() }, &[container])
            .unwrap();
        tree.set_writing_mode(root, writing_mode).unwrap();

        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, test_measure_function).unwrap();
        let block_size = |size: Size<f32>| match writing_mode {
            WritingMode::HorizontalTb => size.height,
            WritingMode::VerticalLr | WritingMode::VerticalRl | WritingMode::SidewaysLr | WritingMode::SidewaysRl => {
                size.width
            }
        };
        (block_size(tree.layout(container).unwrap().size), block_size(tree.layout(item).unwrap().size))
    };

    for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
        for flex_grow in [0.5, 1.0] {
            assert_eq!(
                layout(writing_mode, length(0.0), flex_grow),
                (0.0, 0.0),
                "{writing_mode:?} definite basis with grow {flex_grow}"
            );
            assert_eq!(
                layout(writing_mode, percent(0.0), flex_grow),
                (14.0, 14.0),
                "{writing_mode:?} percentage basis with grow {flex_grow}"
            );
        }
    }
}

#[test]
fn auto_basis_retrieves_a_definite_authored_main_size() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(100.0) },
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: auto(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            &[percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 50.0);
}

#[test]
fn definite_cross_ratio_makes_a_content_based_main_size_definite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: Dimension::content(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                aspect_ratio: Some(2.0),
                ..Default::default()
            },
            &[percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 25.0);
}

#[test]
fn definite_post_flex_main_size_transfers_a_definite_ratio_cross_size() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                align_self: Some(AlignSelf::FLEX_START),
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                aspect_ratio: Some(2.0),
                ..Default::default()
            },
            &[percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size { width: auto(), height: auto() }, ..Default::default() },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 50.0 });
    assert_eq!(tree.layout(percentage).unwrap().size.height, 25.0);
}

#[test]
fn intrinsic_flex_basis_stays_indefinite_after_measurement() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let fixed = fixed_block(&mut tree, 100.0);
    let percentage = percentage_block(&mut tree, 1.0);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: Dimension::max_content(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            &[fixed, percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 0.0);
}

#[test]
fn auto_cross_stretch_makes_the_line_cross_size_definite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let fixed = fixed_block(&mut tree, 50.0);
    let percentage = percentage_block(&mut tree, 0.5);
    let item =
        tree.new_with_children(Style { display: Display::Block, ..Default::default() }, &[fixed, percentage]).unwrap();
    let container = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size { width: length(100.0), height: auto() }, ..Default::default() },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 25.0);
}

/// Regression for WPT `css/css-flexbox/flex-minimum-size-002.html`.
///
/// A percentage minimum whose containing-block block size is indefinite has a
/// zero cyclic fallback. It remains an authored minimum and must not silently
/// turn into Flexbox's content-based `min-size:auto`.
#[test]
fn indefinite_percentage_minimum_does_not_become_a_flex_automatic_minimum() {
    let mut tree = new_test_tree();
    tree.disable_rounding();

    let percentage_minimum = tree
        .new_leaf_with_context(
            Style {
                display: Display::Block,
                min_size: Size { width: auto(), height: percent(1.0) },
                ..Default::default()
            },
            TestNodeContext::fixed(100.0, 13.0),
        )
        .unwrap();
    let inflexible = tree
        .new_leaf_with_context(
            Style { display: Display::Block, flex_grow: 0.0, flex_shrink: 0.0, ..Default::default() },
            TestNodeContext::fixed(50.0, 13.0),
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                max_size: Size { width: auto(), height: length(0.0) },
                overflow: Point { x: Overflow::Hidden, y: Overflow::Hidden },
                ..Default::default()
            },
            &[percentage_minimum, inflexible],
        )
        .unwrap();
    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(container).unwrap().size.height, 0.0);
    assert_eq!(tree.layout(percentage_minimum).unwrap().size.height, 0.0);
    assert_eq!(tree.layout(inflexible).unwrap().size.height, 13.0);
}

#[test]
fn non_stretched_auto_cross_size_remains_indefinite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let fixed = fixed_block(&mut tree, 50.0);
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style { display: Display::Block, align_self: Some(AlignSelf::FLEX_START), ..Default::default() },
            &[fixed, percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(100.0), height: length(50.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 0.0);
}

#[test]
fn stretch_cross_limit_does_not_make_content_sizing_definite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let fixed = fixed_block(&mut tree, 50.0);
    let percentage = percentage_block(&mut tree, 0.5);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                align_self: Some(AlignSelf::FLEX_START),
                max_size: Size { width: auto(), height: Dimension::stretch() },
                ..Default::default()
            },
            &[fixed, percentage],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(100.0), height: length(50.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(percentage).unwrap().size.height, 0.0);
}

#[test]
fn definite_flex_main_size_follows_vertical_logical_axes() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let percentage = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: percent(0.5), height: length(20.0) },
            ..Default::default()
        })
        .unwrap();
    tree.set_writing_mode(percentage, WritingMode::VerticalRl).unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: length(0.0), height: auto() },
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            &[percentage],
        )
        .unwrap();
    tree.set_writing_mode(item, WritingMode::VerticalRl).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: auto(), height: length(100.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();
    tree.set_writing_mode(container, WritingMode::VerticalRl).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(percentage).unwrap().size.width, 50.0);
}
