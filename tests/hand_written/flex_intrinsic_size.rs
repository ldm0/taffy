use taffy::prelude::*;
#[cfg(feature = "float_layout")]
use taffy::Float;
use taffy::Point;

/// Regression for WPT css/css-flexbox/intrinsic-size/row-007.html.
///
/// The item's hypothetical main size is constrained by `max-width`, so an
/// inflexible basis larger than that maximum must not enlarge the flex
/// container's max-content contribution.
#[test]
fn row_max_content_uses_the_constrained_hypothetical_main_size() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            flex_basis: length(200.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            max_size: Size { width: length(100.0), height: auto() },
            border: Rect::length(10.0),
            box_sizing: BoxSizing::BorderBox,
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(item).unwrap().size.width, 100.0);
}

/// Regression for WPT css/css-flexbox/intrinsic-size/row-002.html.
#[test]
fn row_max_content_preserves_an_inflexible_definite_basis() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style { flex_basis: length(100.0), flex_grow: 1.0, flex_shrink: 0.0, ..Style::default() })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/intrinsic-size/row-003.html.
#[test]
fn row_min_content_preserves_an_inflexible_definite_basis() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style { flex_basis: length(100.0), flex_grow: 1.0, flex_shrink: 0.0, ..Style::default() })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/intrinsic-size/row-004.html.
#[test]
fn row_min_content_uses_the_hypothetical_size_when_an_item_cannot_grow() {
    let mut tree = TaffyTree::<()>::new();
    let intrinsic_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(200.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 1.0,
                min_size: Size { width: length(0.0), height: auto() },
                ..Style::default()
            },
            &[intrinsic_child],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

#[derive(Clone, Copy)]
struct RowIntrinsicItem {
    flex_basis: f32,
    flex_grow: f32,
    flex_shrink: f32,
    width: f32,
    min_width_zero: bool,
}

fn row_min_content_width(items: &[RowIntrinsicItem], flex_wrap: FlexWrap) -> f32 {
    let mut tree = TaffyTree::<()>::new();
    let items = items
        .iter()
        .map(|item| {
            let intrinsic_child = tree
                .new_leaf(Style {
                    display: Display::Block,
                    size: Size { width: length(100.0), height: auto() },
                    ..Style::default()
                })
                .unwrap();
            tree.new_with_children(
                Style {
                    flex_basis: length(item.flex_basis),
                    flex_grow: item.flex_grow,
                    flex_shrink: item.flex_shrink,
                    size: Size { width: length(item.width), height: auto() },
                    min_size: Size { width: if item.min_width_zero { length(0.0) } else { auto() }, height: auto() },
                    ..Style::default()
                },
                &[intrinsic_child],
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_wrap,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Style::default()
            },
            &items,
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(container).unwrap().size.width
}

/// Chromium enables `LayoutFlexNewRowAlgorithm` by default. Its compatibility
/// algorithm deliberately differs from the historical expectations recorded
/// by WPT row-005 for several multi-item combinations. Keep the complete
/// matrix here so later cleanups do not accidentally restore the old flex
/// fraction algorithm for some branches only.
#[test]
fn row_min_content_matches_chromiums_stable_multi_item_algorithm() {
    let cases: &[(&[RowIntrinsicItem], f32)] = &[
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 200.0,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: 50.0,
                    min_width_zero: true,
                },
                RowIntrinsicItem {
                    flex_basis: 400.0,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: 50.0,
                    min_width_zero: false,
                },
            ],
            100.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 200.0,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: 50.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 400.0,
                    flex_grow: 1.0,
                    flex_shrink: 2.0,
                    width: 50.0,
                    min_width_zero: false,
                },
            ],
            100.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 200.0,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: 50.0,
                    min_width_zero: true,
                },
                RowIntrinsicItem {
                    flex_basis: 400.0,
                    flex_grow: 1.0,
                    flex_shrink: 2.0,
                    width: 50.0,
                    min_width_zero: true,
                },
            ],
            100.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 200.0,
                    flex_grow: 1.0,
                    flex_shrink: 0.0,
                    width: 50.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 400.0,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: 50.0,
                    min_width_zero: false,
                },
            ],
            250.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 50.0,
                    flex_grow: 0.0,
                    flex_shrink: 0.0,
                    width: 200.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 50.0,
                    flex_grow: 0.0,
                    flex_shrink: 0.0,
                    width: 200.0,
                    min_width_zero: false,
                },
            ],
            200.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 50.0,
                    flex_grow: 1.0,
                    flex_shrink: 0.0,
                    width: 200.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 100.0,
                    flex_grow: 2.0,
                    flex_shrink: 0.0,
                    width: 200.0,
                    min_width_zero: false,
                },
            ],
            400.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 200.0,
                    flex_grow: 0.0,
                    flex_shrink: 1.0,
                    width: 50.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 100.0,
                    flex_grow: 2.0,
                    flex_shrink: 0.0,
                    width: 200.0,
                    min_width_zero: false,
                },
            ],
            250.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 100.0,
                    flex_grow: 0.0,
                    flex_shrink: 1.0,
                    width: 250.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 200.0,
                    flex_grow: 1.0,
                    flex_shrink: 0.0,
                    width: 100.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 300.0,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: 300.0,
                    min_width_zero: false,
                },
            ],
            600.0,
        ),
        (
            &[
                RowIntrinsicItem {
                    flex_basis: 300.0,
                    flex_grow: 0.0,
                    flex_shrink: 10.0,
                    width: 200.0,
                    min_width_zero: false,
                },
                RowIntrinsicItem {
                    flex_basis: 1000.0,
                    flex_grow: 0.0,
                    flex_shrink: 1.0,
                    width: 500.0,
                    min_width_zero: false,
                },
            ],
            700.0,
        ),
    ];

    for (index, (items, expected)) in cases.iter().enumerate() {
        assert_eq!(row_min_content_width(items, FlexWrap::NoWrap), *expected, "row-005 case {}", index + 1);
    }
}

