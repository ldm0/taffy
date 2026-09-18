#![cfg(feature = "float_layout")]
use taffy::geometry::Point;
use taffy::prelude::*;
use taffy::style::{Clear, Float, FloatDirection};
use taffy_test_helpers::new_test_tree;

/// Once an inline line cannot fit beside a float, the final content slot must
/// move below it rather than returning full width at the original line position.
#[test]
fn content_slot_advances_below_last_float() {
    for direction in [FloatDirection::Left, FloatDirection::Right] {
        let mut context = taffy::FloatContext::new();
        context.set_width(200.0);
        context.place_floated_box(Size { width: 200.0, height: 80.0 }, 0.0, [0.0, 0.0], direction, Clear::None);

        let beside = context.find_content_slot(12.0, [10.0, 20.0], Clear::None, None);
        assert!(beside.segment_id.is_some());
        assert_eq!(beside.y, 12.0);

        let below = context.find_content_slot(12.0, [10.0, 20.0], Clear::None, beside.segment_id);
        assert_eq!(below.segment_id, None);
        assert_eq!((below.x, below.y, below.width), (10.0, 80.0, 170.0));

        // Advancing the search must also preserve a later requested line top.
        let later = context.find_content_slot(100.0, [10.0, 20.0], Clear::None, beside.segment_id);
        assert_eq!((later.x, later.y, later.width), (10.0, 100.0, 170.0));
    }
}

#[test]
fn content_slot_walks_staggered_float_bottoms() {
    for taller_first in [false, true] {
        let mut context = taffy::FloatContext::new();
        context.set_width(200.0);
        let mut floats = [(80.0, 40.0, FloatDirection::Left), (60.0, 90.0, FloatDirection::Right)];
        if taller_first {
            floats.reverse();
        }
        for (width, height, direction) in floats {
            context.place_floated_box(Size { width, height }, 0.0, [0.0, 0.0], direction, Clear::None);
        }

        let beside_both = context.find_content_slot(10.0, [0.0, 0.0], Clear::None, None);
        assert_eq!((beside_both.x, beside_both.y, beside_both.width), (80.0, 10.0, 60.0));
        assert!(beside_both.segment_id.is_some());

        let beside_right = context.find_content_slot(10.0, [0.0, 0.0], Clear::None, beside_both.segment_id);
        assert_eq!((beside_right.x, beside_right.y, beside_right.width), (0.0, 40.0, 140.0));
        assert!(beside_right.segment_id.is_some());

        let below = context.find_content_slot(10.0, [0.0, 0.0], Clear::None, beside_right.segment_id);
        assert_eq!(below.segment_id, None);
        assert_eq!((below.x, below.y, below.width), (0.0, 90.0, 200.0));
    }
}

#[test]
fn content_slot_below_float_uses_nested_block_coordinates() {
    let mut formatting_context = taffy::BlockFormattingContext::new();
    let mut root = formatting_context.root_block_context();
    root.set_width(200.0);
    root.place_floated_box(Size { width: 200.0, height: 80.0 }, 20.0, FloatDirection::Left, Clear::None, false);
    let nested = root.sub_context(30.0, [10.0, 20.0]);

    let beside = nested.find_content_slot(5.0, Clear::None, None);
    assert!(beside.segment_id.is_some());
    assert_eq!(beside.y, 5.0);

    let below = nested.find_content_slot(5.0, Clear::None, beside.segment_id);
    assert_eq!(below.segment_id, None);
    assert_eq!((below.x, below.y, below.width), (0.0, 70.0, 170.0));
}

/// Regression test for <https://wpt.live/css/CSS2/floats-clear/floats-146.xht>
///
/// A right float placed after two left floats must not be placed higher than the
/// outer top of the earlier floats (CSS2 float rule 5). This scenario previously
/// panicked in `FloatContext::subdivide_segment` when the float's end coincided
/// with an existing segment boundary due to floating point imprecision.
#[test]
fn float_no_higher_than_earlier_floats() {
    let mut taffy = new_test_tree();

    let block = |width: f32, height: f32, float: Float| Style {
        display: Display::Block,
        float,
        size: Size { width: length(width), height: length(height) },
        ..Default::default()
    };

    // Spacer block advancing the flow position to a fractional y offset (43.2)
    let spacer = taffy.new_leaf(block(400.0, 43.2, Float::None)).unwrap();
    let float_a = taffy.new_leaf(block(258.0, 42.0, Float::Left)).unwrap();
    let float_b = taffy.new_leaf(block(298.0, 42.0, Float::Left)).unwrap();
    let float_c = taffy.new_leaf(block(98.0, 42.0, Float::Right)).unwrap();

    let root = taffy
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(400.0), height: auto() },
                ..Default::default()
            },
            &[spacer, float_a, float_b, float_c],
        )
        .unwrap();

    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();

    let layout_a = taffy.layout(float_a).unwrap();
    let layout_b = taffy.layout(float_b).unwrap();
    let layout_c = taffy.layout(float_c).unwrap();

    // A is placed below the spacer
    assert_eq!(layout_a.location, Point { x: 0.0, y: 43.0 });
    // B does not fit next to A, so is placed below it
    assert_eq!(layout_b.location, Point { x: 0.0, y: 85.0 });
    // C must not be placed higher than B (it fits next to B on the right)
    assert_eq!(layout_c.location, Point { x: 302.0, y: 85.0 });
}

