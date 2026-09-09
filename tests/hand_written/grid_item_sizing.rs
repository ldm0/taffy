use taffy::prelude::*;
use taffy::{LogicalSize, WritingMode};

#[test]
fn explicit_stretch_is_independent_of_the_preferred_ratio_in_both_axes() {
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            for (name, align, justify, inline, block, expected) in [
                ("normal", None, None, auto(), auto(), [240.0, 120.0]),
                ("both stretch", Some(AlignSelf::STRETCH), Some(AlignSelf::STRETCH), auto(), auto(), [240.0, 300.0]),
                ("block stretch", Some(AlignSelf::STRETCH), None, auto(), auto(), [600.0, 300.0]),
                ("inline stretch", None, Some(AlignSelf::STRETCH), auto(), auto(), [240.0, 120.0]),
                ("specified inline", Some(AlignSelf::STRETCH), None, length(100.0), auto(), [100.0, 300.0]),
                ("specified block", None, Some(AlignSelf::STRETCH), auto(), length(100.0), [240.0, 100.0]),
            ] {
                let mut tree = TaffyTree::<()>::new();
                // A child forces actual block/flex/grid dispatch instead of
                // the numeric tree's empty-node leaf fast path.
                let content = tree
                    .new_leaf(Style { size: Size { width: length(0.0), height: length(0.0) }, ..Style::default() })
                    .unwrap();
                let child = tree
                    .new_with_children(
                        Style {
                            display,
                            size: mode.to_physical(LogicalSize { inline_size: inline, block_size: block }),
                            aspect_ratio: Some(if mode.is_horizontal() { 2.0 } else { 0.5 }),
                            align_self: align,
                            justify_self: justify,
                            ..Style::default()
                        },
                        &[content],
                    )
                    .unwrap();
                let owner = tree
                    .new_with_children(
                        Style {
                            display: Display::Grid,
                            grid_template_columns: vec![length(240.0)],
                            grid_template_rows: vec![length(300.0)],
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                for node in [content, child, owner] {
                    tree.set_writing_mode(node, mode).unwrap();
                }
                tree.compute_layout(owner, Size::MAX_CONTENT).unwrap();
                assert_eq!(
                    mode.to_logical(tree.layout(child).unwrap().size),
                    LogicalSize { inline_size: expected[0], block_size: expected[1] },
                    "{mode:?} {display:?} {name}"
                );
            }
        }
    }
}

#[test]
fn explicit_normal_does_not_inherit_parent_stretch() {
    for alignment in [None, Some(AlignSelf::NORMAL)] {
        let mut tree = TaffyTree::<()>::new();
        let child = tree
            .new_leaf(Style {
                display: Display::Block,
                aspect_ratio: Some(2.0),
                align_self: alignment,
                justify_self: alignment,
                ..Style::default()
            })
            .unwrap();
        let owner = tree
            .new_with_children(
                Style {
                    display: Display::Grid,
                    grid_template_columns: vec![length(240.0)],
                    grid_template_rows: vec![length(300.0)],
                    align_items: Some(AlignItems::STRETCH),
                    justify_items: Some(AlignItems::STRETCH),
                    ..Style::default()
                },
                &[child],
            )
            .unwrap();
        tree.compute_layout(owner, Size::MAX_CONTENT).unwrap();
        assert_eq!(
            tree.layout(child).unwrap().size,
            Size { width: 240.0, height: if alignment.is_some() { 120.0 } else { 300.0 } }
        );
    }
}

#[test]
fn content_block_minimum_is_owned_by_each_actual_formatter() {
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            for (name, min, max, overflow, expected_block) in [
                ("automatic", auto(), auto(), taffy::Overflow::Visible, 180.0),
                ("minimum zero", length(0.0), auto(), taffy::Overflow::Visible, 120.0),
                ("hidden", auto(), auto(), taffy::Overflow::Hidden, 120.0),
                ("authored maximum", auto(), length(150.0), taffy::Overflow::Visible, 150.0),
            ] {
                let mut tree = TaffyTree::<()>::new();
                let content = tree
                    .new_leaf(Style {
                        size: mode.to_physical(LogicalSize { inline_size: length(80.0), block_size: length(180.0) }),
                        ..Style::default()
                    })
                    .unwrap();
                let child = tree
                    .new_with_children(
                        Style {
                            display,
                            aspect_ratio: Some(if mode.is_horizontal() { 2.0 } else { 0.5 }),
                            min_size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: min }),
                            max_size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: max }),
                            overflow: taffy::Point { x: overflow, y: overflow },
                            ..Style::default()
                        },
                        &[content],
                    )
                    .unwrap();
                let owner = tree
                    .new_with_children(
                        Style {
                            display: Display::Grid,
                            grid_template_columns: vec![length(240.0)],
                            grid_template_rows: vec![length(300.0)],
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                for node in [content, child, owner] {
                    tree.set_writing_mode(node, mode).unwrap();
                }
                tree.compute_layout(owner, Size::MAX_CONTENT).unwrap();
                assert_eq!(
                    mode.to_logical(tree.layout(child).unwrap().size),
                    LogicalSize { inline_size: 240.0, block_size: expected_block },
                    "{mode:?} {display:?} {name}"
                );
            }
        }
    }
}
