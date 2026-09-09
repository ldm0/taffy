use taffy::prelude::*;
use taffy::{Direction, Overflow, Point, WritingMode};

#[test]
fn flow_geometry_preserves_placement_before_relative_insets_in_every_formatting_context() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for display in [Display::Block, Display::Flex, Display::Grid] {
                for flex_direction in
                    [FlexDirection::Row, FlexDirection::RowReverse, FlexDirection::Column, FlexDirection::ColumnReverse]
                {
                    let mut tree = TaffyTree::<()>::new();
                    tree.disable_rounding();
                    let style = Style {
                        display: Display::Block,
                        size: Size { width: length(40.0), height: length(20.0) },
                        margin: Rect { left: length(3.0), right: length(7.0), top: length(5.0), bottom: length(9.0) },
                        ..Style::default()
                    };
                    let child = tree.new_leaf(style.clone()).unwrap();
                    let parent = tree
                        .new_with_children(
                            Style {
                                display,
                                direction,
                                flex_direction,
                                size: Size { width: length(300.0), height: length(200.0) },
                                padding: length(10.0),
                                justify_content: Some(AlignContent::CENTER),
                                align_content: Some(AlignContent::CENTER),
                                align_items: Some(AlignItems::CENTER),
                                ..Style::default()
                            },
                            &[child],
                        )
                        .unwrap();
                    tree.set_writing_mode(parent, mode).unwrap();
                    tree.set_writing_mode(child, mode).unwrap();
                    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
                    let original = *tree.unrounded_layout(child);
                    assert_eq!(original.in_flow.unwrap().location, original.location);
                    tree.set_style(
                        child,
                        Style { inset: Rect { left: length(25.0), top: length(-35.0), ..auto() }, ..style },
                    )
                    .unwrap();
                    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
                    let moved = *tree.unrounded_layout(child);
                    let context = (mode, direction, display, flex_direction);
                    assert_eq!(moved.in_flow, original.in_flow, "{context:?}");
                    assert_eq!(
                        moved.location,
                        Point { x: original.location.x + 25.0, y: original.location.y - 35.0 },
                        "{context:?}"
                    );
                    // The result remains authoritative when child layout is cached.
                    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
                    assert_eq!(*tree.unrounded_layout(child), moved);
                }
            }
        }
    }
}

#[test]
fn absolute_and_hidden_children_have_no_flow_contribution() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let mut tree = TaffyTree::<()>::new();
        let positioned = tree
            .new_leaf(Style {
                position: Position::Absolute,
                size: Size { width: length(20.0), height: length(20.0) },
                ..Style::default()
            })
            .unwrap();
        let hidden = tree.new_leaf(Style { display: Display::None, ..Style::default() }).unwrap();
        let parent = tree.new_with_children(Style { display, ..Style::default() }, &[positioned, hidden]).unwrap();
        tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
        assert!(tree.unrounded_layout(positioned).in_flow.is_none());
        assert!(tree.unrounded_layout(hidden).in_flow.is_none());
    }
}

#[test]
fn self_collapsing_flow_geometry_does_not_count_the_adjoining_strut_twice() {
    let mut tree = TaffyTree::<()>::new();
    let block = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(50.0), height: length(100.0) },
            margin: Rect { bottom: length(10.0), ..zero() },
            ..Style::default()
        })
        .unwrap();
    let collapsed = tree
        .new_leaf(Style {
            display: Display::Block,
            margin: Rect { bottom: length(50.0), ..zero() },
            ..Style::default()
        })
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(100.0) },
                overflow: Point { x: Overflow::Hidden, y: Overflow::Hidden },
                ..Style::default()
            },
            &[block, collapsed],
        )
        .unwrap();
    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
    let layout = tree.unrounded_layout(collapsed);
    let flow = layout.in_flow.unwrap();
    assert_eq!(flow.location.y + layout.size.height + flow.margin.bottom, 150.0);
}
