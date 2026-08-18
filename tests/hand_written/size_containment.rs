use taffy::prelude::*;
use taffy::{tree::DetailedLayoutInfo, WritingMode};
use taffy_test_helpers::{new_test_tree, test_measure_function, TestNodeContext};

fn contained_size(width: Option<f32>, height: Option<f32>) -> SizeContainment {
    SizeContainment::new(Size { width: true, height: true }, Size { width, height })
}

fn layout_root_with_child(display: Display, size_containment: SizeContainment) -> Size<f32> {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
    let root = tree.new_with_children(Style { display, ..Default::default() }, &[child]).unwrap();
    tree.set_size_containment(root, size_containment).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    tree.layout(root).unwrap().size
}

#[test]
fn explicit_contained_content_size_replaces_descendant_contributions() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        assert_eq!(
            layout_root_with_child(display, contained_size(Some(111.0), Some(222.0))),
            Size { width: 111.0, height: 222.0 },
            "{display:?}",
        );
    }
}

#[test]
fn contained_content_size_replaces_leaf_measurement() {
    let mut tree = new_test_tree();
    let root = tree.new_leaf_with_context(Style::default(), TestNodeContext::fixed(300.0, 400.0)).unwrap();
    tree.set_size_containment(root, contained_size(Some(111.0), Some(222.0))).unwrap();

    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 111.0, height: 222.0 });
}

#[test]
fn contained_content_size_uses_the_content_box_and_obeys_constraints() {
    let mut tree = new_test_tree();
    let padding = Rect { left: length(7.0), right: length(13.0), top: length(11.0), bottom: length(19.0) };
    let border = Rect { left: length(2.0), right: length(3.0), top: length(5.0), bottom: length(7.0) };
    let root = tree
        .new_leaf(Style {
            padding,
            border,
            min_size: Size { width: auto(), height: length(270.0) },
            max_size: Size { width: length(130.0), height: auto() },
            ..Default::default()
        })
        .unwrap();
    tree.set_size_containment(root, contained_size(Some(111.0), Some(222.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 130.0, height: 270.0 });
}

#[test]
fn explicit_preferred_size_wins_over_contained_content_size() {
    let mut tree = new_test_tree();
    let root = tree.new_leaf(Style { size: Size::from_lengths(80.0, 90.0), ..Default::default() }).unwrap();
    tree.set_size_containment(root, contained_size(Some(111.0), Some(222.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 80.0, height: 90.0 });
}

#[test]
fn contained_content_size_resolves_intrinsic_minimum_and_maximum_constraints() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for (preferred, min_size, max_size) in [
            (
                Size::from_lengths(50.0, 60.0),
                Size { width: Dimension::min_content(), height: Dimension::min_content() },
                Size::auto(),
            ),
            (
                Size::from_lengths(300.0, 400.0),
                Size::auto(),
                Size { width: Dimension::max_content(), height: Dimension::max_content() },
            ),
        ] {
            let mut tree = new_test_tree();
            let child = tree.new_leaf(Style { size: Size::from_lengths(500.0, 600.0), ..Default::default() }).unwrap();
            let root = tree
                .new_with_children(
                    Style { display, size: preferred, min_size, max_size, ..Default::default() },
                    &[child],
                )
                .unwrap();
            tree.set_size_containment(root, contained_size(Some(111.0), Some(222.0))).unwrap();

            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(root).unwrap().size, Size { width: 111.0, height: 222.0 }, "{display:?}");
        }
    }
}

#[test]
fn contained_content_size_is_the_subjects_intrinsic_contribution() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let mut tree = new_test_tree();
        let descendant = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
        let subject = tree.new_with_children(Style { display, ..Default::default() }, &[descendant]).unwrap();
        tree.set_size_containment(subject, contained_size(Some(111.0), Some(222.0))).unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Flex,
                    size: Size { width: Dimension::max_content(), height: Dimension::max_content() },
                    ..Default::default()
                },
                &[subject],
            )
            .unwrap();

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(root).unwrap().size, Size { width: 111.0, height: 222.0 }, "{display:?}");
    }
}

