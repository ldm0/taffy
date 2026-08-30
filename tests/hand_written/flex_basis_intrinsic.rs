use taffy::prelude::*;
use taffy::{AbsoluteAxis, WritingMode};
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode as TextWritingMode};

const FLEX_DIRECTIONS: [FlexDirection; 4] =
    [FlexDirection::Row, FlexDirection::RowReverse, FlexDirection::Column, FlexDirection::ColumnReverse];
const WRITING_MODES: [WritingMode; 3] = [WritingMode::HorizontalTb, WritingMode::VerticalLr, WritingMode::VerticalRl];

fn physical_main_axis(container_writing_mode: WritingMode, direction: FlexDirection) -> AbsoluteAxis {
    match direction {
        FlexDirection::Row | FlexDirection::RowReverse => container_writing_mode.inline_axis(),
        FlexDirection::Column | FlexDirection::ColumnReverse => container_writing_mode.block_axis(),
    }
}

fn text_writing_mode(writing_mode: WritingMode) -> TextWritingMode {
    if writing_mode.is_horizontal() {
        TextWritingMode::Horizontal
    } else {
        TextWritingMode::Vertical
    }
}

fn main_dimension(axis: AbsoluteAxis, value: Dimension) -> Size<Dimension> {
    match axis {
        AbsoluteAxis::Horizontal => Size { width: value, height: Dimension::auto() },
        AbsoluteAxis::Vertical => Size { width: Dimension::auto(), height: value },
    }
}

fn main_size(axis: AbsoluteAxis, size: Size<f32>) -> f32 {
    size.get_abs(axis)
}

#[derive(Clone, Copy)]
struct TextFlexBasisCase {
    container_writing_mode: WritingMode,
    direction: FlexDirection,
    item_writing_mode: WritingMode,
    flex_basis: Dimension,
    preferred_main_size: Dimension,
    container_size: Size<Dimension>,
    margin: Rect<LengthPercentageAuto>,
    padding: Rect<LengthPercentage>,
    border: Rect<LengthPercentage>,
    box_sizing: BoxSizing,
}

impl TextFlexBasisCase {
    fn basic(
        container_writing_mode: WritingMode,
        direction: FlexDirection,
        item_writing_mode: WritingMode,
        flex_basis: Dimension,
        preferred_main_size: Dimension,
    ) -> Self {
        Self {
            container_writing_mode,
            direction,
            item_writing_mode,
            flex_basis,
            preferred_main_size,
            container_size: Size::from_lengths(75.0, 75.0),
            margin: Rect::zero(),
            padding: Rect::zero(),
            border: Rect::zero(),
            box_sizing: BoxSizing::ContentBox,
        }
    }
}

fn layout_text_flex_basis(case: TextFlexBasisCase) -> f32 {
    let TextFlexBasisCase {
        container_writing_mode,
        direction,
        item_writing_mode,
        flex_basis,
        preferred_main_size,
        container_size,
        margin,
        padding,
        border,
        box_sizing,
    } = case;
    let mut tree = new_test_tree();
    let axis = physical_main_axis(container_writing_mode, direction);
    let item = tree
        .new_leaf_with_context(
            Style {
                size: main_dimension(axis, preferred_main_size),
                min_size: Size::from_lengths(0.0, 0.0),
                flex_basis,
                flex_grow: 0.0,
                flex_shrink: 0.0,
                margin,
                padding,
                border,
                box_sizing,
                ..Default::default()
            },
            TestNodeContext::ahem_text("aaaaa\u{200b}bbbbb".to_owned(), text_writing_mode(item_writing_mode)),
        )
        .unwrap();
    tree.set_writing_mode(item, item_writing_mode).unwrap();
    let flex = tree
        .new_with_children(
            Style { display: Display::Flex, flex_direction: direction, size: container_size, ..Default::default() },
            &[item],
        )
        .unwrap();
    tree.set_writing_mode(flex, container_writing_mode).unwrap();

    tree.compute_layout_with_measure(flex, Size::MAX_CONTENT, test_measure_function).unwrap();
    main_size(axis, tree.layout(item).unwrap().size)
}

