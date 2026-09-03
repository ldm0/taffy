use taffy::prelude::*;

fn fixed_size(width: f32, height: f32) -> Size<Dimension> {
    Size { width: Dimension::length(width), height: Dimension::length(height) }
}

fn edge_sizes(horizontal: f32, vertical: f32) -> Rect<LengthPercentage> {
    Rect {
        left: LengthPercentage::length(horizontal),
        right: LengthPercentage::length(horizontal),
        top: LengthPercentage::length(vertical),
        bottom: LengthPercentage::length(vertical),
    }
}

#[test]
fn content_basis_ignores_preferred_main_size() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree.new_leaf(Style { size: fixed_size(80.0, 10.0), ..Style::default() }).unwrap();
    let item = tree
        .new_with_children(
            Style {
                size: fixed_size(10.0, 10.0),
                min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
                flex_basis: Dimension::content(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: fixed_size(200.0, 40.0),
                align_items: Some(AlignItems::FLEX_START),
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 80.0, height: 10.0 });
}

#[test]
fn content_basis_uses_only_an_independently_definite_cross_size_for_aspect_ratio() {
    let mut tree = TaffyTree::<()>::new();
    let plain = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(20.0), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            flex_basis: Dimension::content(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            align_self: Some(AlignSelf::FLEX_START),
            aspect_ratio: Some(2.0),
            ..Style::default()
        })
        .unwrap();
    let explicit_cross = tree
        .new_leaf(Style {
            size: fixed_size(20.0, 30.0),
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            flex_basis: Dimension::content(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            align_self: Some(AlignSelf::FLEX_START),
            aspect_ratio: Some(2.0),
            ..Style::default()
        })
        .unwrap();
    let stretched = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(20.0), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            flex_basis: Dimension::content(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            align_self: Some(AlignSelf::STRETCH),
            aspect_ratio: Some(2.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style { display: Display::Flex, size: fixed_size(300.0, 40.0), ..Style::default() },
            &[plain, explicit_cross, stretched],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(plain).unwrap().size, Size { width: 0.0, height: 0.0 });
    assert_eq!(tree.layout(explicit_cross).unwrap().size, Size { width: 60.0, height: 30.0 });
    assert_eq!(tree.layout(stretched).unwrap().size, Size { width: 80.0, height: 40.0 });
}

#[test]
fn content_basis_aspect_ratio_respects_the_box_sizing_edge() {
    let mut tree = TaffyTree::<()>::new();
    let content_box_start = tree
        .new_leaf(Style {
            size: fixed_size(20.0, 30.0),
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            flex_basis: Dimension::content(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            align_self: Some(AlignSelf::FLEX_START),
            aspect_ratio: Some(2.0),
            padding: edge_sizes(7.0, 5.0),
            border: edge_sizes(3.0, 3.0),
            box_sizing: BoxSizing::ContentBox,
            ..Style::default()
        })
        .unwrap();
    let content_box_stretch = tree
        .new_leaf(Style {
            size: Size { width: Dimension::length(20.0), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            flex_basis: Dimension::content(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            align_self: Some(AlignSelf::STRETCH),
            aspect_ratio: Some(2.0),
            padding: edge_sizes(7.0, 5.0),
            border: edge_sizes(3.0, 3.0),
            box_sizing: BoxSizing::ContentBox,
            ..Style::default()
        })
        .unwrap();
    let border_box_start = tree
        .new_leaf(Style {
            size: fixed_size(20.0, 30.0),
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            flex_basis: Dimension::content(),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            align_self: Some(AlignSelf::FLEX_START),
            aspect_ratio: Some(2.0),
            padding: edge_sizes(7.0, 5.0),
            border: edge_sizes(3.0, 3.0),
            box_sizing: BoxSizing::BorderBox,
            ..Style::default()
        })
        .unwrap();

    for child in [content_box_start, content_box_stretch, border_box_start] {
        let container = tree
            .new_with_children(
                Style { display: Display::Flex, size: fixed_size(300.0, 50.0), ..Style::default() },
                &[child],
            )
            .unwrap();
        tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    }

    assert_eq!(tree.layout(content_box_start).unwrap().size, Size { width: 80.0, height: 46.0 });
    assert_eq!(tree.layout(content_box_stretch).unwrap().size, Size { width: 88.0, height: 50.0 });
    assert_eq!(tree.layout(border_box_start).unwrap().size, Size { width: 60.0, height: 30.0 });
}

struct PercentageFlexItemCase {
    direction: FlexDirection,
    flex_basis: Dimension,
    item_size: Size<Dimension>,
    container_size: Size<Dimension>,
    aspect_ratio: Option<f32>,
    fixed_child_height: Option<f32>,
}

fn percentage_child_height_in_flex_item(case: PercentageFlexItemCase) -> f32 {
    let mut tree = TaffyTree::<()>::new();
    let percentage_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::length(50.0), height: Dimension::percent(1.0) },
            ..Style::default()
        })
        .unwrap();
    let mut children = Vec::new();
    if let Some(height) = case.fixed_child_height {
        children.push(
            tree.new_leaf(Style {
                display: Display::Block,
                size: Size { width: Dimension::length(50.0), height: Dimension::length(height) },
                ..Style::default()
            })
            .unwrap(),
        );
    }
    children.push(percentage_child);
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: case.item_size,
                min_size: Size { width: Dimension::auto(), height: Dimension::length(0.0) },
                flex_basis: case.flex_basis,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                aspect_ratio: case.aspect_ratio,
                ..Style::default()
            },
            &children,
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: case.direction,
                size: case.container_size,
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(percentage_child).unwrap().size.height
}