#[test]
fn single_axis_containment_preserves_the_other_natural_axis() {
    let mut tree = new_test_tree();
    let root = tree.new_leaf_with_context(Style::default(), TestNodeContext::fixed(300.0, 400.0)).unwrap();
    tree.set_size_containment(
        root,
        SizeContainment::new(Size { width: true, height: false }, Size { width: Some(111.0), height: Some(222.0) }),
    )
    .unwrap();

    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, test_measure_function).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 111.0, height: 400.0 });
}

#[test]
fn ordinary_contained_axes_have_zero_intrinsic_content_size_without_an_override() {
    for display in [Display::Block, Display::Flex] {
        assert_eq!(layout_root_with_child(display, contained_size(None, None)), Size::ZERO, "{display:?}",);
    }
}

#[test]
fn grid_without_an_override_uses_tracks_sized_without_item_contributions() {
    let mut tree = new_test_tree();
    let children = (0..3)
        .map(|_| tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap())
        .collect::<Vec<_>>();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![length(50.0), auto()],
                grid_template_rows: vec![length(30.0), auto()],
                gap: Size { width: length(5.0), height: length(5.0) },
                ..Default::default()
            },
            &children,
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(None, None)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 55.0, height: 35.0 });
}

#[test]
fn grid_track_derived_contained_size_resolves_intrinsic_constraints() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size::from_lengths(300.0, 400.0),
                max_size: Size { width: Dimension::max_content(), height: Dimension::max_content() },
                grid_template_columns: vec![length(50.0)],
                grid_template_rows: vec![length(30.0)],
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(None, None)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 50.0, height: 30.0 });
}

#[test]
fn contained_grid_size_establishes_available_space_for_flexible_tracks() {
    let mut tree = new_test_tree();
    let first = tree.new_leaf(Style::default()).unwrap();
    let second = tree.new_leaf(Style::default()).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![fr(1.0), fr(3.0)],
                grid_template_rows: vec![fr(1.0), fr(1.0)],
                ..Default::default()
            },
            &[first, second],
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(Some(100.0), Some(80.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 100.0, height: 80.0 });
    assert_eq!(tree.layout(first).unwrap().size, Size { width: 25.0, height: 40.0 });
    assert_eq!(tree.layout(second).unwrap().size, Size { width: 75.0, height: 40.0 });
}

#[test]
fn contained_grid_size_establishes_available_space_for_auto_repeat_columns() {
    let mut tree = new_test_tree();
    let children = (0..3)
        .map(|_| {
            tree.new_leaf(Style { size: Size { width: auto(), height: length(10.0) }, ..Default::default() }).unwrap()
        })
        .collect::<Vec<_>>();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: Dimension::max_content(), height: auto() },
                border: Rect { left: length(3.0), right: length(3.0), top: length(3.0), bottom: length(3.0) },
                grid_template_columns: vec![repeat("auto-fit", vec![length(15.0)])],
                gap: Size { width: length(5.0), height: zero() },
                ..Default::default()
            },
            &children,
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(Some(70.0), Some(80.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(root).unwrap().size, Size { width: 76.0, height: 86.0 });
    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(root) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.auto_repetitions, 3);
    for (child, expected_x) in children.into_iter().zip([3.0, 23.0, 43.0]) {
        assert_eq!(tree.layout(child).unwrap().location.x, expected_x);
        assert_eq!(tree.layout(child).unwrap().size.width, 15.0);
    }
}

#[test]
fn contained_grid_size_establishes_available_space_for_auto_repeat_rows() {
    let mut tree = new_test_tree();
    let children = (0..3).map(|_| tree.new_leaf(Style::default()).unwrap()).collect::<Vec<_>>();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: auto(), height: Dimension::max_content() },
                grid_template_columns: vec![fr(1.0)],
                grid_template_rows: vec![repeat("auto-fit", vec![length(20.0)])],
                gap: Size { width: zero(), height: length(10.0) },
                ..Default::default()
            },
            &children,
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(Some(70.0), Some(80.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(root).unwrap().size, Size { width: 70.0, height: 80.0 });
    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(root) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.rows.auto_repetitions, 3);
    for (child, expected_y) in children.into_iter().zip([0.0, 30.0, 60.0]) {
        assert_eq!(tree.layout(child).unwrap().location.y, expected_y);
        assert_eq!(tree.layout(child).unwrap().size.height, 20.0);
    }
}

