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
    tree.disable_rounding();
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
