use taffy::prelude::*;
use taffy::{style::GridTemplateAreas, tree::DetailedLayoutInfo};

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
