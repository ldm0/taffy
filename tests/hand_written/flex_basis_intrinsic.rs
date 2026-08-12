use taffy::prelude::*;
use taffy::WritingMode;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode as TextWritingMode};

fn layout_flex_basis_with_preferred_main(
    direction: FlexDirection,
    item_writing_mode: WritingMode,
    text_writing_mode: TextWritingMode,
    flex_basis: Dimension,
    preferred_main_size: Dimension,
) -> Size<f32> {
    let mut tree = new_test_tree();
    let size = match direction {
        FlexDirection::Row | FlexDirection::RowReverse => {
            Size { width: preferred_main_size, height: Dimension::auto() }
        }
        FlexDirection::Column | FlexDirection::ColumnReverse => {
            Size { width: Dimension::auto(), height: preferred_main_size }
        }
    };
    let item = tree
        .new_leaf_with_context(
            Style { flex_basis, size, flex_shrink: 0.0, min_size: Size::from_lengths(0.0, 0.0), ..Default::default() },
            TestNodeContext::ahem_text("aaaaa\u{200b}bbbbb".to_owned(), text_writing_mode),
        )
        .unwrap();
    tree.set_writing_mode(item, item_writing_mode).unwrap();
    let flex = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: direction,
                size: Size::from_lengths(75.0, 75.0),
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(flex, Size::MAX_CONTENT, test_measure_function).unwrap();
    tree.layout(item).unwrap().size
}

fn layout_intrinsic_flex_basis(
    direction: FlexDirection,
    item_writing_mode: WritingMode,
    text_writing_mode: TextWritingMode,
    flex_basis: Dimension,
) -> Size<f32> {
    layout_flex_basis_with_preferred_main(
        direction,
        item_writing_mode,
        text_writing_mode,
        flex_basis,
        Dimension::auto(),
    )
}

#[test]
fn intrinsic_flex_basis_keywords_remain_distinct_in_the_main_axis() {
    for (direction, item_writing_mode, text_writing_mode) in [
        (FlexDirection::Row, WritingMode::HorizontalTb, TextWritingMode::Horizontal),
        (FlexDirection::Column, WritingMode::VerticalLr, TextWritingMode::Vertical),
    ] {
        let min_content =
            layout_intrinsic_flex_basis(direction, item_writing_mode, text_writing_mode, Dimension::min_content());
        let max_content =
            layout_intrinsic_flex_basis(direction, item_writing_mode, text_writing_mode, Dimension::max_content());
        let fit_content =
            layout_intrinsic_flex_basis(direction, item_writing_mode, text_writing_mode, Dimension::fit_content());
        let fit_content_length = layout_intrinsic_flex_basis(
            direction,
            item_writing_mode,
            text_writing_mode,
            Dimension::fit_content_function(LengthPercentage::length(60.0)),
        );
        let fit_content_percentage = layout_intrinsic_flex_basis(
            direction,
            item_writing_mode,
            text_writing_mode,
            Dimension::fit_content_function(LengthPercentage::percent(0.8)),
        );
        let content =
            layout_intrinsic_flex_basis(direction, item_writing_mode, text_writing_mode, Dimension::content());

        let main = |size: Size<f32>| match direction {
            FlexDirection::Row | FlexDirection::RowReverse => size.width,
            FlexDirection::Column | FlexDirection::ColumnReverse => size.height,
        };
        assert_eq!(main(min_content), 50.0, "{direction:?} min-content");
        assert_eq!(main(max_content), 100.0, "{direction:?} max-content");
        assert_eq!(main(fit_content), 75.0, "{direction:?} fit-content");
        assert_eq!(main(fit_content_length), 60.0, "{direction:?} fit-content length");
        assert_eq!(main(fit_content_percentage), 60.0, "{direction:?} fit-content percentage");
        assert_eq!(main(content), 100.0, "{direction:?} content");
    }
}

#[test]
fn auto_flex_basis_preserves_an_intrinsic_preferred_main_size() {
    for (direction, item_writing_mode, text_writing_mode) in [
        (FlexDirection::Row, WritingMode::HorizontalTb, TextWritingMode::Horizontal),
        (FlexDirection::Column, WritingMode::VerticalLr, TextWritingMode::Vertical),
    ] {
        let main = |size: Size<f32>| match direction {
            FlexDirection::Row | FlexDirection::RowReverse => size.width,
            FlexDirection::Column | FlexDirection::ColumnReverse => size.height,
        };
        for (preferred_main_size, expected) in
            [(Dimension::min_content(), 50.0), (Dimension::max_content(), 100.0), (Dimension::fit_content(), 75.0)]
        {
            let size = layout_flex_basis_with_preferred_main(
                direction,
                item_writing_mode,
                text_writing_mode,
                Dimension::auto(),
                preferred_main_size,
            );
            assert_eq!(main(size), expected, "{direction:?} {preferred_main_size:?}");
        }
    }
}

#[test]
fn stretch_flex_basis_fills_the_margin_box_and_has_a_content_fallback() {
    let mut tree = new_test_tree();
    let stretched = tree
        .new_leaf_with_context(
            Style {
                flex_basis: Dimension::stretch(),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                margin: Rect { right: length(20.0), ..Rect::zero() },
                border: Rect { left: length(5.0), right: length(5.0), ..Rect::zero() },
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
                min_size: Size { width: auto(), height: length(0.0) },
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
