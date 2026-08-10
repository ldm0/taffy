use taffy::prelude::*;

// CSS Flexbox 1 section 9.5 requires positive main-axis free space to be
// consumed by auto margins before justify-content is applied. This also
// covers the interaction exercised by WPT flexbox-column-row-gap-001.html.
#[test]
fn main_axis_auto_margin_consumes_free_space_before_justification() {
    let mut tree = TaffyTree::<()>::new();
    let leading = tree
        .new_leaf(Style {
            size: Size { width: length(596.0), height: length(45.0) },
            margin: Rect { right: auto(), ..Rect::zero() },
            ..Style::default()
        })
        .unwrap();
    let trailing =
        tree.new_leaf(Style { size: Size { width: length(308.0), height: length(45.0) }, ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(1392.0), height: length(45.0) },
                justify_content: Some(JustifyContent::SPACE_BETWEEN),
                ..Style::default()
            },
            &[leading, trailing],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(leading).unwrap().location.x, 0.0);
    assert_eq!(tree.layout(leading).unwrap().margin.right, 488.0);
    assert_eq!(tree.layout(trailing).unwrap().location.x, 1084.0);
}
