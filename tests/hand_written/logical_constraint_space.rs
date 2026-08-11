use taffy::prelude::*;
use taffy::WritingMode;

fn child_padding_in_vertical_container(display: Display) -> f32 {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            writing_mode: WritingMode::HorizontalTb,
            size: Size { width: length(40.0), height: length(40.0) },
            padding: Rect { left: percent(0.1), ..Rect::zero() },
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display,
                writing_mode: WritingMode::VerticalLr,
                size: Size { width: length(100.0), height: length(200.0) },
                ..Style::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(child).unwrap().padding.left
}

#[test]
fn formatting_contexts_share_the_containing_blocks_logical_inline_percentage_basis() {
    assert_eq!(child_padding_in_vertical_container(Display::Block), 20.0);
    assert_eq!(child_padding_in_vertical_container(Display::Flex), 20.0);
    assert_eq!(child_padding_in_vertical_container(Display::Grid), 20.0);
}

#[test]
fn vertical_root_box_percentages_use_the_initial_containing_blocks_inline_size() {
    let mut tree = TaffyTree::<()>::new();
    let root = tree
        .new_leaf(Style {
            writing_mode: WritingMode::VerticalLr,
            size: Size { width: length(100.0), height: length(200.0) },
            padding: Rect { left: percent(0.1), ..Rect::zero() },
            ..Style::default()
        })
        .unwrap();

    tree.compute_layout(root, Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(200.0) })
        .unwrap();

    assert_eq!(tree.layout(root).unwrap().padding.left, 20.0);
}