/// Compatibility coverage for the first case in WPT
/// css/css-flexbox/intrinsic-size/row-wrap-001.html.
///
/// A wrapped row takes every min-content wrapping opportunity, so its
/// contribution is the largest ordinary item contribution. A definite flex
/// basis must not replace that contribution in this multi-line branch. The
/// historical WPT expectation is 100px, while Chromium's stable
/// `LayoutFlexNewRowAlgorithm` produces the 50px ordinary contribution.
#[test]
fn row_wrap_min_content_uses_the_largest_ordinary_item_contribution() {
    let items = [
        RowIntrinsicItem { flex_basis: 200.0, flex_grow: 1.0, flex_shrink: 1.0, width: 50.0, min_width_zero: true },
        RowIntrinsicItem { flex_basis: 400.0, flex_grow: 1.0, flex_shrink: 1.0, width: 50.0, min_width_zero: false },
    ];

    assert_eq!(row_min_content_width(&items, FlexWrap::Wrap), 50.0);
}

#[cfg(feature = "float_layout")]
#[derive(Clone, Copy)]
struct RowWrapItem {
    flex_basis: f32,
    flex_grow: f32,
    flex_shrink: f32,
    width: f32,
    margin_left: f32,
    has_intrinsic_child: bool,
}

#[cfg(feature = "float_layout")]
fn row_wrap_shrink_to_fit_width(items: &[RowWrapItem], column_gap: f32) -> f32 {
    let mut tree = TaffyTree::<()>::new();
    let items = items
        .iter()
        .map(|item| {
            let style = Style {
                flex_basis: length(item.flex_basis),
                flex_grow: item.flex_grow,
                flex_shrink: item.flex_shrink,
                size: Size { width: length(item.width), height: auto() },
                margin: Rect { left: length(item.margin_left), ..Rect::zero() },
                ..Style::default()
            };
            if item.has_intrinsic_child {
                let child = tree
                    .new_leaf(Style {
                        display: Display::Block,
                        size: Size { width: length(100.0), height: auto() },
                        ..Style::default()
                    })
                    .unwrap();
                tree.new_with_children(style, &[child]).unwrap()
            } else {
                tree.new_leaf(style).unwrap()
            }
        })
        .collect::<Vec<_>>();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_wrap: FlexWrap::Wrap,
                float: Float::Left,
                size: Size { width: auto(), height: length(100.0) },
                gap: Size { width: length(column_gap), height: length(0.0) },
                ..Style::default()
            },
            &items,
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: length(0.0), height: auto() }, ..Style::default() },
            &[flex],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    tree.layout(flex).unwrap().size.width
}

