use taffy::prelude::*;
use taffy::WritingMode;

// CSS Flexbox 1 section 9.5 requires positive main-axis free space to be
// consumed by auto margins before justify-content is applied. This also
// covers the interaction exercised by WPT flexbox-column-row-gap-001.html.
#[test]
fn main_axis_auto_margin_consumes_free_space_before_justification() {
    let mut tree = TaffyTree::<()>::new();
    let leading = tree
        .new_leaf(Style {
            size: Size { width: length(596.0), height: length(45.0) },
            margin: Rect { right: auto(), ..Rect::zero() },
            ..Style::default()
        })
        .unwrap();
    let trailing =
        tree.new_leaf(Style { size: Size { width: length(308.0), height: length(45.0) }, ..Style::default() }).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(1392.0), height: length(45.0) },
                justify_content: Some(JustifyContent::SPACE_BETWEEN),
                ..Style::default()
            },
            &[leading, trailing],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(leading).unwrap().location.x, 0.0);
    assert_eq!(tree.layout(leading).unwrap().margin.right, 488.0);
    assert_eq!(tree.layout(trailing).unwrap().location.x, 1084.0);
}

struct CrossSizePercentageCase {
    item_height: Dimension,
    align_self: AlignSelf,
    auto_cross_margins: bool,
}

fn layout_cross_size_percentage_case(case: CrossSizePercentageCase) -> (Layout, Layout, Layout) {
    let mut tree = TaffyTree::<()>::new();
    let percentage_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(100.0), height: percent(1.0) },
            ..Style::default()
        })
        .unwrap();
    let fixed_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(100.0), height: length(100.0) },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: auto(), height: case.item_height },
                margin: if case.auto_cross_margins {
                    Rect { top: auto(), bottom: auto(), ..Rect::zero() }
                } else {
                    Rect::zero()
                },
                align_self: Some(case.align_self),
                ..Style::default()
            },
            &[percentage_child, fixed_child],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(100.0), height: length(200.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    (*tree.layout(item).unwrap(), *tree.layout(percentage_child).unwrap(), *tree.layout(fixed_child).unwrap())
}

#[test]
fn cross_axis_auto_margins_suppress_stretch_and_keep_auto_block_size_indefinite() {
    let (item, percentage_child, fixed_child) = layout_cross_size_percentage_case(CrossSizePercentageCase {
        item_height: auto(),
        align_self: AlignSelf::STRETCH,
        auto_cross_margins: true,
    });

    assert_eq!(item.size, Size { width: 100.0, height: 100.0 });
    assert_eq!(item.margin.top, 50.0);
    assert_eq!(item.margin.bottom, 50.0);
    assert_eq!(percentage_child.size.height, 0.0);
    assert_eq!(fixed_child.size.height, 100.0);
}

#[test]
fn non_stretched_auto_cross_size_is_not_a_percentage_basis() {
    let (item, percentage_child, _) = layout_cross_size_percentage_case(CrossSizePercentageCase {
        item_height: auto(),
        align_self: AlignSelf::FLEX_START,
        auto_cross_margins: false,
    });

    assert_eq!(item.size.height, 100.0);
    assert_eq!(percentage_child.size.height, 0.0);
}

#[test]
fn stretched_or_authored_cross_sizes_remain_percentage_bases() {
    let (stretched_item, stretched_percentage, _) = layout_cross_size_percentage_case(CrossSizePercentageCase {
        item_height: auto(),
        align_self: AlignSelf::STRETCH,
        auto_cross_margins: false,
    });
    assert_eq!(stretched_item.size.height, 200.0);
    assert_eq!(stretched_percentage.size.height, 200.0);

    let (authored_item, authored_percentage, _) = layout_cross_size_percentage_case(CrossSizePercentageCase {
        item_height: length(100.0),
        align_self: AlignSelf::STRETCH,
        auto_cross_margins: true,
    });
    assert_eq!(authored_item.size.height, 100.0);
    assert_eq!(authored_percentage.size.height, 100.0);
}

#[test]
fn orthogonal_auto_margin_item_clears_its_physical_block_percentage_basis() {
    let mut tree = TaffyTree::<()>::new();
    let percentage_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: percent(1.0), height: length(50.0) },
            ..Style::default()
        })
        .unwrap();
    let fixed_child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(100.0), height: length(50.0) },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                margin: Rect { left: auto(), right: auto(), ..Rect::zero() },
                ..Style::default()
            },
            &[percentage_child, fixed_child],
        )
        .unwrap();
    for node in [percentage_child, fixed_child, item] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }
    let container = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                size: Size { width: length(200.0), height: length(100.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(item).unwrap().margin.left, 50.0);
    assert_eq!(tree.layout(item).unwrap().margin.right, 50.0);
    assert_eq!(tree.layout(percentage_child).unwrap().size.width, 0.0);
    assert_eq!(tree.layout(fixed_child).unwrap().size.width, 100.0);
}
