use taffy::prelude::*;
use taffy::{Point, WritingMode};

fn layout_auto_sized_percentage_gap(
    writing_mode: WritingMode,
    flex_direction: FlexDirection,
) -> (Size<f32>, [Point<f32>; 2]) {
    let mut tree = TaffyTree::<()>::new();
    let item_style = Style {
        size: Size { width: length(50.0), height: length(50.0) },
        flex_grow: 0.0,
        flex_shrink: 0.0,
        ..Style::default()
    };
    let first = tree.new_leaf(item_style.clone()).unwrap();
    let second = tree.new_leaf(item_style).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction,
                flex_wrap: FlexWrap::Wrap,
                gap: Size { width: percent(0.1), height: percent(0.2) },
                align_content: Some(AlignContent::START),
                justify_content: Some(JustifyContent::START),
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();
    for node in [container, first, second] {
        tree.set_writing_mode(node, writing_mode).unwrap();
    }

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    (
        tree.layout(container).unwrap().size,
        [tree.layout(first).unwrap().location, tree.layout(second).unwrap().location],
    )
}

#[test]
fn percentage_gaps_use_final_inline_size_but_only_definite_block_size() {
    assert_eq!(
        layout_auto_sized_percentage_gap(WritingMode::HorizontalTb, FlexDirection::Row),
        (Size { width: 100.0, height: 100.0 }, [Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 50.0 }],),
        "a cyclic inline-axis percentage gap resolves before final line collection"
    );
    assert_eq!(
        layout_auto_sized_percentage_gap(WritingMode::HorizontalTb, FlexDirection::Column),
        (Size { width: 50.0, height: 100.0 }, [Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 50.0 }],),
        "a block-axis percentage gap stays zero when the block size is indefinite"
    );
    assert_eq!(
        layout_auto_sized_percentage_gap(WritingMode::VerticalLr, FlexDirection::Row),
        (Size { width: 100.0, height: 100.0 }, [Point { x: 0.0, y: 0.0 }, Point { x: 50.0, y: 0.0 }],),
        "final line collection follows the logical inline axis in vertical writing"
    );
    assert_eq!(
        layout_auto_sized_percentage_gap(WritingMode::VerticalLr, FlexDirection::Column),
        (Size { width: 100.0, height: 50.0 }, [Point { x: 0.0, y: 0.0 }, Point { x: 50.0, y: 0.0 }],),
        "the indefinite block-axis rule follows vertical writing"
    );
}
