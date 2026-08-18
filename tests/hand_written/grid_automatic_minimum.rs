use taffy::prelude::*;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

#[derive(Clone, Copy)]
enum TrackScenario {
    UnrelatedFlexible,
    SpannedFlexible,
    UnrelatedAutoMinimum,
}

fn layout_spanning_item(scenario: TrackScenario) -> (Layout, Layout) {
    let mut tree = new_test_tree();
    let item = tree
        .new_leaf_with_context(
            Style { grid_column: Line { start: line(1), end: line(3) }, ..Default::default() },
            TestNodeContext::fixed(150.0, 10.0),
        )
        .unwrap();
    let unrelated_track_item =
        tree.new_leaf(Style { grid_column: Line { start: line(3), end: line(4) }, ..Default::default() }).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(100.0, 20.0),
                grid_template_columns: match scenario {
                    TrackScenario::UnrelatedFlexible => vec![auto(), auto(), fr(1.0)],
                    TrackScenario::SpannedFlexible => vec![auto(), fr(1.0), auto()],
                    TrackScenario::UnrelatedAutoMinimum => {
                        vec![minmax(length(0.0), auto()), minmax(length(0.0), auto()), auto()]
                    }
                },
                grid_template_rows: vec![length(20.0)],
                ..Default::default()
            },
            &[item, unrelated_track_item],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    (*tree.layout(item).unwrap(), *tree.layout(unrelated_track_item).unwrap())
}

#[test]
fn unrelated_flexible_track_does_not_zero_a_spanning_items_automatic_minimum() {
    let (item, unrelated_track_item) = layout_spanning_item(TrackScenario::UnrelatedFlexible);

    assert_eq!(item.size.width, 150.0);
    assert_eq!(unrelated_track_item.location.x, 150.0);
}

#[test]
fn flexible_track_within_a_multi_track_span_zeros_the_automatic_minimum() {
    let (item, unrelated_track_item) = layout_spanning_item(TrackScenario::SpannedFlexible);

    assert_eq!(item.size.width, 100.0);
    assert_eq!(unrelated_track_item.location.x, 100.0);
}

#[test]
fn unrelated_auto_min_track_does_not_enable_a_spanning_items_automatic_minimum() {
    let (item, unrelated_track_item) = layout_spanning_item(TrackScenario::UnrelatedAutoMinimum);

    assert_eq!(item.size.width, 100.0);
    assert_eq!(unrelated_track_item.location.x, 100.0);
}

#[test]
fn intrinsic_minimum_can_raise_a_smaller_fixed_track_maximum() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let contributor = tree
        .new_leaf(Style {
            box_sizing: BoxSizing::ContentBox,
            size: Size { width: length(60.0), height: auto() },
            margin: Rect { left: length(5.0), right: length(10.0), top: zero(), bottom: zero() },
            padding: Rect { left: length(6.0), right: length(3.0), ..Rect::zero() },
            border: Rect { left: length(2.0), right: length(4.0), ..Rect::zero() },
            grid_row: Line { start: line(1), end: line(2) },
            ..Default::default()
        })
        .unwrap();
    let stretched =
        tree.new_leaf(Style { grid_row: Line { start: line(2), end: line(3) }, ..Default::default() }).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                grid_template_columns: vec![minmax(auto(), length(0.0))],
                ..Default::default()
            },
            &[contributor, stretched],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    // The contributor's 60px content box plus 9px padding, 6px border and
    // 15px margins establishes a 90px automatic minimum for the track. The
    // fixed 0px maximum is floored by that minimum rather than suppressing it.
    assert_eq!(tree.layout(contributor).unwrap().size.width, 75.0);
    assert_eq!(tree.layout(stretched).unwrap().size.width, 90.0);
}

#[test]
fn ratio_item_automatic_minimum_transfers_a_stretched_cross_size() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let item = tree
        .new_leaf(Style {
            aspect_ratio: Some(2.0),
            align_self: Some(AlignSelf::STRETCH),
            justify_self: Some(AlignSelf::STRETCH),
            ..Default::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style { display: Display::Grid, size: Size::from_lengths(200.0, 200.0), ..Default::default() },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    // The 200px stretched block size is a definite transferred-size
    // suggestion. The item's automatic inline minimum therefore contributes
    // 400px to the auto column, even though the grid container is only 200px
    // wide. The final explicit stretch then fills that 400px track.
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 400.0, height: 200.0 });
}
