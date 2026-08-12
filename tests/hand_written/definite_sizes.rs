//! Definite-size provenance at formatting-context boundaries.
//!
//! Numeric used geometry is not necessarily a percentage-resolution basis:
//! content sizing and intrinsic keywords remain indefinite after measurement,
//! while lengths and sizes transferred from a definite axis remain definite.

use taffy::prelude::*;

fn percentage_child(tree: &mut TaffyTree<()>) -> NodeId {
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(10.0), height: length(20.0) },
            ..Default::default()
        })
        .unwrap();
    tree.new_with_children(
        Style {
            display: Display::Block,
            size: Size { width: length(10.0), height: percent(0.5) },
            ..Default::default()
        },
        &[content],
    )
    .unwrap()
}

#[test]
fn percentage_height_resolves_against_an_authored_definite_height() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = percentage_child(&mut tree);
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(100.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().size.height, 50.0);
}

#[test]
fn minimum_clamped_auto_height_remains_indefinite() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = percentage_child(&mut tree);
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                min_size: Size { width: auto(), height: length(100.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(child).unwrap().size.height, 20.0);
}

#[test]
fn clamped_authored_height_keeps_its_final_definite_value() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = percentage_child(&mut tree);
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(100.0) },
                max_size: Size { width: auto(), height: length(60.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.height, 60.0);
    assert_eq!(tree.layout(child).unwrap().size.height, 30.0);
}

#[test]
fn aspect_ratio_transfers_definiteness_to_the_auto_axis() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = percentage_child(&mut tree);
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                aspect_ratio: Some(2.0),
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.height, 50.0);
    assert_eq!(tree.layout(child).unwrap().size.height, 25.0);
}

#[test]
fn contained_intrinsic_height_remains_indefinite_after_resolution() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = percentage_child(&mut tree);
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_size_containment(
        parent,
        SizeContainment::new(Size { width: false, height: true }, Size { width: None, height: Some(100.0) }),
    )
    .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(child).unwrap().size.height, 20.0);
}

#[test]
fn intrinsic_keyword_height_remains_indefinite_after_measurement() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = percentage_child(&mut tree);
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: Dimension::max_content() },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.height, 20.0);
    assert_eq!(tree.layout(child).unwrap().size.height, 20.0);
}