/// Current Chromium stable behavior for all five cases in WPT
/// css/css-flexbox/intrinsic-size/row-wrap-001.html.
///
/// The WPT file still records historical expectations for four cases. This
/// matrix follows Chromium's `LayoutFlexNewRowAlgorithm`, including the rule
/// that a max-content result cannot be smaller than its min-content result
/// when negative margins make the raw sums cross.
#[cfg(feature = "float_layout")]
#[test]
fn row_wrap_shrink_to_fit_matches_chromiums_stable_matrix() {
    let item = |flex_basis, flex_grow, flex_shrink, width, margin_left, has_intrinsic_child| RowWrapItem {
        flex_basis,
        flex_grow,
        flex_shrink,
        width,
        margin_left,
        has_intrinsic_child,
    };
    let cases = [
        (vec![item(200.0, 1.0, 1.0, 50.0, 0.0, true), item(400.0, 1.0, 1.0, 50.0, 0.0, false)], 0.0, 50.0),
        (vec![item(50.0, 0.0, 0.0, 200.0, 0.0, true), item(50.0, 0.0, 0.0, 50.0, 0.0, true)], 0.0, 200.0),
        (vec![item(50.0, 1.0, 0.0, 200.0, 0.0, true), item(150.0, 2.0, 0.0, 50.0, 0.0, true)], 0.0, 200.0),
        (vec![item(50.0, 1.0, 0.0, 200.0, 0.0, true), item(150.0, 2.0, 0.0, 50.0, 300.0, true)], 0.0, 350.0),
        (vec![item(50.0, 1.0, 0.0, 200.0, 0.0, true), item(150.0, 2.0, 0.0, 50.0, 300.0, true)], 17.0, 350.0),
    ];

    for (index, (items, column_gap, expected)) in cases.iter().enumerate() {
        assert_eq!(row_wrap_shrink_to_fit_width(items, *column_gap), *expected, "row-wrap-001 case {}", index + 1);
    }
}

/// Regression for WPT css/css-flexbox/intrinsic-size/row-006.html.
#[test]
fn row_max_content_hypothetical_size_includes_item_border() {
    let mut tree = TaffyTree::<()>::new();
    let intrinsic_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(80.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                flex_basis: length(0.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                border: Rect::length(10.0),
                ..Style::default()
            },
            &[intrinsic_child],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/intrinsic-size/col-wrap-002.html.
///
/// A max block-size provides the line length for a wrapped column even while
/// its inline size is being measured under a max-content constraint.
#[test]
fn column_wrap_max_content_sums_columns_formed_by_a_max_block_size() {
    let mut tree = TaffyTree::<()>::new();
    let item_style = Style {
        min_size: Size { width: auto(), height: length(0.0) },
        size: Size { width: length(50.0), height: auto() },
        flex_basis: length(100.0),
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
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: auto() },
                max_size: Size { width: auto(), height: length(100.0) },
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(first).unwrap().location, Point { x: 0.0, y: 0.0 });
    assert_eq!(tree.layout(second).unwrap().location, Point { x: 50.0, y: 0.0 });
}

/// Regression for WPT css/css-flexbox/intrinsic-size/col-wrap-003.html.
#[test]
fn column_wrap_intrinsic_cross_size_includes_container_decoration_once() {
    let mut tree = TaffyTree::<()>::new();
    let item_style = Style {
        min_size: Size { width: auto(), height: length(0.0) },
        size: Size { width: length(40.0), height: auto() },
        flex_basis: length(100.0),
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
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: auto() },
                max_size: Size { width: auto(), height: length(100.0) },
                padding: Rect { left: length(5.0), ..Rect::zero() },
                border: Rect { left: length(9.0), right: length(6.0), ..Rect::zero() },
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(first).unwrap().location, Point { x: 14.0, y: 0.0 });
    assert_eq!(tree.layout(second).unwrap().location, Point { x: 54.0, y: 0.0 });
}

