use taffy::prelude::*;

/// Regression for WPT css/css-flexbox/aspect-ratio-transferred-max-size.html.
///
/// A max-size transferred into the main axis constrains the hypothetical main
/// size, but it must not clamp the size produced by flexible-length resolution.
#[test]
fn transferred_max_size_does_not_clamp_the_flexed_main_size() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            max_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            aspect_ratio: Some(0.5),
            flex_basis: Dimension::length(0.0),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}
