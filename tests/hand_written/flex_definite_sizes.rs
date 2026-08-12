//! Flex-specific definite-size rules for descendant percentage resolution.
//!
//! These regressions mirror the semantic cases in CSS Flexbox §9.9 and WPT
//! `percentage-heights-001.html` / `percentage-heights-018.html`.

use taffy::prelude::*;
use taffy::WritingMode;

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
