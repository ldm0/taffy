use taffy::prelude::*;
use taffy::Point;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

fn layout_measured_item_with_parent_alignment(
    parent_display: Display,
    align_items: Option<AlignItems>,
    justify_items: Option<AlignItems>,
    child_style: Style,
) -> Layout {
    let mut tree = new_test_tree();
    let child = tree.new_leaf_with_context(child_style, TestNodeContext::fixed(20.0, 10.0)).unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: parent_display,
                size: Size::from_lengths(100.0, 80.0),
                grid_template_columns: if parent_display == Display::Grid {
                    vec![length(100.0)]
                } else {
                    Default::default()
                },
                grid_template_rows: if parent_display == Display::Grid {
                    vec![length(80.0)]
                } else {
                    Default::default()
                },
                align_items,
                justify_items,
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout_with_measure(parent, Size::MAX_CONTENT, test_measure_function).unwrap();
    *tree.layout(child).unwrap()
}

fn layout_measured_item(parent_display: Display, child_style: Style) -> Layout {
    layout_measured_item_with_parent_alignment(parent_display, None, None, child_style)
}

#[test]
fn flex_resolves_normal_self_alignment_to_cross_axis_stretch() {
    let layout = layout_measured_item(
        Display::Flex,
        Style { display: Display::Block, align_self: Some(AlignSelf::NORMAL), ..Default::default() },
    );

    assert_eq!(layout.size, Size { width: 20.0, height: 80.0 });
}

#[test]
fn grid_normal_uses_implicit_stretch_for_non_replaced_items() {
    let layout = layout_measured_item(
        Display::Grid,
        Style {
            display: Display::Block,
            align_self: Some(AlignSelf::NORMAL),
            justify_self: Some(AlignSelf::NORMAL),
            ..Default::default()
        },
    );

    assert_eq!(layout.size, Size { width: 100.0, height: 80.0 });
}

#[test]
fn grid_normal_keeps_replaced_items_content_sized_but_explicit_stretch_does_not() {
    let normal = layout_measured_item(
        Display::Grid,
        Style {
            display: Display::Block,
            item_is_replaced: true,
            align_self: Some(AlignSelf::NORMAL),
            justify_self: Some(AlignSelf::NORMAL),
            ..Default::default()
        },
    );
    let stretch = layout_measured_item(
        Display::Grid,
        Style {
            display: Display::Block,
            item_is_replaced: true,
            align_self: Some(AlignSelf::STRETCH),
            justify_self: Some(AlignSelf::STRETCH),
            ..Default::default()
        },
    );

    assert_eq!(normal.size, Size { width: 20.0, height: 10.0 });
    assert_eq!(stretch.size, Size { width: 100.0, height: 80.0 });
}

#[test]
fn grid_parent_normal_keeps_replaced_items_content_sized() {
    let layout = layout_measured_item_with_parent_alignment(
        Display::Grid,
        Some(AlignItems::NORMAL),
        Some(AlignItems::NORMAL),
        Style { display: Display::Block, item_is_replaced: true, ..Default::default() },
    );

    assert_eq!(layout.location, Point::ZERO);
    assert_eq!(layout.size, Size { width: 20.0, height: 10.0 });
}

#[test]
fn grid_explicit_normal_overrides_parent_center_alignment() {
    let layout = layout_measured_item_with_parent_alignment(
        Display::Grid,
        Some(AlignItems::CENTER),
        Some(AlignItems::CENTER),
        Style {
            display: Display::Block,
            item_is_replaced: true,
            align_self: Some(AlignSelf::NORMAL),
            justify_self: Some(AlignSelf::NORMAL),
            ..Default::default()
        },
    );

    assert_eq!(layout.location, Point::ZERO);
    assert_eq!(layout.size, Size { width: 20.0, height: 10.0 });
}

#[test]
fn grid_normal_lets_a_preferred_ratio_precede_implicit_stretch() {
    let normal = layout_measured_item(
        Display::Grid,
        Style {
            display: Display::Block,
            size: Size { width: auto(), height: length(20.0) },
            aspect_ratio: Some(2.0),
            justify_self: Some(AlignSelf::NORMAL),
            ..Default::default()
        },
    );
    let stretch = layout_measured_item(
        Display::Grid,
        Style {
            display: Display::Block,
            size: Size { width: auto(), height: length(20.0) },
            aspect_ratio: Some(2.0),
            justify_self: Some(AlignSelf::STRETCH),
            ..Default::default()
        },
    );

    assert_eq!(normal.size, Size { width: 40.0, height: 20.0 });
    assert_eq!(stretch.size, Size { width: 100.0, height: 20.0 });
}

fn layout_absolute_grid_item(child_style: Style) -> Layout {
    layout_measured_item(
        Display::Grid,
        Style { display: Display::Block, position: Position::Absolute, inset: Rect::length(0.0), ..child_style },
    )
}

#[test]
fn absolute_grid_normal_uses_replaced_aware_implicit_stretch() {
    let normal = layout_absolute_grid_item(Style {
        align_self: Some(AlignSelf::NORMAL),
        justify_self: Some(AlignSelf::NORMAL),
        ..Default::default()
    });
    let replaced_normal = layout_absolute_grid_item(Style {
        item_is_replaced: true,
        align_self: Some(AlignSelf::NORMAL),
        justify_self: Some(AlignSelf::NORMAL),
        ..Default::default()
    });
    let replaced_stretch = layout_absolute_grid_item(Style {
        item_is_replaced: true,
        align_self: Some(AlignSelf::STRETCH),
        justify_self: Some(AlignSelf::STRETCH),
        ..Default::default()
    });

    assert_eq!(normal.size, Size { width: 100.0, height: 80.0 });
    assert_eq!(replaced_normal.size, Size { width: 20.0, height: 10.0 });
    assert_eq!(replaced_stretch.size, Size { width: 100.0, height: 80.0 });
}

#[test]
fn absolute_grid_normal_lets_ratio_precede_implicit_stretch() {
    let normal = layout_absolute_grid_item(Style {
        size: Size { width: auto(), height: length(20.0) },
        aspect_ratio: Some(2.0),
        align_self: Some(AlignSelf::NORMAL),
        justify_self: Some(AlignSelf::NORMAL),
        ..Default::default()
    });
    let stretch = layout_absolute_grid_item(Style {
        size: Size { width: auto(), height: length(20.0) },
        aspect_ratio: Some(2.0),
        align_self: Some(AlignSelf::STRETCH),
        justify_self: Some(AlignSelf::STRETCH),
        ..Default::default()
    });

    assert_eq!(normal.size, Size { width: 40.0, height: 20.0 });
    assert_eq!(stretch.size, Size { width: 100.0, height: 20.0 });
}

#[test]
fn absolute_grid_start_alignment_does_not_stretch_between_insets() {
    let layout = layout_absolute_grid_item(Style {
        align_self: Some(AlignSelf::START),
        justify_self: Some(AlignSelf::START),
        ..Default::default()
    });

    assert_eq!(layout.size, Size { width: 20.0, height: 10.0 });
}
