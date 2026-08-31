use taffy::prelude::*;
use taffy::tree::DetailedLayoutInfo;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

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

#[derive(Clone, Copy)]
enum TrackAxis {
    Columns,
    Rows,
}

fn layout_single_auto_track(axis: TrackAxis, item_style: Style) -> (f32, Layout) {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let item = tree.new_leaf(item_style).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(10.0, 10.0),
                grid_template_columns: match axis {
                    TrackAxis::Columns => vec![minmax(auto(), auto())],
                    TrackAxis::Rows => vec![length(10.0)],
                },
                grid_template_rows: match axis {
                    TrackAxis::Columns => vec![length(10.0)],
                    TrackAxis::Rows => vec![minmax(auto(), auto())],
                },
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    let track_size = match axis {
        TrackAxis::Columns => info.columns.sizes[0],
        TrackAxis::Rows => info.rows.sizes[0],
    };
    (track_size, *tree.layout(item).unwrap())
}

#[test]
fn definite_preferred_size_uses_its_clamped_min_content_contribution() {
    let (track, item) = layout_single_auto_track(
        TrackAxis::Columns,
        Style {
            size: Size::from_lengths(60.0, 10.0),
            min_size: Size { width: length(90.0), height: auto() },
            ..Default::default()
        },
    );

    assert_eq!(track, 90.0);
    assert_eq!(item.size.width, 90.0);
}

#[test]
fn smaller_minimum_does_not_replace_a_definite_preferred_size() {
    let (track, item) = layout_single_auto_track(
        TrackAxis::Columns,
        Style {
            size: Size::from_lengths(60.0, 10.0),
            min_size: Size { width: length(40.0), height: auto() },
            ..Default::default()
        },
    );

    assert_eq!(track, 60.0);
    assert_eq!(item.size.width, 60.0);
}

#[test]
fn ratio_transferred_auto_preferred_size_still_uses_the_used_minimum() {
    let (track, item) = layout_single_auto_track(
        TrackAxis::Columns,
        Style {
            size: Size { width: auto(), height: length(60.0) },
            min_size: Size { width: length(90.0), height: auto() },
            aspect_ratio: Some(1.0),
            ..Default::default()
        },
    );

    assert_eq!(track, 90.0);
    assert_eq!(item.size.width, 90.0);
}

#[test]
fn block_axis_uses_the_same_minimum_contribution_source_rules() {
    let (track, item) = layout_single_auto_track(
        TrackAxis::Rows,
        Style {
            size: Size::from_lengths(10.0, 60.0),
            min_size: Size { width: auto(), height: length(90.0) },
            ..Default::default()
        },
    );

    assert_eq!(track, 90.0);
    assert_eq!(item.size.height, 90.0);
}

#[test]
fn content_box_minimum_contribution_includes_padding_and_border() {
    let (track, item) = layout_single_auto_track(
        TrackAxis::Columns,
        Style {
            box_sizing: BoxSizing::ContentBox,
            size: Size::from_lengths(60.0, 10.0),
            min_size: Size { width: length(90.0), height: auto() },
            padding: Rect { left: length(8.0), right: length(8.0), top: length(0.0), bottom: length(0.0) },
            border: Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) },
            ..Default::default()
        },
    );

    assert_eq!(track, 116.0);
    assert_eq!(item.size.width, 116.0);
}

fn intrinsic_minimum_contribution(width: Dimension, min_width: Dimension) -> f32 {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item = tree
        .new_leaf_with_context(
            Style {
                box_sizing: BoxSizing::ContentBox,
                size: Size { width, height: length(10.0) },
                min_size: Size { width: min_width, height: auto() },
                border: Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) },
                ..Default::default()
            },
            TestNodeContext::ahem_text("aaaa\u{200b}aaaa".to_owned(), WritingMode::Horizontal),
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(10.0, 10.0),
                grid_template_columns: vec![minmax(auto(), auto())],
                grid_template_rows: vec![length(10.0)],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    info.columns.sizes[0]
}

#[test]
fn max_content_preferred_size_uses_the_max_content_minimum_contribution() {
    assert_eq!(intrinsic_minimum_contribution(Dimension::min_content(), auto()), 50.0);
    assert_eq!(intrinsic_minimum_contribution(Dimension::max_content(), auto()), 90.0);
}

#[test]
fn intrinsic_minimum_sizes_select_their_content_contribution() {
    assert_eq!(intrinsic_minimum_contribution(auto(), Dimension::min_content()), 50.0);
    assert_eq!(intrinsic_minimum_contribution(auto(), Dimension::max_content()), 90.0);
}