#[test]
fn content_basis_keeps_an_indefinite_block_main_size_out_of_percentage_resolution() {
    assert_eq!(
        percentage_child_height_in_flex_item(PercentageFlexItemCase {
            direction: FlexDirection::Column,
            flex_basis: Dimension::content(),
            item_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            container_size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
            aspect_ratio: None,
            fixed_child_height: None,
        }),
        0.0,
    );
    assert_eq!(
        percentage_child_height_in_flex_item(PercentageFlexItemCase {
            direction: FlexDirection::Column,
            flex_basis: Dimension::content(),
            item_size: Size { width: Dimension::auto(), height: Dimension::length(200.0) },
            container_size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
            aspect_ratio: None,
            fixed_child_height: Some(50.0),
        }),
        0.0,
    );
}

#[test]
fn definite_flex_item_block_sizes_remain_percentage_resolution_bases() {
    assert_eq!(
        percentage_child_height_in_flex_item(PercentageFlexItemCase {
            direction: FlexDirection::Column,
            flex_basis: Dimension::auto(),
            item_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            container_size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
            aspect_ratio: None,
            fixed_child_height: None,
        }),
        100.0,
    );
    assert_eq!(
        percentage_child_height_in_flex_item(PercentageFlexItemCase {
            direction: FlexDirection::Row,
            flex_basis: Dimension::content(),
            item_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            container_size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
            aspect_ratio: None,
            fixed_child_height: None,
        }),
        100.0,
    );
    assert_eq!(
        percentage_child_height_in_flex_item(PercentageFlexItemCase {
            direction: FlexDirection::Column,
            flex_basis: Dimension::content(),
            item_size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            container_size: fixed_size(200.0, 200.0),
            aspect_ratio: None,
            fixed_child_height: None,
        }),
        200.0,
    );
    assert_eq!(
        percentage_child_height_in_flex_item(PercentageFlexItemCase {
            direction: FlexDirection::Column,
            flex_basis: Dimension::content(),
            item_size: fixed_size(100.0, 200.0),
            container_size: Size { width: Dimension::length(200.0), height: Dimension::auto() },
            aspect_ratio: Some(2.0),
            fixed_child_height: None,
        }),
        200.0,
    );
}
