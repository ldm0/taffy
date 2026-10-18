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

#[test]
fn orthogonal_grid_items_resolve_self_alignment_in_their_own_writing_mode() {
    use taffy::{Direction, Point, WritingMode};
    for container_direction in [Direction::Ltr, Direction::Rtl] {
        for item_direction in [Direction::Ltr, Direction::Rtl] {
            for item_mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
                for self_start in [true, false] {
                    let mut tree = TaffyTree::<()>::new();
                    let alignment = if self_start { AlignSelf::SELF_START } else { AlignSelf::SELF_END };
                    let item = tree
                        .new_leaf(Style {
                            direction: item_direction,
                            size: Size { width: length(20.0), height: length(50.0) },
                            justify_self: Some(alignment),
                            align_self: Some(alignment),
                            ..Style::default()
                        })
                        .unwrap();
                    tree.set_writing_mode(item, item_mode).unwrap();
                    let grid = tree
                        .new_with_children(
                            Style {
                                display: Display::Grid,
                                direction: container_direction,
                                grid_template_columns: vec![length(100.0)],
                                grid_template_rows: vec![length(150.0)],
                                ..Style::default()
                            },
                            &[item],
                        )
                        .unwrap();
                    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();
                    let right = (item_mode == WritingMode::VerticalRl) == self_start;
                    let bottom = (item_direction == Direction::Rtl) == self_start;
                    assert_eq!(tree.layout(item).unwrap().location,
                        Point { x: if right { 80.0 } else { 0.0 }, y: if bottom { 100.0 } else { 0.0 } },
                        "container={container_direction:?}, item={item_direction:?}/{item_mode:?}, self_start={self_start}");
                }
            }
        }
    }
}
