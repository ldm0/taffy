use taffy::prelude::*;

fn fixed_size(width: f32, height: f32) -> Size<Dimension> {
    Size { width: length(width), height: length(height) }
}

fn vertical_margins(top: f32, bottom: f32) -> Rect<LengthPercentageAuto> {
    Rect { top: length(top), bottom: length(bottom), ..Rect::zero() }
}

fn row_container(size: Size<Dimension>) -> Style {
    Style { display: Display::Flex, size, align_items: Some(AlignItems::FLEX_START), ..Style::default() }
}

#[test]
fn explicit_cross_stretch_is_definite_for_percentage_children() {
    let flex_bases = [
        Dimension::auto(),
        Dimension::content(),
        Dimension::min_content(),
        Dimension::fit_content(),
        Dimension::max_content(),
    ];

    for direction in [FlexDirection::Row, FlexDirection::Column] {
        for flex_basis in flex_bases {
            let is_row = direction == FlexDirection::Row;
            let mut tree = TaffyTree::<()>::new();
            let percentage_child = tree
                .new_leaf(Style {
                    size: if is_row {
                        Size { width: Dimension::auto(), height: percent(1.0) }
                    } else {
                        Size { width: percent(1.0), height: Dimension::auto() }
                    },
                    aspect_ratio: Some(1.0),
                    ..Style::default()
                })
                .unwrap();
            let item = tree
                .new_with_children(
                    Style {
                        size: if is_row {
                            Size { width: Dimension::auto(), height: Dimension::stretch() }
                        } else {
                            Size { width: Dimension::stretch(), height: Dimension::auto() }
                        },
                        min_size: Size::zero(),
                        flex_basis,
                        flex_grow: 0.0,
                        flex_shrink: 0.0,
                        ..Style::default()
                    },
                    &[percentage_child],
                )
                .unwrap();
            let container = tree
                .new_with_children(
                    Style { flex_direction: direction, ..row_container(fixed_size(50.0, 50.0)) },
                    &[item],
                )
                .unwrap();

            tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

            assert_eq!(
                tree.layout(item).unwrap().size,
                Size { width: 50.0, height: 50.0 },
                "{direction:?}, flex-basis: {flex_basis:?}",
            );
            assert_eq!(
                tree.layout(percentage_child).unwrap().size,
                Size { width: 50.0, height: 50.0 },
                "{direction:?}, flex-basis: {flex_basis:?}",
            );
        }
    }
}

#[test]
fn explicit_stretch_fills_the_available_margin_box() {
    for box_sizing in [BoxSizing::ContentBox, BoxSizing::BorderBox] {
        let mut tree = TaffyTree::<()>::new();
        let item = tree
            .new_leaf(Style {
                size: Size { width: length(20.0), height: Dimension::stretch() },
                margin: vertical_margins(10.0, 15.0),
                padding: Rect::length(5.0),
                border: Rect::length(2.0),
                box_sizing,
                ..Style::default()
            })
            .unwrap();
        let container = tree.new_with_children(row_container(fixed_size(100.0, 100.0)), &[item]).unwrap();

        tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(item).unwrap().size.height, 75.0, "{box_sizing:?}");
    }
}

#[test]
fn stretch_maximum_is_re_resolved_from_the_final_flex_line() {
    let mut tree = TaffyTree::<()>::new();
    let limited = tree
        .new_leaf(Style {
            size: fixed_size(40.0, 55.0),
            max_size: Size { width: Dimension::auto(), height: Dimension::stretch() },
            margin: vertical_margins(2.0, 3.0),
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let line_owner =
        tree.new_leaf(Style { size: fixed_size(40.0, 60.0), flex_shrink: 0.0, ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style { flex_wrap: FlexWrap::Wrap, ..row_container(fixed_size(100.0, 50.0)) },
            &[limited, line_owner],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(limited).unwrap().size.height, 55.0);
}

#[test]
fn stretch_minimum_is_re_resolved_from_the_final_flex_line() {
    let mut tree = TaffyTree::<()>::new();
    let limited = tree
        .new_leaf(Style {
            size: fixed_size(40.0, 55.0),
            min_size: Size { width: Dimension::auto(), height: Dimension::stretch() },
            margin: vertical_margins(2.0, 3.0),
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let line_owner =
        tree.new_leaf(Style { size: fixed_size(40.0, 80.0), flex_shrink: 0.0, ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style { flex_wrap: FlexWrap::Wrap, ..row_container(fixed_size(100.0, 50.0)) },
            &[limited, line_owner],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(limited).unwrap().size.height, 75.0);
}

#[test]
fn explicit_cross_stretch_transfers_through_aspect_ratio() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: Dimension::auto(), height: Dimension::stretch() },
            min_size: Size { width: length(0.0), height: Dimension::auto() },
            align_self: Some(AlignSelf::FLEX_START),
            aspect_ratio: Some(2.0),
            flex_grow: 0.0,
            flex_shrink: 0.0,
            ..Style::default()
        })
        .unwrap();
    let container = tree.new_with_children(row_container(fixed_size(200.0, 50.0)), &[item]).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 50.0 });
}
