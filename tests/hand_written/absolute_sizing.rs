use taffy::prelude::*;
use taffy::{Direction, LogicalBoxStrut, LogicalSize, Overflow, Point, WritingDirection, WritingMode};

const MODES: [WritingMode; 3] = [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr];
const OWNERS: [Display; 3] = [Display::Block, Display::Flex, Display::Grid];

#[test]
fn measured_block_content_preserves_the_ratio_dependent_minimum() {
    for mode in MODES {
        for display in OWNERS {
            for (name, minimum, maximum, overflow, expected) in [
                ("automatic minimum", auto(), auto(), Overflow::Visible, 100.0),
                ("explicit zero", length(0.0), auto(), Overflow::Visible, 50.0),
                ("hidden overflow", auto(), auto(), Overflow::Hidden, 50.0),
                ("authored maximum", auto(), length(75.0), Overflow::Visible, 75.0),
            ] {
                let mut tree = TaffyTree::<()>::new();
                let child = tree
                    .new_leaf_with_context(
                        Style {
                            display: Display::Block,
                            position: Position::Absolute,
                            inset: inset_with_auto_block_end(mode),
                            size: mode.to_physical(LogicalSize { inline_size: length(100.0), block_size: auto() }),
                            min_size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: minimum }),
                            max_size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: maximum }),
                            overflow: Point { x: overflow, y: overflow },
                            aspect_ratio: Some(if mode.is_horizontal() { 2.0 } else { 0.5 }),
                            ..Style::default()
                        },
                        (),
                    )
                    .unwrap();
                let owner = tree
                    .new_with_children(
                        Style {
                            display,
                            size: mode
                                .to_physical(LogicalSize { inline_size: length(240.0), block_size: length(300.0) }),
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                for node in [child, owner] {
                    tree.set_writing_mode(node, mode).unwrap();
                }
                tree.compute_layout_with_measure(owner, Size::MAX_CONTENT, |_, _, _, _, _| {
                    mode.to_physical(LogicalSize { inline_size: 100.0, block_size: 100.0 })
                })
                .unwrap();
                assert_eq!(
                    mode.to_logical(tree.layout(child).unwrap().size),
                    LogicalSize { inline_size: 100.0, block_size: expected },
                    "{mode:?} {display:?} {name}"
                );
            }
        }
    }
}

fn inset_with_auto_block_end(mode: WritingMode) -> Rect<LengthPercentageAuto> {
    WritingDirection::new(mode, Direction::Ltr).to_physical_box_strut(LogicalBoxStrut {
        inline_start: length(0.0),
        inline_end: length(0.0),
        block_start: length(0.0),
        block_end: auto(),
    })
}

#[test]
fn absolute_block_insets_can_determine_the_ratio_inline_size() {
    for mode in MODES {
        for display in OWNERS {
            let mut tree = TaffyTree::<()>::new();
            let child = tree
                .new_leaf(Style {
                    display: Display::Block,
                    position: Position::Absolute,
                    inset: WritingDirection::new(mode, Direction::Ltr).to_physical_box_strut(LogicalBoxStrut {
                        inline_start: auto(),
                        inline_end: auto(),
                        block_start: percent(0.3),
                        block_end: percent(0.5),
                    }),
                    aspect_ratio: Some(if mode.is_horizontal() { 3.0 } else { 1.0 / 3.0 }),
                    ..Style::default()
                })
                .unwrap();
            let owner = tree
                .new_with_children(
                    Style {
                        display,
                        size: mode.to_physical(LogicalSize { inline_size: length(240.0), block_size: length(300.0) }),
                        ..Style::default()
                    },
                    &[child],
                )
                .unwrap();
            for node in [child, owner] {
                tree.set_writing_mode(node, mode).unwrap();
            }
            tree.compute_layout(owner, Size::MAX_CONTENT).unwrap();
            assert_eq!(
                mode.to_logical(tree.layout(child).unwrap().size),
                LogicalSize { inline_size: 180.0, block_size: 60.0 },
                "{mode:?} {display:?}"
            );
        }
    }
}

