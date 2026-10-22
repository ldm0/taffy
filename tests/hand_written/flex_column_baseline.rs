use taffy::prelude::*;
use taffy::Direction;

#[test]
fn column_baseline_groups_ignore_wider_nonparticipants_and_follow_cross_start() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for flow in [FlexDirection::Column, FlexDirection::ColumnReverse] {
            for wrap in [FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse] {
                for wide_participates in [false, true] {
                    let mut tree = TaffyTree::<()>::new();
                    let item = |width, align_self| Style {
                        display: Display::Block,
                        direction,
                        size: Size { width: length(width), height: length(10.0) },
                        flex_shrink: 0.0,
                        align_self: Some(align_self),
                        ..Style::default()
                    };
                    let narrow = tree.new_leaf(item(10.0, AlignSelf::BASELINE)).unwrap();
                    let wide = tree
                        .new_leaf(item(
                            30.0,
                            if wide_participates { AlignSelf::BASELINE } else { AlignSelf::FLEX_START },
                        ))
                        .unwrap();
                    let container = tree
                        .new_with_children(
                            Style {
                                direction,
                                flex_direction: flow,
                                flex_wrap: wrap,
                                align_content: AlignContent::FLEX_START,
                                size: Size { width: length(60.0), height: length(20.0) },
                                ..Style::default()
                            },
                            &[narrow, wide],
                        )
                        .unwrap();
                    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
                    let cross_reversed = (direction == Direction::Rtl) ^ (wrap == FlexWrap::WrapReverse);
                    let group_width = if wide_participates { 30.0 } else { 10.0 };
                    let expected = if cross_reversed { 60.0 - group_width } else { 0.0 };
                    assert_eq!(
                        tree.layout(narrow).unwrap().location.x,
                        expected,
                        "direction={direction:?}, flow={flow:?}, wrap={wrap:?}, wide_participates={wide_participates}"
                    );
                    if wide_participates {
                        assert_eq!(tree.layout(wide).unwrap().location.x, expected);
                    }
                }
            }
        }
    }
}
