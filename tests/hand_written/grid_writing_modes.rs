use taffy::prelude::*;
use taffy::{Direction, Point, WritingMode};

#[test]
fn grid_track_alignment_projects_logical_start_end_and_distribution_in_rtl() {
    // Replaces the keyword-mirroring test: alignment retains its logical
    // meaning, and only the final fragment is projected to physical space.
    for (alignment, starts) in [
        (AlignContent::START, [0.0, 20.0]),
        (AlignContent::END, [60.0, 80.0]),
        (AlignContent::FLEX_START, [0.0, 20.0]),
        (AlignContent::FLEX_END, [60.0, 80.0]),
        (AlignContent::STRETCH, [0.0, 20.0]),
        (AlignContent::CENTER, [30.0, 50.0]),
        (AlignContent::SPACE_BETWEEN, [0.0, 80.0]),
        (AlignContent::SPACE_EVENLY, [20.0, 60.0]),
        (AlignContent::SPACE_AROUND, [15.0, 65.0]),
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut tree = TaffyTree::<()>::new();
            let children = [0, 1].map(|_| {
                tree.new_leaf(Style { size: Size { width: length(10.0), height: length(10.0) }, ..Style::default() })
                    .unwrap()
            });
            let root = tree
                .new_with_children(
                    Style {
                        display: Display::Grid,
                        direction,
                        size: Size { width: length(120.0), height: length(50.0) },
                        grid_template_columns: vec![length(20.0), length(40.0)],
                        grid_template_rows: vec![length(20.0)],
                        justify_content: Some(alignment),
                        justify_items: Some(AlignItems::START),
                        align_items: Some(AlignItems::START),
                        ..Style::default()
                    },
                    &children,
                )
                .unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            for (child, start) in children.into_iter().zip(starts) {
                let expected = if direction == Direction::Rtl { 110.0 - start } else { start };
                assert_eq!(tree.layout(child).unwrap().location.x, expected, "{alignment:?}/{direction:?}");
            }
        }
    }
}

fn layout_grid(mode: WritingMode, direction: Direction, size: Size<Dimension>) -> Vec<Layout> {
    let mut tree = TaffyTree::<()>::new();
    let children = (0..4)
        .map(|_| {
            let node = tree
                .new_leaf(Style { size: Size { width: length(10.0), height: length(15.0) }, ..Style::default() })
                .unwrap();
            tree.set_writing_mode(node, mode).unwrap();
            node
        })
        .collect::<Vec<_>>();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                direction,
                size,
                grid_template_columns: vec![length(40.0), length(70.0)],
                grid_template_rows: vec![length(20.0), length(30.0)],
                align_items: Some(AlignItems::START),
                justify_items: Some(AlignItems::START),
                ..Style::default()
            },
            &children,
        )
        .unwrap();
    tree.set_writing_mode(root, mode).unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    std::iter::once(root).chain(children).map(|node| *tree.layout(node).unwrap()).collect()
}

#[test]
fn grid_tracks_and_auto_placement_follow_logical_start_edges() {
    // Numeric physical expectations independent of the implementation's
    // writing-mode converter. Columns are the inline axis in every mode.
    for (mode, direction, expected) in [
        (WritingMode::HorizontalTb, Direction::Ltr, [(0.0, 0.0), (40.0, 0.0), (0.0, 20.0), (40.0, 20.0)]),
        (WritingMode::HorizontalTb, Direction::Rtl, [(110.0, 0.0), (70.0, 0.0), (110.0, 20.0), (70.0, 20.0)]),
        (WritingMode::VerticalRl, Direction::Ltr, [(110.0, 0.0), (110.0, 40.0), (90.0, 0.0), (90.0, 40.0)]),
        (WritingMode::VerticalRl, Direction::Rtl, [(110.0, 85.0), (110.0, 45.0), (90.0, 85.0), (90.0, 45.0)]),
        (WritingMode::VerticalLr, Direction::Ltr, [(0.0, 0.0), (0.0, 40.0), (20.0, 0.0), (20.0, 40.0)]),
        (WritingMode::VerticalLr, Direction::Rtl, [(0.0, 85.0), (0.0, 45.0), (20.0, 85.0), (20.0, 45.0)]),
        (WritingMode::SidewaysRl, Direction::Ltr, [(110.0, 0.0), (110.0, 40.0), (90.0, 0.0), (90.0, 40.0)]),
        (WritingMode::SidewaysRl, Direction::Rtl, [(110.0, 85.0), (110.0, 45.0), (90.0, 85.0), (90.0, 45.0)]),
        (WritingMode::SidewaysLr, Direction::Ltr, [(0.0, 85.0), (0.0, 45.0), (20.0, 85.0), (20.0, 45.0)]),
        (WritingMode::SidewaysLr, Direction::Rtl, [(0.0, 0.0), (0.0, 40.0), (20.0, 0.0), (20.0, 40.0)]),
    ] {
        let layouts = layout_grid(mode, direction, Size { width: length(120.0), height: length(100.0) });
        for (layout, (x, y)) in layouts[1..].iter().zip(expected) {
            assert_eq!(layout.location, Point { x, y }, "{mode:?}/{direction:?}");
            assert_eq!(layout.size, Size { width: 10.0, height: 15.0 });
        }
    }
}

#[test]
fn intrinsic_grid_size_projects_columns_and_rows_to_physical_axes() {
    for (mode, expected) in [
        (WritingMode::HorizontalTb, Size { width: 110.0, height: 50.0 }),
        (WritingMode::VerticalRl, Size { width: 50.0, height: 110.0 }),
        (WritingMode::VerticalLr, Size { width: 50.0, height: 110.0 }),
        (WritingMode::SidewaysRl, Size { width: 50.0, height: 110.0 }),
        (WritingMode::SidewaysLr, Size { width: 50.0, height: 110.0 }),
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let layouts = layout_grid(mode, direction, Size { width: auto(), height: auto() });
            assert_eq!(layouts[0].size, expected, "{mode:?}/{direction:?}");
        }
    }
}
