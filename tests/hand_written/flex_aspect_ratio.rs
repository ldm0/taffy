use taffy::prelude::*;
use taffy::{AbsoluteAxis, Overflow, WritingMode};
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

#[cfg(feature = "float_layout")]
use super::test_tree::{TestNode, TestTree};
#[cfg(feature = "float_layout")]
use taffy::{Float, ResolvedAspectRatio};

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

/// Regression for WPT css/css-flexbox/aspect-ratio-intrinsic-size-003.html.
#[test]
fn auto_flex_basis_uses_a_definite_stretched_cross_size_through_aspect_ratio() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree.new_leaf(Style { aspect_ratio: Some(1.0), ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-026.html.
///
/// A column flex container's intrinsic inline size is the largest ordinary
/// inline contribution of its items. The item's content-derived main size is
/// not a definite inline size and must not be transferred backwards through
/// its preferred ratio while the float computes its shrink-to-fit width.
#[cfg(feature = "float_layout")]
#[test]
fn column_intrinsic_inline_size_does_not_transfer_a_content_main_size() {
    let padding = Rect { left: length(15.0), right: zero(), top: length(10.0), bottom: zero() };

    for ratio_sizing_box in [BoxSizing::BorderBox, BoxSizing::ContentBox] {
        let root = TestNode::container(
            Display::Block,
            Style { size: Size { width: length(800.0), height: auto() }, ..Style::default() },
            Rect::ZERO,
        );
        let flex = TestNode::container(
            Display::Flex,
            Style {
                flex_direction: FlexDirection::Column,
                float: Float::Left,
                size: Size { width: auto(), height: length(1.0) },
                ..Style::default()
            },
            Rect::ZERO,
        );
        let mut item = TestNode::container(
            Display::Block,
            Style {
                box_sizing: BoxSizing::BorderBox,
                min_size: Size { width: length(25.0), height: auto() },
                padding,
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            Rect::ZERO,
        );
        item.resolved_aspect_ratio = ResolvedAspectRatio::new(1.0, ratio_sizing_box);
        let content = TestNode::leaf(
            Style { display: Display::Block, size: Size { width: auto(), height: length(190.0) }, ..Style::default() },
            Size::ZERO,
        );

        let mut tree = TestTree::new(root, flex);
        tree.nodes.push(item);
        tree.nodes.push(content);
        tree.nodes[1].children.push(2);
        tree.nodes[2].children.push(3);
        tree.compute(Size::MAX_CONTENT);

        assert_eq!(tree.layout(1).size, Size { width: 25.0, height: 1.0 }, "{ratio_sizing_box:?}");
        assert_eq!(tree.layout(2).size, Size { width: 25.0, height: 200.0 }, "{ratio_sizing_box:?}");
    }
}

fn layout_flexed_ratio_item(
    mut item_style: Style,
    flex_direction: FlexDirection,
    container_size: Size<Dimension>,
) -> (Size<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    item_style.flex_basis = Dimension::length(0.0);
    item_style.flex_grow = 1.0;
    item_style.flex_shrink = 1.0;
    let item = tree.new_leaf(item_style).unwrap();
    let container = tree
        .new_with_children(
            Style { display: Display::Flex, flex_direction, size: container_size, ..Style::default() },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    (tree.layout(item).unwrap().size, tree.layout(container).unwrap().size)
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-011.html.
#[test]
fn flexed_main_size_recomputes_the_ratio_dependent_cross_size() {
    let row = layout_flexed_ratio_item(
        Style {
            size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        FlexDirection::Row,
        Size { width: Dimension::length(100.0), height: Dimension::auto() },
    );

    assert_eq!(row, (Size { width: 100.0, height: 100.0 }, Size { width: 100.0, height: 100.0 }));
}

#[test]
fn column_intrinsic_inline_size_transfers_only_the_authored_main_size() {
    let column = layout_flexed_ratio_item(
        Style {
            size: Size { width: Dimension::auto(), height: Dimension::length(50.0) },
            min_size: Size { width: Dimension::auto(), height: Dimension::length(0.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        FlexDirection::Column,
        Size { width: Dimension::auto(), height: Dimension::length(100.0) },
    );

    assert_eq!(column, (Size { width: 50.0, height: 100.0 }, Size { width: 50.0, height: 100.0 }));
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-023.html.
#[test]
fn flexed_replaced_main_size_recomputes_the_ratio_dependent_cross_size() {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style {
                size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
                min_size: Size { width: Dimension::auto(), height: Dimension::length(0.0) },
                aspect_ratio: Some(1.0),
                item_is_replaced: true,
                flex_basis: Dimension::length(0.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                ..Style::default()
            },
            TestNodeContext::fixed(20.0, 50.0),
        )
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

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn direct_cross_size_wins_over_the_ratio_after_main_axis_flexing() {
    let sizes = layout_flexed_ratio_item(
        Style {
            size: Size { width: Dimension::length(50.0), height: Dimension::length(30.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        FlexDirection::Row,
        Size { width: Dimension::length(100.0), height: Dimension::auto() },
    );

    assert_eq!(sizes, (Size { width: 100.0, height: 30.0 }, Size { width: 100.0, height: 30.0 }));
}

#[test]
fn flexed_ratio_cross_size_uses_the_ratio_sizing_box() {
    let sizes = layout_flexed_ratio_item(
        Style {
            size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            padding: Rect {
                left: LengthPercentage::length(10.0),
                right: LengthPercentage::length(10.0),
                top: LengthPercentage::length(10.0),
                bottom: LengthPercentage::length(10.0),
            },
            box_sizing: BoxSizing::ContentBox,
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        FlexDirection::Row,
        Size { width: Dimension::length(120.0), height: Dimension::auto() },
    );

    assert_eq!(sizes, (Size { width: 120.0, height: 120.0 }, Size { width: 120.0, height: 120.0 }));
}

#[test]
fn transferred_cross_maximum_clamps_the_recomputed_ratio_size() {
    let sizes = layout_flexed_ratio_item(
        Style {
            size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(0.0), height: Dimension::auto() },
            max_size: Size { width: Dimension::auto(), height: Dimension::length(80.0) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        FlexDirection::Row,
        Size { width: Dimension::length(100.0), height: Dimension::auto() },
    );

    assert_eq!(sizes, (Size { width: 100.0, height: 80.0 }, Size { width: 100.0, height: 80.0 }));
}

fn layout_ratio_flex_item_with_content(
    mut item_style: Style,
    flex_direction: FlexDirection,
    mut content_style: Style,
) -> Size<f32> {
    let mut tree = TaffyTree::<()>::new();
    content_style.display = Display::Block;
    let content = tree.new_leaf(content_style).unwrap();
    item_style.display = Display::Block;
    let item = tree.new_with_children(item_style, &[content]).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction,
                size: Size { width: Dimension::length(300.0), height: Dimension::auto() },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(item).unwrap().size
}

fn layout_ratio_flex_item_with_direction(item_style: Style, flex_direction: FlexDirection) -> Size<f32> {
    layout_ratio_flex_item_with_content(
        item_style,
        flex_direction,
        Style { size: Size { width: Dimension::length(100.0), height: Dimension::auto() }, ..Style::default() },
    )
}

fn layout_ratio_flex_item(item_style: Style) -> Size<f32> {
    layout_ratio_flex_item_with_direction(item_style, FlexDirection::Row)
}

fn ratio_flex_item_style() -> Style {
    Style {
        size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
        aspect_ratio: Some(0.5),
        flex_basis: Dimension::length(0.0),
        ..Style::default()
    }
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-002.html.
#[test]
fn flex_item_ratio_size_does_not_bypass_its_content_based_automatic_minimum() {
    let size = layout_ratio_flex_item(ratio_flex_item_style());

    assert_eq!(size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-004.html.
#[test]
fn column_flex_item_ratio_size_encompasses_its_intrinsic_block_size() {
    let size = layout_ratio_flex_item_with_content(
        Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            aspect_ratio: Some(2.0),
            flex_basis: Dimension::length(0.0),
            ..Style::default()
        },
        FlexDirection::Column,
        Style { size: Size { width: Dimension::auto(), height: Dimension::length(100.0) }, ..Style::default() },
    );

    assert_eq!(size, Size { width: 100.0, height: 100.0 });
}

fn size_in_axis(axis: AbsoluteAxis, value: Dimension) -> Size<Dimension> {
    match axis {
        AbsoluteAxis::Horizontal => Size { width: value, height: Dimension::auto() },
        AbsoluteAxis::Vertical => Size { width: Dimension::auto(), height: value },
    }
}

fn layout_column_ratio_basis(
    writing_mode: WritingMode,
    direction: FlexDirection,
    mut item_style: Style,
) -> (Size<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: size_in_axis(writing_mode.inline_axis(), Dimension::length(100.0)),
            ..Style::default()
        })
        .unwrap();
    item_style.display = Display::Block;
    match writing_mode.block_axis() {
        AbsoluteAxis::Horizontal => item_style.min_size.width = Dimension::length(0.0),
        AbsoluteAxis::Vertical => item_style.min_size.height = Dimension::length(0.0),
    }
    let item = tree.new_with_children(item_style, &[content]).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: direction,
                align_items: Some(AlignItems::FLEX_START),
                ..Style::default()
            },
            &[item],
        )
        .unwrap();
    for node in [content, item, container] {
        tree.set_writing_mode(node, writing_mode).unwrap();
    }

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    (tree.layout(item).unwrap().size, tree.layout(container).unwrap().size)
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-037.html.
#[test]
fn column_flex_base_uses_the_ratio_of_its_max_content_inline_size() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: Dimension::auto(), height: Dimension::length(0.0) },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::FLEX_START),
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn content_block_flex_basis_keywords_use_initial_inline_geometry() {
    for flex_basis in [
        Dimension::auto(),
        Dimension::content(),
        Dimension::min_content(),
        Dimension::max_content(),
        Dimension::fit_content(),
        Dimension::stretch(),
    ] {
        let sizes = layout_column_ratio_basis(
            WritingMode::HorizontalTb,
            FlexDirection::Column,
            Style { aspect_ratio: Some(1.0), flex_basis, ..Style::default() },
        );

        assert_eq!(
            sizes,
            (Size { width: 100.0, height: 100.0 }, Size { width: 100.0, height: 100.0 }),
            "flex-basis={flex_basis:?}"
        );
    }
}

#[test]
fn content_block_flex_basis_preserves_inline_constraints_and_ratio_box_sizing() {
    let padding = Rect {
        left: LengthPercentage::length(10.0),
        right: LengthPercentage::length(10.0),
        top: LengthPercentage::length(10.0),
        bottom: LengthPercentage::length(10.0),
    };
    let content_box = layout_column_ratio_basis(
        WritingMode::HorizontalTb,
        FlexDirection::Column,
        Style { aspect_ratio: Some(2.0), padding, box_sizing: BoxSizing::ContentBox, ..Style::default() },
    );
    let border_box = layout_column_ratio_basis(
        WritingMode::HorizontalTb,
        FlexDirection::Column,
        Style { aspect_ratio: Some(2.0), padding, box_sizing: BoxSizing::BorderBox, ..Style::default() },
    );
    let inline_maximum = layout_column_ratio_basis(
        WritingMode::HorizontalTb,
        FlexDirection::Column,
        Style {
            aspect_ratio: Some(1.0),
            max_size: Size { width: Dimension::length(80.0), height: Dimension::auto() },
            ..Style::default()
        },
    );

    assert_eq!(content_box.0, Size { width: 120.0, height: 70.0 });
    assert_eq!(border_box.0, Size { width: 120.0, height: 60.0 });
    assert_eq!(inline_maximum.0, Size { width: 80.0, height: 80.0 });
}

#[test]
fn content_block_flex_basis_follows_the_items_logical_block_axis() {
    for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalLr, WritingMode::VerticalRl] {
        for direction in [FlexDirection::Column, FlexDirection::ColumnReverse] {
            let sizes = layout_column_ratio_basis(
                writing_mode,
                direction,
                Style { aspect_ratio: Some(2.0), ..Style::default() },
            );
            let expected = if writing_mode.is_horizontal() {
                Size { width: 100.0, height: 50.0 }
            } else {
                Size { width: 200.0, height: 100.0 }
            };

            assert_eq!(sizes.0, expected, "writing-mode={writing_mode:?} direction={direction:?}");
        }
    }
}

#[test]
fn content_flex_basis_ignores_the_authored_block_size_before_ratio_transfer() {
    let sizes = layout_column_ratio_basis(
        WritingMode::HorizontalTb,
        FlexDirection::Column,
        Style {
            size: Size { width: Dimension::auto(), height: Dimension::length(500.0) },
            aspect_ratio: Some(1.0),
            flex_basis: Dimension::content(),
            ..Style::default()
        },
    );

    assert_eq!(sizes.0, Size { width: 100.0, height: 100.0 });
}

#[test]
fn flex_automatic_minimum_uses_the_final_stretched_cross_size() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            display: Display::Block,
            aspect_ratio: Some(0.5),
            flex_basis: Dimension::length(0.0),
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::length(300.0), height: Dimension::length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 50.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/flex-aspect-ratio-039.html.
#[test]
fn transferred_cross_constraints_bound_the_flex_main_size() {
    let size_from_cross_minimum = layout_ratio_flex_item_with_content(
        Style {
            min_size: Size { width: Dimension::length(0.0), height: Dimension::length(50.0) },
            aspect_ratio: Some(2.0),
            ..Style::default()
        },
        FlexDirection::Row,
        Style::default(),
    );
    assert_eq!(size_from_cross_minimum, Size { width: 100.0, height: 50.0 });

    let size_capped_by_cross_maximum = layout_ratio_flex_item_with_content(
        Style {
            max_size: Size { width: Dimension::auto(), height: Dimension::length(50.0) },
            aspect_ratio: Some(2.0),
            ..Style::default()
        },
        FlexDirection::Row,
        Style { size: Size { width: Dimension::length(200.0), height: Dimension::auto() }, ..Style::default() },
    );
    assert_eq!(size_capped_by_cross_maximum, Size { width: 100.0, height: 50.0 });
}

#[test]
fn flex_automatic_minimum_keeps_specified_and_ratio_dependent_suggestions_distinct() {
    let mut specified = ratio_flex_item_style();
    specified.size.width = Dimension::length(40.0);
    let specified_size = layout_ratio_flex_item(specified);
    assert_eq!(specified_size, Size { width: 40.0, height: 100.0 });

    let mut capped = ratio_flex_item_style();
    capped.max_size.width = Dimension::length(80.0);
    let capped_size = layout_ratio_flex_item(capped);
    assert_eq!(capped_size, Size { width: 80.0, height: 100.0 });

    let mut replaced = ratio_flex_item_style();
    replaced.item_is_replaced = true;
    let replaced_size = layout_ratio_flex_item(replaced);
    assert_eq!(replaced_size, Size { width: 50.0, height: 100.0 });
}

fn layout_replaced_flex_item(item_style: Style, mut container_style: Style) -> Size<f32> {
    fn square_replaced_measure(
        known_dimensions: Size<Option<f32>>,
        _available_space: Size<AvailableSpace>,
        _node_id: NodeId,
        _context: Option<&mut TestNodeContext>,
        _style: &Style,
    ) -> Size<f32> {
        match known_dimensions {
            Size { width: Some(width), height: Some(height) } => Size { width, height },
            Size { width: Some(width), height: None } => Size { width, height: width },
            Size { width: None, height: Some(height) } => Size { width: height, height },
            Size { width: None, height: None } => Size { width: 10.0, height: 10.0 },
        }
    }

    let mut tree = new_test_tree();
    let item = tree.new_leaf_with_context(item_style, TestNodeContext::zero()).unwrap();
    container_style.display = Display::Flex;
    let container = tree.new_with_children(container_style, &[item]).unwrap();

    tree.compute_layout_with_measure(container, Size::MAX_CONTENT, square_replaced_measure).unwrap();

    tree.layout(item).unwrap().size
}

/// Regression for WPT css/css-flexbox/flex-aspect-ratio-img-column-011.html.
#[test]
fn replaced_automatic_minimum_uses_only_an_independently_definite_cross_size() {
    let row = layout_replaced_flex_item(
        Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            item_is_replaced: true,
            ..Style::default()
        },
        Style { size: Size { width: Dimension::length(10.0), height: Dimension::auto() }, ..Style::default() },
    );
    let column = layout_replaced_flex_item(
        Style {
            size: Size { width: Dimension::auto(), height: Dimension::length(100.0) },
            aspect_ratio: Some(1.0),
            item_is_replaced: true,
            ..Style::default()
        },
        Style {
            flex_direction: FlexDirection::Column,
            size: Size { width: Dimension::length(50.0), height: Dimension::length(10.0) },
            ..Style::default()
        },
    );

    assert_eq!(row, Size { width: 10.0, height: 10.0 });
    assert_eq!(column, Size { width: 50.0, height: 50.0 });
}

#[test]
fn scrollable_flex_item_uses_zero_automatic_minimum() {
    let mut scroll_container = ratio_flex_item_style();
    scroll_container.overflow.x = Overflow::Hidden;
    let size = layout_ratio_flex_item(scroll_container);

    assert_eq!(size, Size { width: 0.0, height: 100.0 });

    let mut clipped = ratio_flex_item_style();
    clipped.overflow.x = Overflow::Clip;
    let clipped_size = layout_ratio_flex_item(clipped);
    assert_eq!(clipped_size, Size { width: 100.0, height: 100.0 });
}
