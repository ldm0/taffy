//! `fit-content()` growth-limit distribution for spanning grid items.

use taffy::prelude::*;
use taffy::tree::DetailedLayoutInfo;
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext, WritingMode};

#[derive(Clone, Copy)]
enum Axis {
    Columns,
    Rows,
}

fn track_sizes(
    axis: Axis,
    tracks: Vec<GridTemplateComponent<String>>,
    spanning_text: &str,
    second_track_item_size: Option<f32>,
) -> Vec<f32> {
    let mut tree = new_test_tree();
    tree.disable_rounding();

    let track_count = tracks.len() as i16;
    let spanning_item = tree
        .new_leaf_with_context(
            Style {
                grid_column: match axis {
                    Axis::Columns => Line { start: line(1), end: line(track_count + 1) },
                    Axis::Rows => Line::AUTO,
                },
                grid_row: match axis {
                    Axis::Columns => Line::AUTO,
                    Axis::Rows => Line { start: line(1), end: line(track_count + 1) },
                },
                ..Default::default()
            },
            TestNodeContext::ahem_text(
                spanning_text.to_owned(),
                match axis {
                    Axis::Columns => WritingMode::Horizontal,
                    Axis::Rows => WritingMode::Vertical,
                },
            ),
        )
        .unwrap();

    let mut children = vec![spanning_item];
    if let Some(size) = second_track_item_size {
        let item = tree
            .new_leaf_with_context(
                Style {
                    grid_column: match axis {
                        Axis::Columns => Line { start: line(2), end: line(3) },
                        Axis::Rows => Line::AUTO,
                    },
                    grid_row: match axis {
                        Axis::Columns => Line::AUTO,
                        Axis::Rows => Line { start: line(2), end: line(3) },
                    },
                    ..Default::default()
                },
                match axis {
                    Axis::Columns => TestNodeContext::fixed(size, 10.0),
                    Axis::Rows => TestNodeContext::fixed(10.0, size),
                },
            )
            .unwrap();
        children.push(item);
    }

    let (columns, rows) = match axis {
        Axis::Columns => (tracks, vec![length(10.0)]),
        Axis::Rows => (vec![length(10.0)], tracks),
    };
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(100.0, 100.0),
                justify_content: Some(JustifyContent::START),
                align_content: Some(AlignContent::START),
                grid_template_columns: columns,
                grid_template_rows: rows,
                ..Default::default()
            },
            &children,
        )
        .unwrap();

    tree.compute_layout_with_measure(grid, Size::MAX_CONTENT, test_measure_function).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    match axis {
        Axis::Columns => info.columns.sizes.clone(),
        Axis::Rows => info.rows.sizes.clone(),
    }
}

#[test]
fn infinite_fit_content_growth_limit_participates_before_finite_tracks() {
    for axis in [Axis::Columns, Axis::Rows] {
        let sizes = track_sizes(
            axis,
            vec![fit_content(LengthPercentage::length(110.0)), fit_content(LengthPercentage::length(40.0))],
            "XXX\u{200b}XXX",
            Some(20.0),
        );
        assert_eq!(sizes, [40.0, 20.0]);
    }
}

#[test]
fn infinite_fit_content_growth_limit_shares_with_other_infinite_tracks() {
    for axis in [Axis::Columns, Axis::Rows] {
        let sizes = track_sizes(
            axis,
            vec![auto(), fit_content(LengthPercentage::length(110.0)), auto()],
            "XXX\u{200b}XXX\u{200b}XXX",
            None,
        );
        assert_eq!(sizes, [30.0, 30.0, 30.0]);
    }
}