#[test]
fn contained_auto_repeat_uses_the_clamped_used_size() {
    let mut tree = new_test_tree();
    let children = (0..4).map(|_| tree.new_leaf(Style::default()).unwrap()).collect::<Vec<_>>();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: Dimension::max_content(), height: auto() },
                max_size: Size { width: length(55.0), height: auto() },
                grid_template_columns: vec![repeat("auto-fit", vec![length(10.0)])],
                gap: Size { width: length(5.0), height: zero() },
                ..Default::default()
            },
            &children,
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(Some(100.0), None)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(root).unwrap().size.width, 55.0);
    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(root) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.auto_repetitions, 4);
}

#[test]
fn contained_auto_repeat_maps_physical_overrides_to_logical_grid_axes() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style::default()).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: Dimension::max_content(), height: Dimension::max_content() },
                grid_template_columns: vec![repeat("auto-fit", vec![length(15.0)])],
                grid_template_rows: vec![repeat("auto-fit", vec![length(20.0)])],
                gap: Size { width: length(5.0), height: length(10.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_writing_mode(root, WritingMode::VerticalRl).unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalRl).unwrap();
    tree.set_size_containment(root, contained_size(Some(80.0), Some(70.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(root).unwrap().size, Size { width: 80.0, height: 70.0 });
    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(root) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.auto_repetitions, 3);
    assert_eq!(info.rows.auto_repetitions, 3);
}

#[test]
fn contained_auto_repeat_without_an_explicit_override_remains_intrinsic() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style::default()).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![repeat("auto-fit", vec![length(10.0)])],
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(None, None)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    let DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(root) else {
        panic!("grid layout must publish detailed track information");
    };
    assert_eq!(info.columns.auto_repetitions, 1);
}

#[test]
fn grid_containment_is_independent_in_each_physical_axis() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![auto()],
                grid_template_rows: vec![auto()],
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_size_containment(root, SizeContainment::new(Size { width: true, height: false }, Size::NONE)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 0.0, height: 400.0 });
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 300.0, height: 400.0 });
}

#[test]
fn grid_containment_maps_physical_axes_through_vertical_writing_mode() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![auto()],
                grid_template_rows: vec![auto()],
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_writing_mode(root, WritingMode::VerticalRl).unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalRl).unwrap();
    tree.set_size_containment(root, SizeContainment::new(Size { width: true, height: false }, Size::NONE)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 0.0, height: 400.0 });
}

#[test]
fn contained_grid_content_size_is_wrapped_in_its_box_decoration() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                padding: Rect { left: length(10.0), right: length(10.0), top: length(10.0), bottom: length(10.0) },
                border: Rect { left: length(2.0), right: length(2.0), top: length(2.0), bottom: length(2.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(Some(111.0), Some(222.0))).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 135.0, height: 246.0 });
}

#[test]
fn contained_grid_intrinsic_size_excludes_item_created_implicit_tracks() {
    let mut tree = new_test_tree();
    let children = (0..3)
        .map(|_| tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap())
        .collect::<Vec<_>>();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![length(50.0)],
                grid_template_rows: vec![length(30.0)],
                grid_auto_rows: vec![length(70.0)],
                gap: Size { width: length(5.0), height: length(5.0) },
                ..Default::default()
            },
            &children,
        )
        .unwrap();
    tree.set_size_containment(root, contained_size(None, None)).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 50.0, height: 30.0 });
}

#[test]
fn updating_used_containment_invalidates_cached_layout() {
    let mut tree = new_test_tree();
    let child = tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap();
    let root = tree.new_with_children(Style { display: Display::Block, ..Default::default() }, &[child]).unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 300.0, height: 400.0 });

    tree.set_size_containment(root, contained_size(Some(111.0), Some(222.0))).unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 111.0, height: 222.0 });

    tree.set_size_containment(root, SizeContainment::NONE).unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 300.0, height: 400.0 });
}
