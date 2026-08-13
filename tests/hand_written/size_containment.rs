use taffy::{prelude::*, Point};
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
    // Intrinsic content box is 111x222; padding and borders add 25x42 before
    // authored min/max constraints clamp the border box.
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
fn out_of_flow_grid_without_an_override_keeps_its_track_derived_size() {
    let mut tree = new_test_tree();
    let children = (0..3)
        .map(|_| tree.new_leaf(Style { size: Size::from_lengths(300.0, 400.0), ..Default::default() }).unwrap())
        .collect::<Vec<_>>();
    let subject = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                position: Position::Absolute,
                inset: Rect { left: length(10.0), top: length(20.0), ..Rect::auto() },
                grid_template_columns: vec![length(50.0), auto()],
                grid_template_rows: vec![length(30.0), auto()],
                gap: Size { width: length(5.0), height: length(5.0) },
                ..Default::default()
            },
            &children,
        )
        .unwrap();
    tree.set_size_containment(subject, contained_size(None, None)).unwrap();
    let root = tree
        .new_with_children(Style { size: Size::from_lengths(300.0, 200.0), ..Default::default() }, &[subject])
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(subject).unwrap().size, Size { width: 55.0, height: 35.0 });
    assert_eq!(tree.layout(subject).unwrap().location, Point { x: 10.0, y: 20.0 });
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
