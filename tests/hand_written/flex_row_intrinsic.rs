use taffy::prelude::*;
use taffy::WritingMode as LayoutWritingMode;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

#[test]
fn inflexible_auto_basis_uses_its_max_content_size_in_a_min_content_row() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style { flex_grow: 0.0, flex_shrink: 0.0, ..Default::default() },
            TestNodeContext::ahem_text("aaaaa\u{200b}bbbbb".to_owned(), WritingMode::Horizontal),
        )
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(flex, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(item).unwrap().size.width, 100.0);
}

#[test]
fn inflexible_fixed_basis_uses_its_hypothetical_size_for_intrinsic_rows() {
    let mut tree = TaffyTree::<()>::new();
    let growing_content = tree.new_leaf(Style { size: Size::from_lengths(200.0, 10.0), ..Default::default() }).unwrap();
    let cannot_grow = tree
        .new_with_children(
            Style {
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 1.0,
                min_size: Size::from_lengths(0.0, 0.0),
                ..Default::default()
            },
            &[growing_content],
        )
        .unwrap();
    let cannot_shrink = tree
        .new_leaf(Style {
            flex_basis: length(100.0),
            flex_grow: 1.0,
            flex_shrink: 0.0,
            min_size: Size::from_lengths(0.0, 0.0),
            ..Default::default()
        })
        .unwrap();

    for (item, intrinsic_width) in [(cannot_grow, Dimension::min_content()), (cannot_shrink, Dimension::max_content())]
    {
        let flex = tree
            .new_with_children(
                Style {
                    display: Display::Flex,
                    size: Size { width: intrinsic_width, height: length(100.0) },
                    ..Default::default()
                },
                &[item],
            )
            .unwrap();
        tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(flex).unwrap().size.width, 100.0);
        assert_eq!(tree.layout(item).unwrap().size.width, 100.0);
    }
}

#[test]
fn max_width_border_box_clamps_the_entire_intrinsic_flex_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            flex_basis: length(200.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            max_size: Size { width: length(100.0), height: Dimension::auto() },
            border: Rect { left: length(10.0), right: length(10.0), ..Rect::zero() },
            box_sizing: BoxSizing::BorderBox,
            ..Default::default()
        })
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(item).unwrap().size.width, 100.0);
}

#[test]
fn intrinsic_ratio_uses_the_clamped_cross_size() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: auto(), height: length(200.0) },
            max_size: Size { width: auto(), height: length(50.0) },
            aspect_ratio: Some(2.0),
            flex_grow: 1.0,
            ..Default::default()
        })
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::max_content(), height: length(200.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 50.0 });
}

#[test]
fn wrapped_row_min_content_is_the_largest_item_contribution_and_ignores_main_gap() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree.new_leaf(Style { size: Size::from_lengths(100.0, 10.0), ..Default::default() }).unwrap();
    let large = tree
        .new_with_children(
            Style {
                flex_basis: length(200.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                min_size: Size::from_lengths(0.0, 0.0),
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let small = tree.new_leaf(Style { size: Size::from_lengths(50.0, 10.0), ..Default::default() }).unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                gap: Size { width: length(17.0), height: length(24.0) },
                ..Default::default()
            },
            &[large, small],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size.width, 100.0);
}

fn preferred_contribution_row_width(first_grow: f32, second_grow: f32) -> f32 {
    let mut tree = TaffyTree::<()>::new();
    let first_content = tree.new_leaf(Style { size: Size::from_lengths(100.0, 10.0), ..Default::default() }).unwrap();
    let first = tree
        .new_with_children(
            Style {
                size: Size { width: length(500.0), height: auto() },
                flex_basis: length(200.0),
                flex_grow: first_grow,
                flex_shrink: 1.0,
                ..Default::default()
            },
            &[first_content],
        )
        .unwrap();
    let second_content = tree.new_leaf(Style { size: Size::from_lengths(100.0, 10.0), ..Default::default() }).unwrap();
    let second = tree
        .new_with_children(
            Style {
                size: Size { width: length(200.0), height: auto() },
                flex_basis: length(100.0),
                flex_grow: second_grow,
                flex_shrink: 0.0,
                ..Default::default()
            },
            &[second_content],
        )
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();
    tree.layout(flex).unwrap().size.width
}

#[test]
fn row_intrinsic_size_uses_web_compatible_item_contributions() {
    assert_eq!(preferred_contribution_row_width(0.0, 0.0), 300.0);
    assert_eq!(preferred_contribution_row_width(0.0, 0.1), 400.0);
    assert_eq!(preferred_contribution_row_width(0.1, 0.1), 700.0);
    assert_eq!(preferred_contribution_row_width(1.0, 1.0), 700.0);
}

#[test]
fn row_intrinsic_main_sizing_follows_the_containers_logical_inline_axis() {
    let mut tree = TaffyTree::<()>::new();
    let item_style = Style { size: Size::from_lengths(20.0, 50.0), ..Default::default() };
    let first = tree.new_leaf(item_style.clone()).unwrap();
    let second = tree.new_leaf(item_style).unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                size: Size { width: length(100.0), height: Dimension::max_content() },
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();
    tree.set_writing_mode(flex, LayoutWritingMode::VerticalLr).unwrap();

    tree.compute_layout(flex, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn intrinsic_row_width_is_recomputed_when_an_ancestor_changes_the_block_constraint() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree.new_leaf(Style { aspect_ratio: Some(1.0), ..Default::default() }).unwrap();
    let row = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::max_content(), height: percent(1.0) },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();
    let parent_style = |height| Style {
        display: Display::Block,
        size: Size { width: length(500.0), height: length(height) },
        ..Default::default()
    };
    let parent = tree.new_with_children(parent_style(200.0), &[row]).unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(row).unwrap().size, Size { width: 200.0, height: 200.0 });

    tree.set_style(parent, parent_style(100.0)).unwrap();
    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(row).unwrap().size, Size { width: 100.0, height: 100.0 });
}
