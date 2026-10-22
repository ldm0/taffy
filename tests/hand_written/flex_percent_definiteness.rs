use taffy::prelude::*;
use taffy::WritingMode;

#[test]
fn min_height_flex_items_keep_definite_inline_axes_for_percentage_descendants() {
    for container_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for item_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
            let mut tree = TaffyTree::<()>::new();
            let descendant = tree
                .new_leaf(Style {
                    display: Display::Block,
                    size: Size { width: auto(), height: percent(1.0) },
                    ..Style::default()
                })
                .unwrap();
            tree.set_writing_mode(descendant, item_mode).unwrap();
            let item = tree
                .new_with_children(Style { display: Display::Block, flex_grow: 1.0, ..Style::default() }, &[descendant])
                .unwrap();
            tree.set_writing_mode(item, item_mode).unwrap();
            let container = tree
                .new_with_children(
                    Style {
                        flex_direction: if container_mode.is_horizontal() {
                            FlexDirection::Column
                        } else {
                            FlexDirection::Row
                        },
                        min_size: Size { width: auto(), height: percent(1.0) },
                        ..Style::default()
                    },
                    &[item],
                )
                .unwrap();
            tree.set_writing_mode(container, container_mode).unwrap();
            let wrapper = tree
                .new_with_children(
                    Style {
                        display: Display::Block,
                        size: Size { width: length(200.0), height: length(100.0) },
                        ..Style::default()
                    },
                    &[container],
                )
                .unwrap();
            tree.compute_layout(wrapper, Size::MAX_CONTENT).unwrap();
            let expected = if container_mode.is_horizontal() && item_mode.is_horizontal() { 0.0 } else { 100.0 };
            assert_eq!(
                tree.layout(descendant).unwrap().size.height,
                expected,
                "container={container_mode:?}, item={item_mode:?}"
            );
        }
    }
}