/// Regression test for <https://wpt.live/css/CSS2/floats-clear/margin-collapse-033.xht>
///
/// Floats that occupy no horizontal space (e.g. a zero-width float) still affect
/// `clear`: a cleared block must be placed past the bottom of all floats on the
/// relevant side, regardless of their width.
#[test]
fn clear_past_zero_width_float() {
    let mut taffy = new_test_tree();

    // float: left with auto width resolving to 0
    let floated = taffy
        .new_leaf(Style {
            display: Display::Block,
            float: Float::Left,
            size: Size { width: auto(), height: length(1.0) },
            ..Default::default()
        })
        .unwrap();
    let cleared = taffy.new_leaf(Style { display: Display::Block, clear: Clear::Left, ..Default::default() }).unwrap();
    let sibling = taffy
        .new_leaf(Style {
            display: Display::Block,
            margin: Rect { top: length(99.0), ..Rect::zero() },
            ..Default::default()
        })
        .unwrap();

    let container = taffy
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[floated, cleared, sibling],
        )
        .unwrap();
    let root = taffy.new_with_children(Style { display: Display::Block, ..Default::default() }, &[container]).unwrap();

    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();

    // The cleared (self-collapsing) block is placed below the float (y=1). Its clearance
    // prevents the sibling's 99px margin from collapsing with preceding margins, so the
    // container's height is 1 + 99 = 100.
    assert_eq!(taffy.layout(container).unwrap().size.height, 100.0);
}

fn float_block(width: f32, height: f32, float: Float) -> Style {
    Style {
        display: Display::Block,
        float,
        size: Size { width: length(width), height: length(height) },
        ..Default::default()
    }
}

fn root_style(width: f32) -> Style {
    Style { display: Display::Block, size: Size { width: length(width), height: auto() }, ..Default::default() }
}

/// Regression test for <https://wpt.live/css/CSS2/floats/floats-rule3-outside-left-001.xht>
///
/// CSS2 float rule 7: a float that is wider than its containing block and has no other
/// float beside it may overflow the containing block's edge rather than being pushed down.
#[test]
fn oversized_float_overflows_containing_block() {
    let mut taffy = new_test_tree();

    let float_a = taffy.new_leaf(float_block(150.0, 50.0, Float::Left)).unwrap();
    let root = taffy.new_with_children(root_style(100.0), &[float_a]).unwrap();

    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(taffy.layout(float_a).unwrap().location, Point { x: 0.0, y: 0.0 });
}

/// CSS2 float rules 3 & 7: a float with another float beside it must not overlap that float
/// (rule 3) nor extend past the containing block's opposite edge (rule 7) - it must move down.
#[test]
fn float_beside_existing_float_moves_down_instead_of_overflowing() {
    let mut taffy = new_test_tree();

    // Rule 3: an opposite-direction float that doesn't fit must move down, not overlap
    let left = taffy.new_leaf(float_block(60.0, 50.0, Float::Left)).unwrap();
    let right = taffy.new_leaf(float_block(60.0, 50.0, Float::Right)).unwrap();
    // Rule 7: a same-direction float that doesn't fit must move down, not overflow the
    // containing block's trailing edge
    let left2 = taffy.new_leaf(float_block(60.0, 50.0, Float::Left)).unwrap();

    let root = taffy.new_with_children(root_style(100.0), &[left, right, left2]).unwrap();

    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(taffy.layout(left).unwrap().location, Point { x: 0.0, y: 0.0 });
    assert_eq!(taffy.layout(right).unwrap().location, Point { x: 40.0, y: 50.0 });
    assert_eq!(taffy.layout(left2).unwrap().location, Point { x: 0.0, y: 100.0 });
}