/// The min-content inline size of a wrapped column is the largest outer item
/// contribution, not the sum used by its max-content layout.
#[test]
fn column_wrap_min_content_uses_the_largest_item_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let first = tree
        .new_leaf(Style {
            min_size: Size { width: auto(), height: length(0.0) },
            size: Size { width: length(75.0), height: auto() },
            flex_basis: length(100.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let second = tree
        .new_leaf(Style {
            min_size: Size { width: auto(), height: length(0.0) },
            size: Size { width: length(25.0), height: auto() },
            flex_basis: length(100.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 75.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/intrinsic-size/col-wrap-013.html.
#[test]
fn column_wrap_max_content_keeps_each_columns_own_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let first_content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(75.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let second_content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(25.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let item_style = Style {
        min_size: Size { width: auto(), height: length(0.0) },
        flex_basis: length(100.0),
        flex_grow: 0.0,
        flex_shrink: 0.0,
        ..Style::default()
    };
    let first = tree.new_with_children(item_style.clone(), &[first_content]).unwrap();
    let second = tree.new_with_children(item_style, &[second_content]).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(second).unwrap().location.x, 75.0);
}

/// Regression for WPT css/css-flexbox/intrinsic-size/col-wrap-014.html.
#[test]
fn column_wrap_max_content_applies_each_items_maximum() {
    let mut tree = TaffyTree::<()>::new();
    let first_content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(75.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let second_content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(50.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let first = tree
        .new_with_children(
            Style {
                min_size: Size { width: auto(), height: length(0.0) },
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Style::default()
            },
            &[first_content],
        )
        .unwrap();
    let second = tree
        .new_with_children(
            Style {
                min_size: Size { width: auto(), height: length(0.0) },
                max_size: Size { width: length(25.0), height: auto() },
                flex_basis: length(100.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Style::default()
            },
            &[second_content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(second).unwrap().size.width, 25.0);
}

/// Regression for WPT css/css-flexbox/intrinsic-size/col-wrap-020.html.
#[test]
fn column_wrap_max_content_includes_the_cross_axis_gap() {
    let mut tree = TaffyTree::<()>::new();
    let item_style = Style {
        min_size: Size { width: auto(), height: length(0.0) },
        size: Size { width: length(10.0), height: auto() },
        flex_basis: length(100.0),
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
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                gap: Size { width: length(80.0), height: length(0.0) },
                ..Style::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(second).unwrap().location.x, 90.0);
}

/// Regression for WPT css/css-flexbox/intrinsic-size/row-001.html.
#[cfg(feature = "float_layout")]
#[test]
fn inflexible_auto_basis_preserves_its_min_content_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let float_style = Style {
        display: Display::Block,
        float: Float::Left,
        size: Size { width: length(100.0), height: auto() },
        ..Style::default()
    };
    let first_float = tree.new_leaf(float_style.clone()).unwrap();
    let second_float = tree.new_leaf(float_style).unwrap();
    let item = tree
        .new_with_children(
            Style { display: Display::Block, flex_grow: 0.0, flex_shrink: 0.0, flex_basis: auto(), ..Style::default() },
            &[first_float, second_float],
        )
        .unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                float: Float::Left,
                size: Size { width: auto(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: length(0.0), height: auto() }, ..Style::default() },
            &[flex],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(flex).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-flexbox/intrinsic-size/col-wrap-017.html.
#[cfg(feature = "float_layout")]
#[test]
fn column_wrap_uses_a_ratio_items_max_content_cross_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let float_style =
        Style { display: Display::Block, float: Float::Left, size: Size::from_lengths(50.0, 50.0), ..Style::default() };
    let first_float = tree.new_leaf(float_style.clone()).unwrap();
    let second_float = tree.new_leaf(float_style).unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: percent(1.0), height: auto() },
                min_size: Size { width: auto(), height: length(0.0) },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            &[first_float, second_float],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                size: Size { width: Dimension::max_content(), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size, Size { width: 100.0, height: 100.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}
