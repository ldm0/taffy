use taffy::geometry::Point;
use taffy::prelude::*;
use taffy::style::Overflow;
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

#[test]
fn intrinsic_minimum_floors_a_smaller_fixed_track_maximum() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let contributor = tree
        .new_leaf(Style {
            box_sizing: BoxSizing::ContentBox,
            size: Size { width: length(60.0), height: auto() },
            margin: Rect { left: length(5.0), right: length(10.0), top: zero(), bottom: zero() },
            padding: Rect { left: length(6.0), right: length(3.0), ..Rect::zero() },
            border: Rect { left: length(2.0), right: length(4.0), ..Rect::zero() },
            ..Default::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                grid_template_columns: vec![minmax(auto(), length(0.0))],
                ..Default::default()
            },
            &[contributor],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.sizes, vec![90.0]);
    assert_eq!(tree.layout(contributor).unwrap().size.width, 75.0);
}

fn automatic_minimum_with_fixed_track_maximum(track_maximum: f32) -> f32 {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item = tree
        .new_leaf_with_context(
            Style {
                margin: Rect { left: length(5.0), right: length(10.0), top: zero(), bottom: zero() },
                border: Rect { left: length(2.0), right: length(2.0), ..Rect::zero() },
                justify_self: Some(AlignSelf::START),
                ..Default::default()
            },
            TestNodeContext::fixed(100.0, 10.0),
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(200.0), height: auto() },
                grid_template_columns: vec![minmax(auto(), length(track_maximum))],
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
fn automatic_minimum_fixed_maximum_clamp_preserves_outer_border_floor() {
    // The 15px outer margins and 4px border floor the fixed zero maximum.
    assert_eq!(automatic_minimum_with_fixed_track_maximum(0.0), 19.0);
    // A larger fixed maximum clamps the complete outer contribution rather
    // than the border box before margins are added.
    assert_eq!(automatic_minimum_with_fixed_track_maximum(20.0), 20.0);
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
fn replaced_width_transfers_to_an_automatic_row_minimum() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item_style = || Style {
        item_is_replaced: true,
        size: Size { width: length(50.0), height: auto() },
        aspect_ratio: Some(0.5),
        align_self: Some(AlignSelf::STRETCH),
        justify_self: Some(AlignSelf::STRETCH),
        ..Default::default()
    };
    let first = tree.new_leaf_with_context(item_style(), TestNodeContext::aspect_ratio(25.0, 2.0)).unwrap();
    let second = tree.new_leaf_with_context(item_style(), TestNodeContext::aspect_ratio(25.0, 2.0)).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(10.0, 10.0),
                grid_template_columns: vec![auto(), auto()],
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.sizes, vec![50.0, 50.0]);
    assert_eq!(info.rows.sizes, vec![100.0]);
    assert_eq!(tree.layout(first).unwrap().size, Size { width: 50.0, height: 100.0 });
    assert_eq!(tree.layout(second).unwrap().size, Size { width: 50.0, height: 100.0 });
}

#[test]
fn replaced_height_transfers_to_an_automatic_column_minimum() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item_style = || Style {
        item_is_replaced: true,
        size: Size { width: auto(), height: length(50.0) },
        aspect_ratio: Some(2.0),
        align_self: Some(AlignSelf::STRETCH),
        justify_self: Some(AlignSelf::STRETCH),
        ..Default::default()
    };
    let first = tree.new_leaf_with_context(item_style(), TestNodeContext::aspect_ratio(50.0, 0.5)).unwrap();
    let second = tree.new_leaf_with_context(item_style(), TestNodeContext::aspect_ratio(50.0, 0.5)).unwrap();
    let grid = tree
        .new_with_children(
            Style { display: Display::Grid, size: Size::from_lengths(10.0, 10.0), ..Default::default() },
            &[first, second],
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.sizes, vec![100.0]);
    assert_eq!(info.rows.sizes, vec![50.0, 50.0]);
    assert_eq!(tree.layout(first).unwrap().size, Size { width: 100.0, height: 50.0 });
    assert_eq!(tree.layout(second).unwrap().size, Size { width: 100.0, height: 50.0 });
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

#[test]
fn grid_item_percentages_resolve_against_the_final_grid_area() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let content = tree.new_leaf(Style { size: Size::from_lengths(30.0, 30.0), ..Default::default() }).unwrap();
    let item = tree
        .new_with_children(
            Style {
                size: Size { width: percent(1.0), height: percent(1.0) },
                min_size: Size { width: percent(1.0), height: percent(1.0) },
                grid_row: Line { start: line(2), end: line(3) },
                grid_column: Line { start: line(2), end: line(3) },
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(10.0, 10.0),
                grid_template_columns: vec![length(3.0), auto(), length(4.0)],
                grid_template_rows: vec![length(1.0), auto(), length(2.0)],
                align_items: Some(AlignItems::START),
                justify_items: Some(AlignItems::START),
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 3.0, height: 7.0 });
    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.sizes, vec![3.0, 3.0, 4.0]);
    assert_eq!(info.rows.sizes, vec![1.0, 7.0, 2.0]);
}

