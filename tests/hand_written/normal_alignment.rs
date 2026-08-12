use taffy::prelude::*;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

fn layout_measured_item(parent_display: Display, child_style: Style) -> Layout {
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
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout_with_measure(parent, Size::MAX_CONTENT, test_measure_function).unwrap();
    *tree.layout(child).unwrap()
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

#[test]
fn grid_explicit_block_stretch_precedes_implicit_inline_stretch() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item = tree
        .new_leaf(Style {
            aspect_ratio: Some(1.0),
            align_self: Some(AlignSelf::STRETCH),
            justify_self: Some(AlignSelf::NORMAL),
            ..Default::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![length(50.0)],
                grid_template_rows: vec![length(100.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 50.0, height: 100.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

fn layout_absolute_item(parent_display: Display, is_replaced: bool, alignment: AlignSelf) -> Layout {
    layout_measured_item(
        parent_display,
        Style {
            display: Display::Block,
            position: Position::Absolute,
            item_is_replaced: is_replaced,
            inset: Rect::length(0.0),
            align_self: Some(alignment),
            justify_self: Some(alignment),
            ..Default::default()
        },
    )
}

#[test]
fn positioned_normal_uses_replaced_aware_implicit_stretch_in_every_container() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let normal = layout_absolute_item(display, false, AlignSelf::NORMAL);
        let replaced_normal = layout_absolute_item(display, true, AlignSelf::NORMAL);
        let replaced_stretch = layout_absolute_item(display, true, AlignSelf::STRETCH);

        assert_eq!(normal.size, Size { width: 100.0, height: 80.0 }, "{display:?} normal");
        assert_eq!(replaced_normal.size, Size { width: 20.0, height: 10.0 }, "{display:?} replaced normal");
        assert_eq!(replaced_stretch.size, Size { width: 100.0, height: 80.0 }, "{display:?} replaced stretch");
    }
}
