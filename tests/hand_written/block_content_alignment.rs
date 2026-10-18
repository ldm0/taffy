use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
use taffy::{
    AutoSizeBehavior, LayoutInput, LayoutOutput, RequestedAxis, RunMode, SizingMode, SizingPurpose, WritingMode,
};

fn output(tree: &mut TestTree) -> LayoutOutput {
    tree.compute_child_layout(
        NodeId::from(0_usize),
        LayoutInput {
            known_dimensions: Size::NONE,
            known_dimensions_are_definite: Size { width: false, height: false },
            parent_size: Size::NONE,
            parent_writing_mode: WritingMode::HorizontalTb,
            available_space: Size::MAX_CONTENT,
            sizing_mode: SizingMode::InherentSize,
            sizing_purpose: SizingPurpose::Layout,
            axis: RequestedAxis::Both,
            run_mode: RunMode::PerformLayout,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            vertical_margins_are_collapsible: Line::FALSE,
        },
    )
}

#[test]
fn block_content_end_excludes_positioned_overflow_and_tracks_percentages() {
    for relative in [false, true] {
        for percent_height in [false, true] {
            let root = TestNode::container(
                Display::Block,
                Style {
                    position: Position::Relative,
                    size: Size { width: length(100.0), height: length(120.0) },
                    padding: Rect::length(5.0),
                    border: Rect::length(5.0),
                    ..Style::default()
                },
                Rect::ZERO,
            );
            let child = TestNode::leaf(
                Style {
                    size: Size {
                        width: length(10.0),
                        height: if percent_height { percent(0.5) } else { length(10.0) },
                    },
                    position: Position::Relative,
                    inset: Rect { top: if relative { percent(0.5) } else { auto() }, ..Rect::auto() },
                    ..Style::default()
                },
                Size::ZERO,
            );
            let mut tree = TestTree::new(root, child);
            tree.nodes.push(TestNode::leaf(
                Style { position: Position::Absolute, inset: Rect::length(0.0), ..Style::default() },
                Size::ZERO,
            ));
            tree.nodes[0].children.push(2);
            let result = output(&mut tree);
            let height = if percent_height { 50.0 } else { 10.0 };
            assert_eq!(result.block_content_end, Some(10.0 + height));
            assert_eq!(tree.layout(1).location.y, 10.0 + if relative { 50.0 } else { 0.0 });
            assert_eq!(tree.layout(2).size.height, 110.0);
            #[cfg(feature = "content_size")]
            assert!(result.scrollable_overflow_rect.bottom > result.block_content_end.unwrap());
        }
    }
}

#[test]
#[cfg(feature = "float_layout")]
fn table_cell_alignment_moves_floats_but_not_absolute_children() {
    for align_floats in [false, true] {
        for mixed in [false, true] {
            for (alignment, offset) in [(AlignContent::CENTER, 35.0), (AlignContent::END, 70.0)] {
                let mut root = TestNode::container(
                    Display::Block,
                    Style {
                        position: Position::Relative,
                        size: Size { width: length(100.0), height: length(120.0) },
                        padding: Rect::length(5.0),
                        border: Rect::length(5.0),
                        align_content: alignment,
                        ..Style::default()
                    },
                    Rect::ZERO,
                );
                root.align_floats = align_floats;
                let child = TestNode::leaf(
                    Style {
                        float: taffy::Float::Left,
                        size: Size { width: length(10.0), height: length(30.0) },
                        ..Style::default()
                    },
                    Size::ZERO,
                );
                let mut tree = TestTree::new(root, child);
                tree.nodes.push(TestNode::leaf(
                    Style { position: Position::Absolute, inset: Rect::length(0.0), ..Style::default() },
                    Size::ZERO,
                ));
                tree.nodes[0].children.push(2);
                if mixed {
                    tree.nodes.push(TestNode::leaf(
                        Style { size: Size { width: length(10.0), height: length(20.0) }, ..Style::default() },
                        Size::ZERO,
                    ));
                    tree.nodes[0].children.push(3);
                }
                let result = output(&mut tree);
                assert_eq!(tree.layout(1).location.y, 10.0 + if align_floats { offset } else { 0.0 });
                assert_eq!(tree.layout(2).location.y, 5.0);
                assert_eq!(tree.layout(2).size.height, 110.0);
                if mixed {
                    assert_eq!(tree.layout(3).location.y, 10.0 + offset);
                }
                let content_end = if align_floats {
                    40.0 + offset
                } else if mixed {
                    30.0 + offset
                } else {
                    40.0
                };
                assert_eq!(result.block_content_end, Some(content_end));
            }
        }
    }
}
