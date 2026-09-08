use taffy::prelude::*;
use taffy::{Direction, Point, WritingDirection, WritingMode};

fn absolute_layout(container_style: Style, child_style: Style) -> Layout {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            position: Position::Absolute,
            size: Size { width: length(40.0), height: length(20.0) },
            ..child_style
        })
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                size: Size { width: length(220.0), height: length(140.0) },
                box_sizing: BoxSizing::BorderBox,
                border: Rect { left: length(2.0), right: length(2.0), top: length(2.0), bottom: length(2.0) },
                padding: Rect { left: length(20.0), right: length(20.0), top: length(10.0), bottom: length(10.0) },
                justify_content: Some(JustifyContent::CENTER),
                align_items: Some(AlignItems::CENTER),
                ..container_style
            },
            &[child],
        )
        .unwrap();
    tree.disable_rounding();
    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    *tree.layout(child).unwrap()
}

#[test]
#[cfg(feature = "block_layout")]
fn absolute_margins_deduct_insets_and_containing_borders_before_distribution() {
    for display in [Display::Block, Display::Flex] {
        for (right, expected_margin, expected_x) in [(auto(), [68.0, 68.0], 90.0), (length(30.0), [106.0, 30.0], 128.0)]
        {
            let layout = absolute_layout(
                Style { display, ..Style::default() },
                Style {
                    inset: Rect { left: length(20.0), right: length(20.0), ..Rect::auto() },
                    margin: Rect { left: auto(), right, ..Rect::zero() },
                    ..Style::default()
                },
            );
            assert_eq!([layout.margin.left, layout.margin.right], expected_margin, "{display:?}");
            assert_eq!(layout.location.x, expected_x, "{display:?}");
        }
    }
}

#[test]
fn absolute_auto_inset_keeps_auto_margins_zero() {
    let layout = absolute_layout(Style::default(), Style { margin: Rect::auto(), ..Style::default() });
    assert_eq!(layout.margin, Rect::ZERO);
    assert_eq!(layout.location, Point { x: 90.0, y: 60.0 });
    let layout = absolute_layout(
        Style::default(),
        Style { inset: Rect { left: length(25.0), ..Rect::auto() }, margin: Rect::auto(), ..Style::default() },
    );
    assert_eq!(layout.margin, Rect::ZERO);
    assert_eq!(layout.location, Point { x: 27.0, y: 60.0 });
}

#[test]
fn absolute_negative_auto_margins_follow_the_containing_writing_direction() {
    // Exercise the shared resolver directly: no layout algorithm is allowed to
    // clamp negative free space before resolving its logical inline/block axes.
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let resolved = taffy::compute::resolve_absolute_margins(
                Rect { left: None, right: None, top: None, bottom: None },
                Rect { left: Some(10.0), right: Some(10.0), top: Some(10.0), bottom: Some(10.0) },
                Size { width: 100.0, height: 100.0 },
                Size { width: 120.0, height: 120.0 },
                WritingDirection { mode, direction },
            );
            let logical = WritingDirection { mode, direction }.to_logical_box_strut(resolved);
            assert_eq!(logical.inline_start, 0.0);
            assert_eq!(logical.inline_end, -40.0);
            assert_eq!(logical.block_start, -20.0);
            assert_eq!(logical.block_end, -20.0);
        }
    }
}

#[test]
fn absolute_cross_safety_uses_padding_box_and_explicit_self_alignment() {
    for flex_wrap in [FlexWrap::NoWrap, FlexWrap::WrapReverse] {
        for (align_self, height, top, bottom, expected_y) in [
            (None, 140.0, auto(), auto(), 0.0),
            (Some(AlignSelf::SAFE_CENTER), 140.0, auto(), auto(), 2.0),
            (Some(AlignSelf::SAFE_CENTER), 100.0, length(20.0), length(30.0), 22.0),
            (Some(AlignSelf::SAFE_CENTER), 140.0, length(-20.0), length(-20.0), 0.0),
        ] {
            let mut tree = TaffyTree::<()>::new();
            let child = tree
                .new_leaf(Style {
                    position: Position::Absolute,
                    size: Size { width: length(40.0), height: length(height) },
                    margin: Rect { top, bottom, ..Rect::zero() },
                    align_self,
                    ..Style::default()
                })
                .unwrap();
            let parent = tree
                .new_with_children(
                    Style {
                        display: Display::Flex,
                        size: Size { width: length(220.0), height: length(140.0) },
                        border: Rect { left: length(2.0), right: length(2.0), top: length(2.0), bottom: length(2.0) },
                        padding: Rect {
                            left: length(20.0),
                            right: length(20.0),
                            top: length(10.0),
                            bottom: length(10.0),
                        },
                        align_items: Some(AlignItems::SAFE_CENTER),
                        flex_wrap,
                        ..Style::default()
                    },
                    &[child],
                )
                .unwrap();
            tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(child).unwrap().location.y, expected_y, "{flex_wrap:?}, {align_self:?}, {height}");
        }
    }
}

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
