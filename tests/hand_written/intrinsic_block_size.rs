use taffy::prelude::*;
use taffy::WritingMode;
use taffy_test_helpers::{new_test_tree, test_measure_function};

fn layout_subject(subject_style: Style, writing_mode: WritingMode) -> Size<f32> {
    let mut tree = new_test_tree();
    let content = tree
        .new_leaf(Style { size: Size::from_lengths(100.0, 100.0), flex_shrink: 0.0, ..Default::default() })
        .unwrap();
    let subject = tree.new_with_children(subject_style, &[content]).unwrap();
    tree.set_writing_mode(subject, writing_mode).unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size::from_lengths(300.0, 300.0), ..Default::default() },
            &[subject],
        )
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(300.0) },
        test_measure_function,
    )
    .unwrap();
    tree.layout(subject).unwrap().size
}

fn layout_content_sized_box(
    display: Display,
    preferred_block_size: Dimension,
    min_block_size: Dimension,
    max_block_size: Dimension,
) -> f32 {
    layout_subject(
        Style {
            display,
            size: Size { width: Dimension::length(100.0), height: preferred_block_size },
            min_size: Size { width: Dimension::auto(), height: min_block_size },
            max_size: Size { width: Dimension::auto(), height: max_block_size },
            flex_shrink: 0.0,
            ..Default::default()
        },
        WritingMode::HorizontalTb,
    )
    .height
}

#[test]
fn intrinsic_block_sizes_resolve_from_content_across_formatting_contexts() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for preferred in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
            assert_eq!(
                layout_content_sized_box(display, preferred, Dimension::auto(), Dimension::auto()),
                100.0,
                "{display:?} preferred {preferred:?}",
            );
        }

        assert_eq!(
            layout_content_sized_box(display, Dimension::length(0.0), Dimension::max_content(), Dimension::auto(),),
            100.0,
            "{display:?} min-block-size max-content",
        );
        assert_eq!(
            layout_content_sized_box(display, Dimension::length(200.0), Dimension::auto(), Dimension::min_content(),),
            100.0,
            "{display:?} max-block-size min-content",
        );
    }
}

#[test]
fn intrinsic_block_constraints_follow_a_column_flex_main_axis() {
    for (preferred, min, max) in [
        (Dimension::max_content(), Dimension::auto(), Dimension::auto()),
        (Dimension::length(0.0), Dimension::max_content(), Dimension::auto()),
        (Dimension::length(200.0), Dimension::auto(), Dimension::min_content()),
    ] {
        let size = layout_subject(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: Dimension::length(100.0), height: preferred },
                min_size: Size { width: Dimension::auto(), height: min },
                max_size: Size { width: Dimension::auto(), height: max },
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::HorizontalTb,
        );
        assert_eq!(size.height, 100.0, "preferred={preferred:?} min={min:?} max={max:?}");
    }
}

#[test]
fn intrinsic_block_constraints_follow_vertical_writing_modes() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let minimum = layout_subject(
            Style {
                display,
                size: Size::from_lengths(0.0, 100.0),
                min_size: Size { width: Dimension::max_content(), height: Dimension::auto() },
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::VerticalLr,
        );
        assert_eq!(minimum.width, 100.0, "{display:?} min-block-size");

        let maximum = layout_subject(
            Style {
                display,
                size: Size::from_lengths(200.0, 100.0),
                max_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
                flex_shrink: 0.0,
                ..Default::default()
            },
            WritingMode::VerticalRl,
        );
        assert_eq!(maximum.width, 100.0, "{display:?} max-block-size");
    }
}

#[test]
fn intrinsic_block_constraints_include_the_selected_box_edges() {
    let mut tree = new_test_tree();
    let content = tree.new_leaf(Style { size: Size::from_lengths(50.0, 100.0), ..Default::default() }).unwrap();
    let subject = tree
        .new_with_children(
            Style {
                display: Display::Block,
                box_sizing: BoxSizing::ContentBox,
                size: Size::from_lengths(50.0, 0.0),
                min_size: Size { width: Dimension::auto(), height: Dimension::max_content() },
                padding: Rect::length(10.0),
                ..Default::default()
            },
            &[content],
        )
        .unwrap();

    tree.compute_layout_with_measure(subject, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(subject).unwrap().size.height, 120.0);
}

fn layout_absolute_fit_content(display: Display) -> Layout {
    let mut tree = new_test_tree();
    let content = tree.new_leaf(Style { size: Size::from_lengths(20.0, 90.0), ..Default::default() }).unwrap();
    let subject = tree
        .new_with_children(
            Style {
                display: Display::Block,
                position: Position::Absolute,
                size: Size { width: Dimension::length(20.0), height: Dimension::fit_content() },
                inset: Rect { left: length(0.0), right: auto(), top: percent(0.5), bottom: length(0.0) },
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let root = tree
        .new_with_children(Style { display, size: Size::from_lengths(100.0, 100.0), ..Default::default() }, &[subject])
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(100.0) },
        test_measure_function,
    )
    .unwrap();
    *tree.layout(subject).unwrap()
}

#[test]
fn absolute_fit_content_block_size_is_content_sized_not_inset_stretched() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let layout = layout_absolute_fit_content(display);
        assert_eq!(layout.size.height, 90.0, "{display:?}");
        assert_eq!(layout.location.y, 50.0, "{display:?}");
    }
}