#[test]
fn percentage_preferred_size_does_not_replace_the_automatic_minimum() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let content = tree.new_leaf(Style { size: Size::from_lengths(100.0, 450.0), ..Default::default() }).unwrap();
    let item = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: auto(), height: percent(1.0) }, ..Default::default() },
            &[content],
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(180.0, 200.0),
                grid_template_columns: vec![length(180.0)],
                grid_template_rows: vec![auto()],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.rows.sizes, vec![450.0]);
    assert_eq!(tree.layout(item).unwrap().size.height, 450.0);
}

#[derive(Debug, PartialEq)]
struct NestedScrollLayout {
    track: f32,
    sidebar: f32,
    controlled: f32,
    wrapper: f32,
    contents: f32,
}

fn nested_scroll_container_layout(wrapper_height: Dimension) -> NestedScrollLayout {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let tall_content = tree.new_leaf(Style { size: Size::from_lengths(100.0, 400.0), ..Default::default() }).unwrap();
    let sidebar_contents = tree
        .new_with_children(
            Style {
                display: Display::Block,
                grid_row: Line { start: line(2), end: line(3) },
                overflow: Point { x: Overflow::Scroll, y: Overflow::Scroll },
                max_size: Size { width: auto(), height: percent(1.0) },
                ..Default::default()
            },
            &[tall_content],
        )
        .unwrap();
    let empty = tree.new_leaf(Style::default()).unwrap();
    let wrapper = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: wrapper_height },
                overflow: Point { x: Overflow::Hidden, y: Overflow::Hidden },
                grid_template_columns: vec![length(100.0)],
                grid_template_rows: vec![length(50.0), fr(1.0)],
                ..Default::default()
            },
            &[empty, sidebar_contents],
        )
        .unwrap();
    let controlled = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                overflow: Point { x: Overflow::Scroll, y: Overflow::Scroll },
                ..Default::default()
            },
            &[wrapper],
        )
        .unwrap();
    let sidebar = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: auto(), height: percent(1.0) },
                flex_grow: 1.0,
                ..Default::default()
            },
            &[controlled],
        )
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(180.0, 200.0),
                grid_template_columns: vec![length(180.0)],
                grid_template_rows: vec![auto()],
                ..Default::default()
            },
            &[sidebar],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    NestedScrollLayout {
        track: info.rows.sizes[0],
        sidebar: tree.layout(sidebar).unwrap().size.height,
        controlled: tree.layout(controlled).unwrap().size.height,
        wrapper: tree.layout(wrapper).unwrap().size.height,
        contents: tree.layout(sidebar_contents).unwrap().size.height,
    }
}

#[test]
fn nested_scroll_container_keeps_the_grid_items_automatic_minimum() {
    assert_eq!(
        nested_scroll_container_layout(auto()),
        NestedScrollLayout { track: 450.0, sidebar: 450.0, controlled: 450.0, wrapper: 450.0, contents: 400.0 }
    );
}

#[test]
fn nested_scroll_container_resolves_percentages_after_its_block_size_becomes_definite() {
    assert_eq!(
        nested_scroll_container_layout(length(200.0)),
        NestedScrollLayout { track: 200.0, sidebar: 200.0, controlled: 200.0, wrapper: 200.0, contents: 150.0 }
    );
}

#[test]
fn orthogonal_block_contribution_uses_the_resolved_inline_grid_area() {
    let mut tree = new_test_tree();
    tree.disable_rounding();
    let item = tree
        .new_leaf_with_context(
            Style::default(),
            TestNodeContext::ahem_text("aaaaaaaaaa\u{200b}aaaaa".to_owned(), WritingMode::Vertical),
        )
        .unwrap();
    tree.set_writing_mode(item, taffy::WritingMode::VerticalLr).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: length(100.0) },
                grid_template_columns: vec![auto()],
                grid_template_rows: vec![auto()],
                ..Default::default()
            },
            &[item],
        )
        .unwrap();
    let intrinsic_parent =
        tree.new_with_children(Style { display: Display::Flex, ..Default::default() }, &[grid]).unwrap();

    tree.compute_layout_with_measure(intrinsic_parent, Size::MAX_CONTENT, test_measure_function).unwrap();

    assert_eq!(tree.layout(intrinsic_parent).unwrap().size.width, 20.0);
    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 20.0, height: 100.0 });
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 20.0, height: 100.0 });
}