fn layout_simple_text_flex_basis(
    container_writing_mode: WritingMode,
    direction: FlexDirection,
    item_writing_mode: WritingMode,
    flex_basis: Dimension,
    preferred_main_size: Dimension,
) -> f32 {
    layout_text_flex_basis(TextFlexBasisCase::basic(
        container_writing_mode,
        direction,
        item_writing_mode,
        flex_basis,
        preferred_main_size,
    ))
}

fn expected_intrinsic_basis(
    container_writing_mode: WritingMode,
    direction: FlexDirection,
    item_writing_mode: WritingMode,
    value: Dimension,
) -> f32 {
    let main_axis = physical_main_axis(container_writing_mode, direction);
    if main_axis != item_writing_mode.inline_axis() {
        return 20.0;
    }
    if value.is_min_content() {
        50.0
    } else if value.is_max_content() {
        100.0
    } else {
        75.0
    }
}

#[test]
fn intrinsic_flex_basis_keywords_remain_distinct_across_logical_axes() {
    for container_writing_mode in WRITING_MODES {
        for direction in FLEX_DIRECTIONS {
            for item_writing_mode in WRITING_MODES {
                for value in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
                    let actual = layout_simple_text_flex_basis(
                        container_writing_mode,
                        direction,
                        item_writing_mode,
                        value,
                        Dimension::auto(),
                    );
                    let expected =
                        expected_intrinsic_basis(container_writing_mode, direction, item_writing_mode, value);
                    assert_eq!(
                        actual, expected,
                        "container={container_writing_mode:?} direction={direction:?} item={item_writing_mode:?} basis={value:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn intrinsic_flex_basis_ignores_the_authored_preferred_main_size() {
    for container_writing_mode in WRITING_MODES {
        for direction in FLEX_DIRECTIONS {
            for item_writing_mode in WRITING_MODES {
                let actual = layout_simple_text_flex_basis(
                    container_writing_mode,
                    direction,
                    item_writing_mode,
                    Dimension::max_content(),
                    Dimension::length(300.0),
                );
                let expected = expected_intrinsic_basis(
                    container_writing_mode,
                    direction,
                    item_writing_mode,
                    Dimension::max_content(),
                );
                assert_eq!(
                    actual, expected,
                    "container={container_writing_mode:?} direction={direction:?} item={item_writing_mode:?}"
                );
            }
        }
    }
}

#[test]
fn auto_flex_basis_preserves_an_intrinsic_preferred_main_size() {
    for container_writing_mode in WRITING_MODES {
        for direction in FLEX_DIRECTIONS {
            for item_writing_mode in WRITING_MODES {
                for preferred_main_size in
                    [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()]
                {
                    let actual = layout_simple_text_flex_basis(
                        container_writing_mode,
                        direction,
                        item_writing_mode,
                        Dimension::auto(),
                        preferred_main_size,
                    );
                    let expected = expected_intrinsic_basis(
                        container_writing_mode,
                        direction,
                        item_writing_mode,
                        preferred_main_size,
                    );
                    assert_eq!(
                        actual, expected,
                        "container={container_writing_mode:?} direction={direction:?} item={item_writing_mode:?} preferred={preferred_main_size:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn fit_content_flex_basis_uses_available_space_after_main_axis_margins() {
    let actual = layout_text_flex_basis(TextFlexBasisCase {
        container_size: Size::from_lengths(100.0, 75.0),
        margin: Rect { left: length(10.0), right: length(20.0), top: zero(), bottom: zero() },
        ..TextFlexBasisCase::basic(
            WritingMode::HorizontalTb,
            FlexDirection::Row,
            WritingMode::HorizontalTb,
            Dimension::fit_content(),
            Dimension::auto(),
        )
    });

    assert_eq!(actual, 70.0);
}

#[test]
fn intrinsic_flex_basis_includes_padding_and_border_for_both_box_sizing_modes() {
    let padding = Rect { left: length(10.0), right: length(10.0), top: zero(), bottom: zero() };
    let border = Rect { left: length(5.0), right: length(5.0), top: zero(), bottom: zero() };

    for box_sizing in [BoxSizing::ContentBox, BoxSizing::BorderBox] {
        for (basis, expected) in
            [(Dimension::min_content(), 80.0), (Dimension::max_content(), 130.0), (Dimension::fit_content(), 100.0)]
        {
            let actual = layout_text_flex_basis(TextFlexBasisCase {
                container_size: Size::from_lengths(100.0, 75.0),
                padding,
                border,
                box_sizing,
                ..TextFlexBasisCase::basic(
                    WritingMode::HorizontalTb,
                    FlexDirection::Row,
                    WritingMode::HorizontalTb,
                    basis,
                    Dimension::auto(),
                )
            });
            assert_eq!(actual, expected, "box_sizing={box_sizing:?} basis={basis:?}");
        }
    }
}

#[test]
fn intrinsic_flex_basis_is_clamped_only_by_the_hypothetical_main_size() {
    let mut tree = new_test_tree();
    let min_clamped = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::min_content(),
                min_size: Size { width: length(80.0), height: length(0.0) },
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            TestNodeContext::ahem_text("aaaaa\u{200b}bbbbb".to_owned(), TextWritingMode::Horizontal),
        )
        .unwrap();
    let max_clamped = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::max_content(),
                min_size: Size::from_lengths(0.0, 0.0),
                max_size: Size { width: length(90.0), height: auto() },
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            TestNodeContext::ahem_text("aaaaa\u{200b}bbbbb".to_owned(), TextWritingMode::Horizontal),
        )
        .unwrap();
    let flex = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size::from_lengths(200.0, 75.0), ..Default::default() },
            &[min_clamped, max_clamped],
        )
        .unwrap();

    tree.compute_layout_with_measure(flex, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(min_clamped).unwrap().size.width, 80.0);
    assert_eq!(tree.layout(max_clamped).unwrap().size.width, 90.0);
}

#[test]
fn stretch_flex_basis_fills_a_definite_margin_box_and_falls_back_to_content() {
    let mut tree = new_test_tree();
    let stretched = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::stretch(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                margin: Rect { right: length(20.0), ..Rect::zero() },
                ..Default::default()
            },
            TestNodeContext::fixed(30.0, 50.0),
        )
        .unwrap();
    let definite = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size { width: length(100.0), height: auto() }, ..Default::default() },
            &[stretched],
        )
        .unwrap();

    tree.compute_layout_with_measure(definite, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(stretched).unwrap().size.width, 80.0);

    let fallback = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::stretch(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                min_size: Size::from_lengths(0.0, 0.0),
                ..Default::default()
            },
            TestNodeContext::fixed(30.0, 50.0),
        )
        .unwrap();
    let indefinite = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[fallback],
        )
        .unwrap();

    tree.compute_layout_with_measure(indefinite, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(fallback).unwrap().size.height, 50.0);
}

#[test]
fn percentage_flex_basis_keeps_definite_and_content_fallback_semantics() {
    let mut tree = new_test_tree();
    let definite_item = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::percent(0.5),
                min_size: Size::from_lengths(0.0, 0.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            TestNodeContext::fixed(30.0, 10.0),
        )
        .unwrap();
    let definite = tree
        .new_with_children(
            Style { display: Display::Flex, size: Size::from_lengths(200.0, 20.0), ..Default::default() },
            &[definite_item],
        )
        .unwrap();
    tree.compute_layout_with_measure(definite, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(definite_item).unwrap().size.width, 100.0);

    let fallback_item = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::percent(0.5),
                min_size: Size::from_lengths(0.0, 0.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..Default::default()
            },
            TestNodeContext::fixed(30.0, 10.0),
        )
        .unwrap();
    let indefinite =
        tree.new_with_children(Style { display: Display::Flex, ..Default::default() }, &[fallback_item]).unwrap();
    tree.compute_layout_with_measure(indefinite, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(fallback_item).unwrap().size.width, 30.0);
}
