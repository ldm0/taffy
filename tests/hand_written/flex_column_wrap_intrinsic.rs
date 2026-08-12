use taffy::prelude::*;
use taffy::{Point, WritingMode};
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode as TextWritingMode};

fn fixed_basis_item(tree: &mut TaffyTree<()>, width: f32, basis: f32) -> NodeId {
    tree.new_leaf(Style {
        size: Size { width: length(width), height: auto() },
        min_size: Size { width: auto(), height: length(0.0) },
        flex_basis: length(basis),
        flex_grow: 0.0,
        flex_shrink: 0.0,
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn max_content_width_accounts_for_every_column_under_a_definite_height() {
    let mut tree = TaffyTree::<()>::new();
    let first = fixed_basis_item(&mut tree, 50.0, 100.0);
    let second = fixed_basis_item(&mut tree, 50.0, 100.0);
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(first).unwrap().location, Point { x: 0.0, y: 0.0 });
    assert_eq!(tree.layout(second).unwrap().location, Point { x: 50.0, y: 0.0 });
}

#[test]
fn max_content_width_uses_a_definite_max_height_for_line_breaking() {
    let mut tree = TaffyTree::<()>::new();
    let first = fixed_basis_item(&mut tree, 50.0, 100.0);
    let second = fixed_basis_item(&mut tree, 50.0, 100.0);
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: auto() },
                max_size: Size { width: Dimension::auto(), height: length(100.0) },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn intrinsic_width_includes_cross_axis_decoration_and_column_gaps() {
    let mut tree = TaffyTree::<()>::new();
    let first = fixed_basis_item(&mut tree, 40.0, 100.0);
    let second = fixed_basis_item(&mut tree, 40.0, 100.0);
    let decorated = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                padding: Rect { left: length(5.0), ..Rect::zero() },
                border: Rect { left: length(9.0), right: length(6.0), ..Rect::zero() },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    let third = fixed_basis_item(&mut tree, 10.0, 100.0);
    let fourth = fixed_basis_item(&mut tree, 10.0, 100.0);
    let gapped = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                gap: Size { width: length(80.0), height: length(0.0) },
                ..Default::default()
            },
            &[third, fourth],
        )
        .unwrap();

    tree.compute_layout(decorated, Size::MAX_CONTENT).unwrap();
    tree.compute_layout(gapped, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(decorated).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(gapped).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn min_content_width_is_the_largest_item_contribution_not_the_sum_of_columns() {
    let mut tree = TaffyTree::<()>::new();
    let first = fixed_basis_item(&mut tree, 100.0, 100.0);
    let second = fixed_basis_item(&mut tree, 100.0, 100.0);
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn max_content_width_uses_the_widest_item_in_each_formed_column() {
    let mut tree = TaffyTree::<()>::new();
    let first = fixed_basis_item(&mut tree, 30.0, 50.0);
    let second = fixed_basis_item(&mut tree, 70.0, 50.0);
    let third = fixed_basis_item(&mut tree, 20.0, 100.0);
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Default::default()
            },
            &[first, second, third],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 90.0, height: 100.0 });
    assert_eq!(tree.layout(first).unwrap().location.x, 0.0);
    assert_eq!(tree.layout(second).unwrap().location.x, 0.0);
    assert_eq!(tree.layout(third).unwrap().location.x, 70.0);
}

#[test]
fn max_content_column_width_gives_fit_content_items_their_full_contribution() {
    let mut tree = new_test_tree();
    let first = tree
        .new_leaf(Style {
            size: Size { width: length(100.0), height: auto() },
            min_size: Size { width: auto(), height: length(0.0) },
            flex_basis: length(100.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            ..Default::default()
        })
        .unwrap();
    let second = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::fit_content(), height: auto() },
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            TestNodeContext::ahem_text("aaaaaaa\u{200b}bbbbbbbb".to_owned(), TextWritingMode::Horizontal),
        )
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout_with_measure(flex, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(second).unwrap().size.width, 150.0);
    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 250.0, height: 100.0 });
}

#[test]
fn cyclic_percentage_width_does_not_inflate_the_max_content_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let definite = fixed_basis_item(&mut tree, 100.0, 100.0);
    let percentage = tree
        .new_leaf(Style {
            size: Size { width: percent(1.0), height: auto() },
            min_size: Size { width: auto(), height: length(0.0) },
            flex_basis: length(100.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            ..Default::default()
        })
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Default::default()
            },
            &[definite, percentage],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(percentage).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(percentage).unwrap().location.x, 100.0);
}

#[test]
fn intrinsic_width_is_recomputed_when_an_ancestor_changes_the_block_constraint() {
    let mut tree = TaffyTree::<()>::new();
    let first = fixed_basis_item(&mut tree, 50.0, 100.0);
    let second = fixed_basis_item(&mut tree, 50.0, 100.0);
    let wrapped = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: auto() },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();
    let parent_style = |height| Style {
        display: Display::Flex,
        size: Size { width: length(500.0), height: length(height) },
        ..Default::default()
    };
    let parent = tree.new_with_children(parent_style(200.0), &[wrapped]).unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(wrapped).unwrap().size, Size { width: 50.0, height: 200.0 });

    tree.set_style(parent, parent_style(100.0)).unwrap();
    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(wrapped).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn column_wrap_intrinsic_sizing_follows_logical_axes_in_vertical_writing() {
    let mut tree = TaffyTree::<()>::new();
    let item_style = Style {
        size: Size { width: auto(), height: length(50.0) },
        min_size: Size { width: length(0.0), height: auto() },
        flex_basis: length(100.0),
        flex_grow: 0.0,
        flex_shrink: 0.0,
        ..Default::default()
    };
    let first = tree.new_leaf(item_style.clone()).unwrap();
    let second = tree.new_leaf(item_style).unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: length(100.0), height: Dimension::max_content() },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();
    tree.set_writing_mode(flex, WritingMode::VerticalLr).unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
}
