//! Relative-position inset resolution for block-flow children.
//!
//! Percentage block-axis insets resolve against a definite containing-block
//! height. If that height is indefinite, the percentage-bearing inset behaves
//! as `auto` rather than resolving against the eventual content/min height.

use taffy::prelude::*;
use taffy_test_helpers::new_test_tree;

#[test]
fn block_percentage_top_resolves_against_definite_parent_height() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            position: Position::Relative,
            size: Size { width: length(10.0), height: length(10.0) },
            inset: Rect { left: auto(), right: auto(), top: percent(0.25), bottom: auto() },
            ..Default::default()
        })
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(400.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().location.y, 100.0);
}

#[test]
fn block_percentage_top_stays_unresolved_for_auto_height_parent() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            position: Position::Relative,
            size: Size { width: length(100.0), height: length(100.0) },
            inset: Rect { left: auto(), right: auto(), top: percent(-100.0), bottom: auto() },
            ..Default::default()
        })
        .unwrap();
    let root = tree
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

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().location.y, 0.0);
}

#[test]
fn block_percentage_top_stays_unresolved_for_absolute_auto_height_parent() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            position: Position::Relative,
            size: Size { width: length(100.0), height: length(100.0) },
            inset: Rect { left: auto(), right: auto(), top: percent(-100.0), bottom: auto() },
            ..Default::default()
        })
        .unwrap();
    let absolute_parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                position: Position::Absolute,
                size: Size { width: length(100.0), height: auto() },
                min_size: Size { width: auto(), height: length(100.0) },
                inset: Rect { left: length(0.0), right: auto(), top: length(0.0), bottom: auto() },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(200.0), height: length(200.0) },
                ..Default::default()
            },
            &[absolute_parent],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(absolute_parent).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(child).unwrap().location.y, 0.0);
}

#[cfg(feature = "flexbox")]
#[test]
fn block_percentage_top_resolves_against_a_definite_flex_stretch_size() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            position: Position::Relative,
            size: Size { width: length(10.0), height: length(10.0) },
            inset: Rect { left: auto(), right: auto(), top: percent(0.5), bottom: auto() },
            ..Default::default()
        })
        .unwrap();
    let stretched = tree
        .new_with_children(Style { display: Display::Block, size: Size::auto(), ..Default::default() }, &[child])
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(100.0), height: length(200.0) },
                ..Default::default()
            },
            &[stretched],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(stretched).unwrap().size.height, 200.0);
    assert_eq!(tree.layout(child).unwrap().location.y, 100.0);
}
