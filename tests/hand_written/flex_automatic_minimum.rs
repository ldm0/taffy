use taffy::prelude::*;
use taffy::WritingMode;

#[test]
fn flex_minimum_distinguishes_content_ratio_and_specified_main_size() {
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [FlexDirection::Row, FlexDirection::Column] {
            let horizontal = mode.is_horizontal() == (direction == FlexDirection::Row);
            let dimensions = |main, cross| {
                if horizontal {
                    Size { width: main, height: cross }
                } else {
                    Size { width: cross, height: main }
                }
            };
            for display in [Display::Block, Display::Flex, Display::Grid] {
                for (name, specified, cross, content, expected) in [
                    ("empty", None, Some(100.0), 0.0, 100.0),
                    ("smaller content", None, Some(100.0), 60.0, 100.0),
                    ("larger content", None, Some(100.0), 160.0, 160.0),
                    ("specified cap", Some(70.0), Some(100.0), 160.0, 70.0),
                    ("main is not independent cross", Some(70.0), None, 0.0, 0.0),
                ] {
                    let mut tree = TaffyTree::<()>::new();
                    let content = tree
                        .new_leaf(Style { size: dimensions(length(content), length(0.0)), ..Style::default() })
                        .unwrap();
                    let item = tree
                        .new_with_children(
                            Style {
                                display,
                                size: dimensions(
                                    specified.map(length).unwrap_or(auto()),
                                    cross.map(length).unwrap_or(auto()),
                                ),
                                aspect_ratio: Some(1.0),
                                flex_basis: length(0.0),
                                align_self: Some(AlignSelf::START),
                                ..Style::default()
                            },
                            &[content],
                        )
                        .unwrap();
                    let owner = tree
                        .new_with_children(
                            Style {
                                display: Display::Flex,
                                flex_direction: direction,
                                size: dimensions(length(0.0), length(100.0)),
                                ..Style::default()
                            },
                            &[item],
                        )
                        .unwrap();
                    for node in [owner, item, content] {
                        tree.set_writing_mode(node, mode).unwrap();
                    }
                    tree.compute_layout(owner, Size::MAX_CONTENT).unwrap();
                    let actual = tree.layout(item).unwrap().size;
                    assert_eq!(
                        if horizontal { actual.width } else { actual.height },
                        expected,
                        "{mode:?} {direction:?} {display:?} {name}"
                    );
                }
            }
        }
    }
}
