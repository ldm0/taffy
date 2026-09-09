use taffy::prelude::*;
use taffy::{style::GridTemplateAreas, tree::DetailedLayoutInfo, Direction, WritingMode};

#[test]
fn detailed_grid_info_preserves_logical_track_and_line_order_in_every_flow() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut tree = TaffyTree::<()>::new();
            let children = [-4, 1, 2].map(|start| {
                tree.new_leaf(Style { grid_column: Line { start: line(start), end: span(1) }, ..Default::default() })
                    .unwrap()
            });
            let grid = tree
                .new_with_children(
                    Style {
                        display: Display::Grid,
                        direction,
                        grid_template_columns: vec![length(40.0), length(70.0)],
                        grid_template_rows: vec![length(30.0)],
                        grid_auto_columns: vec![length(20.0)],
                        gap: Size { width: length(5.0), height: length(7.0) },
                        ..Default::default()
                    },
                    &children,
                )
                .unwrap();
            tree.set_writing_mode(grid, mode).unwrap();
            tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();
            let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
                panic!("grid must publish used track metadata");
            };
            assert_eq!(info.columns.negative_implicit_tracks, 1, "{mode:?}/{direction:?}");
            assert_eq!(info.columns.explicit_tracks, 2);
            assert_eq!(info.columns.positive_implicit_tracks, 0);
            assert_eq!(info.columns.sizes, [20.0, 40.0, 70.0]);
            assert_eq!(info.columns.gutters, [0.0, 5.0, 5.0, 0.0]);
            assert_eq!(info.rows.sizes, [30.0]);
            for (index, item) in info.items.iter().enumerate() {
                assert_eq!(item.column_start, index as u16 + 1);
                assert_eq!(item.column_end, index as u16 + 2);
                assert_eq!((item.row_start, item.row_end), (1, 2));
            }
        }
    }
}

#[test]
fn detailed_grid_info_retains_auto_repetitions_separately_from_explicit_tracks() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let child = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                grid_template_columns: vec![repeat("auto-fill", vec![length(20.0)])],
                grid_template_areas: Some(GridTemplateAreas { areas: Vec::new(), row_count: 1, column_count: 8 }),
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(grid) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.rows.auto_repetitions, 0);
    assert_eq!(info.columns.explicit_tracks, 8);
    assert_eq!(info.columns.auto_repetitions, 5);
}