#[test]
fn absolute_content_block_size_respects_automatic_and_authored_limits() {
    for mode in MODES {
        for display in OWNERS {
            for (name, minimum, maximum, overflow, expected) in [
                ("automatic minimum", auto(), auto(), Overflow::Visible, 100.0),
                ("explicit zero", length(0.0), auto(), Overflow::Visible, 50.0),
                ("hidden overflow", auto(), auto(), Overflow::Hidden, 50.0),
                ("authored maximum", auto(), length(75.0), Overflow::Visible, 75.0),
            ] {
                let mut tree = TaffyTree::<()>::new();
                let content = tree
                    .new_leaf(Style {
                        display: Display::Block,
                        size: mode.to_physical(LogicalSize { inline_size: length(100.0), block_size: length(100.0) }),
                        ..Style::default()
                    })
                    .unwrap();
                let child = tree
                    .new_with_children(
                        Style {
                            display: Display::Block,
                            position: Position::Absolute,
                            inset: inset_with_auto_block_end(mode),
                            size: mode.to_physical(LogicalSize { inline_size: length(100.0), block_size: auto() }),
                            min_size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: minimum }),
                            max_size: mode.to_physical(LogicalSize { inline_size: auto(), block_size: maximum }),
                            aspect_ratio: Some(if mode.is_horizontal() { 2.0 } else { 0.5 }),
                            overflow: Point { x: overflow, y: overflow },
                            ..Style::default()
                        },
                        &[content],
                    )
                    .unwrap();
                let owner = tree
                    .new_with_children(
                        Style {
                            display,
                            size: mode
                                .to_physical(LogicalSize { inline_size: length(240.0), block_size: length(300.0) }),
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
                    LogicalSize { inline_size: 100.0, block_size: expected },
                    "{mode:?} {display:?} {name}"
                );
            }
        }
    }
}

#[test]
fn absolute_ratio_sizing_distinguishes_normal_and_explicit_stretch() {
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for display in [Display::Block, Display::Flex, Display::Grid] {
            for (name, inline, block, alignment, justification, expected) in [
                ("normal", length(100.0), auto(), None, None, (100.0, 100.0)),
                ("block stretch", length(100.0), auto(), Some(AlignSelf::STRETCH), None, (100.0, 300.0)),
                ("both auto", auto(), auto(), None, None, (240.0, 240.0)),
                ("stronger block stretch", auto(), auto(), Some(AlignSelf::STRETCH), None, (300.0, 300.0)),
                ("both stretch", auto(), auto(), Some(AlignSelf::STRETCH), Some(AlignSelf::STRETCH), (240.0, 300.0)),
                ("stronger inline stretch", auto(), length(100.0), None, Some(AlignSelf::STRETCH), (240.0, 100.0)),
            ] {
                let mut tree = TaffyTree::<()>::new();
                let child = tree
                    .new_leaf(Style {
                        display: Display::Block,
                        position: Position::Absolute,
                        inset: Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: length(0.0) },
                        size: mode.to_physical(LogicalSize { inline_size: inline, block_size: block }),
                        aspect_ratio: Some(1.0),
                        align_self: alignment,
                        justify_self: justification,
                        ..Default::default()
                    })
                    .unwrap();
                let root = tree
                    .new_with_children(
                        Style {
                            display,
                            size: mode
                                .to_physical(LogicalSize { inline_size: length(240.0), block_size: length(300.0) }),
                            ..Default::default()
                        },
                        &[child],
                    )
                    .unwrap();
                for node in [root, child] {
                    tree.set_writing_mode(node, mode).unwrap();
                }
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                assert_eq!(
                    mode.to_logical(tree.layout(child).unwrap().size),
                    LogicalSize { inline_size: expected.0, block_size: expected.1 },
                    "{mode:?} {display:?} {name}"
                );
            }
        }
    }
}
