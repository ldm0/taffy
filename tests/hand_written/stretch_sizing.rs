use taffy::prelude::*;
use taffy::WritingMode;

fn layout_stretched_child(
    child_display: Display,
    child_size: Size<Dimension>,
    child_min_size: Size<Dimension>,
    child_max_size: Size<Dimension>,
    parent_size: Size<Dimension>,
) -> Layout {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style { display: Display::Block, size: Size::from_lengths(20.0, 20.0), ..Default::default() })
        .unwrap();
    let child = tree
        .new_with_children(
            Style {
                display: child_display,
                size: child_size,
                min_size: child_min_size,
                max_size: child_max_size,
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let root = tree
        .new_with_children(Style { display: Display::Block, size: parent_size, ..Default::default() }, &[child])
        .unwrap();

    tree.compute_layout(root, Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::Definite(300.0) })
        .unwrap();
    *tree.layout(child).unwrap()
}

#[test]
fn definite_block_available_size_resolves_preferred_minimum_and_maximum_stretch() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let preferred = layout_stretched_child(
            display,
            Size { width: length(20.0), height: Dimension::stretch() },
            Size::AUTO,
            Size::AUTO,
            Size::from_lengths(100.0, 100.0),
        );
        assert_eq!(preferred.size.height, 100.0, "{display:?} preferred stretch");

        let minimum = layout_stretched_child(
            display,
            Size::from_lengths(20.0, 20.0),
            Size { width: auto(), height: Dimension::stretch() },
            Size::AUTO,
            Size::from_lengths(100.0, 100.0),
        );
        assert_eq!(minimum.size.height, 100.0, "{display:?} minimum stretch");

        let maximum = layout_stretched_child(
            display,
            Size::from_lengths(20.0, 120.0),
            Size::AUTO,
            Size { width: auto(), height: Dimension::stretch() },
            Size::from_lengths(100.0, 100.0),
        );
        assert_eq!(maximum.size.height, 100.0, "{display:?} maximum stretch");
    }
}

#[test]
fn indefinite_block_available_size_uses_each_stretch_fallback() {
    let preferred = layout_stretched_child(
        Display::Block,
        Size { width: length(20.0), height: Dimension::stretch() },
        Size::AUTO,
        Size::AUTO,
        Size { width: length(100.0), height: auto() },
    );
    assert_eq!(preferred.size.height, 20.0);

    let minimum = layout_stretched_child(
        Display::Block,
        Size::from_lengths(20.0, 20.0),
        Size { width: auto(), height: Dimension::stretch() },
        Size::AUTO,
        Size { width: length(100.0), height: auto() },
    );
    assert_eq!(minimum.size.height, 20.0);

    let maximum = layout_stretched_child(
        Display::Block,
        Size::from_lengths(20.0, 120.0),
        Size::AUTO,
        Size { width: auto(), height: Dimension::stretch() },
        Size { width: length(100.0), height: auto() },
    );
    assert_eq!(maximum.size.height, 120.0);
}

fn layout_nested_stretch(parent_style: Style, child_style: Style) -> Layout {
    let mut tree = TaffyTree::<()>::new();
    let child = tree.new_leaf(child_style).unwrap();
    let parent = tree.new_with_children(parent_style, &[child]).unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size::from_lengths(200.0, 200.0), ..Default::default() },
            &[parent],
        )
        .unwrap();
    tree.compute_layout(root, Size { width: AvailableSpace::Definite(200.0), height: AvailableSpace::Definite(200.0) })
        .unwrap();
    *tree.layout(child).unwrap()
}

fn layout_stretched_item(parent_display: Display, child_style: Style) -> Layout {
    layout_nested_stretch(
        Style {
            display: parent_display,
            size: Size::from_lengths(100.0, 100.0),
            grid_template_columns: if parent_display == Display::Grid {
                vec![length(100.0)]
            } else {
                Default::default()
            },
            grid_template_rows: if parent_display == Display::Grid { vec![length(100.0)] } else { Default::default() },
            ..Default::default()
        },
        child_style,
    )
}

#[test]
fn flex_and_grid_items_resolve_stretch_from_their_containing_area() {
    for display in [Display::Flex, Display::Grid] {
        let block_axis = layout_stretched_item(
            display,
            Style {
                display: Display::Block,
                size: Size { width: length(20.0), height: Dimension::stretch() },
                margin: Rect { left: zero(), right: zero(), top: length(10.0), bottom: length(10.0) },
                ..Default::default()
            },
        );
        assert_eq!(block_axis.size.height, 80.0, "{display:?} item block axis");

        let inline_axis = layout_stretched_item(
            display,
            Style {
                display: Display::Block,
                size: Size { width: Dimension::stretch(), height: length(20.0) },
                margin: Rect { left: length(10.0), right: length(5.0), top: zero(), bottom: zero() },
                ..Default::default()
            },
        );
        assert_eq!(inline_axis.size.width, 85.0, "{display:?} item inline axis");
    }
}

#[test]
fn flex_and_grid_items_apply_stretch_to_minimum_and_maximum_constraints() {
    for display in [Display::Flex, Display::Grid] {
        let minimum = layout_stretched_item(
            display,
            Style {
                display: Display::Block,
                size: Size::from_lengths(20.0, 20.0),
                min_size: Size { width: auto(), height: Dimension::stretch() },
                ..Default::default()
            },
        );
        assert_eq!(minimum.size.height, 100.0, "{display:?} item minimum");

        let maximum = layout_stretched_item(
            display,
            Style {
                display: Display::Block,
                size: Size::from_lengths(20.0, 120.0),
                max_size: Size { width: auto(), height: Dimension::stretch() },
                ..Default::default()
            },
        );
        assert_eq!(maximum.size.height, 100.0, "{display:?} item maximum");
    }
}

#[test]
fn grid_auto_track_resolves_maximum_stretch_after_intrinsic_track_sizing() {
    let child = layout_nested_stretch(
        Style { display: Display::Grid, size: Size::from_lengths(100.0, 100.0), ..Default::default() },
        Style {
            display: Display::Block,
            size: Size::from_lengths(20.0, 120.0),
            max_size: Size { width: auto(), height: Dimension::stretch() },
            ..Default::default()
        },
    );

    // The automatic row is intrinsically sized before `stretch` has a
    // definite grid area, so it grows to the unclamped preferred size. The
    // resulting 120px area then becomes the stretch basis.
    assert_eq!(child.size.height, 120.0);
}

#[test]
fn wrapped_flex_item_reresolves_stretch_against_its_line() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            box_sizing: BoxSizing::ContentBox,
            size: Size::from_lengths(20.0, 55.0),
            max_size: Size { width: auto(), height: Dimension::stretch() },
            margin: Rect { left: zero(), right: zero(), top: length(2.0), bottom: length(3.0) },
            padding: Rect::length(2.0),
            border: Rect::length(3.0),
            ..Default::default()
        })
        .unwrap();
    let tall_sibling = tree
        .new_leaf(Style { display: Display::Block, size: Size::from_lengths(20.0, 60.0), ..Default::default() })
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                box_sizing: BoxSizing::ContentBox,
                size: Size::from_lengths(100.0, 50.0),
                flex_wrap: FlexWrap::Wrap,
                ..Default::default()
            },
            &[child, tall_sibling],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    // The 50px container gives the preliminary 45px margin-box clamp used
    // while sizing the line. The sibling then establishes a 60px line, so
    // the final stretch maximum is 60 - 2 - 3 = 55px.
    assert_eq!(tree.layout(child).unwrap().size.height, 55.0);
}

/// Regression for
/// <https://wpt.live/css/css-sizing/stretch/flex-line-004.html>.
///
/// The container's intrinsic width is clamped after its min-content
/// contribution is measured. That final used width is available to
/// `width: stretch` without becoming a definite percentage basis. A wider
/// sibling may still enlarge its own flex line independently.
#[test]
fn wrapped_column_stretch_uses_the_clamped_container_width_per_line() {
    let mut tree = TaffyTree::<()>::new();
    let content = |tree: &mut TaffyTree<()>| {
        tree.new_leaf(Style { display: Display::Block, size: Size::from_lengths(20.0, 20.0), ..Default::default() })
            .unwrap()
    };
    let first_content = content(&mut tree);
    let last_content = content(&mut tree);
    let first = tree
        .new_with_children(
            Style {
                display: Display::Block,
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: Dimension::stretch(), height: length(75.0) },
                border: Rect::length(3.0),
                ..Default::default()
            },
            &[first_content],
        )
        .unwrap();
    let wide = tree
        .new_leaf(Style {
            display: Display::Block,
            box_sizing: BoxSizing::ContentBox,
            size: Size { width: length(150.0), height: auto() },
            border: Rect::length(3.0),
            ..Default::default()
        })
        .unwrap();
    let last = tree
        .new_with_children(
            Style {
                display: Display::Block,
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: Dimension::stretch(), height: length(75.0) },
                border: Rect::length(3.0),
                ..Default::default()
            },
            &[last_content],
        )
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                box_sizing: BoxSizing::ContentBox,
                flex_direction: FlexDirection::Column,
                flex_wrap: FlexWrap::Wrap,
                align_content: Some(AlignContent::START),
                size: Size { width: Dimension::min_content(), height: length(100.0) },
                max_size: Size { width: length(100.0), height: auto() },
                border: Rect::length(3.0),
                ..Default::default()
            },
            &[first, wide, last],
        )
        .unwrap();

    tree.compute_layout(
        parent,
        Size { width: AvailableSpace::Definite(500.0), height: AvailableSpace::Definite(500.0) },
    )
    .unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.width, 106.0);
    assert_eq!(tree.layout(first).unwrap().size.width, 156.0);
    assert_eq!(tree.layout(last).unwrap().size.width, 100.0);
}

#[test]
fn definite_flex_cross_stretch_transfers_into_the_flex_base_size() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::stretch(), height: auto() },
            aspect_ratio: Some(1.0),
            min_size: Size::from_lengths(0.0, 0.0),
            align_self: Some(AlignSelf::START),
            ..Default::default()
        })
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size::from_lengths(200.0, 200.0),
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 200.0, height: 200.0 });
}

#[test]
fn indefinite_flex_minimum_stretch_is_zero_without_an_automatic_minimum() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style { display: Display::Block, size: Size::from_lengths(20.0, 200.0), ..Default::default() })
        .unwrap();
    let child = tree
        .new_with_children(
            Style {
                display: Display::Block,
                min_size: Size { width: auto(), height: Dimension::stretch() },
                flex_basis: length(0.0),
                ..Default::default()
            },
            &[content],
        )
        .unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size { width: length(200.0), height: auto() },
                min_size: Size { width: auto(), height: length(100.0) },
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(parent).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(child).unwrap().size.height, 0.0);
}

#[test]
fn changed_containing_block_size_invalidates_stretch_layout() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(20.0), height: Dimension::stretch() },
            ..Default::default()
        })
        .unwrap();
    let parent = tree
        .new_with_children(
            Style { display: Display::FlowRoot, size: Size::from_lengths(100.0, 100.0), ..Default::default() },
            &[child],
        )
        .unwrap();

    tree.compute_layout(
        parent,
        Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(100.0) },
    )
    .unwrap();
    assert_eq!(tree.layout(child).unwrap().size.height, 100.0);

    tree.set_style(
        parent,
        Style { display: Display::FlowRoot, size: Size::from_lengths(100.0, 60.0), ..Default::default() },
    )
    .unwrap();
    tree.compute_layout(
        parent,
        Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(100.0) },
    )
    .unwrap();
    assert_eq!(tree.layout(child).unwrap().size.height, 60.0);
}

fn layout_absolute_stretch(parent_display: Display, inset: Rect<LengthPercentageAuto>) -> Layout {
    layout_nested_stretch(
        Style {
            display: parent_display,
            box_sizing: BoxSizing::ContentBox,
            size: Size::from_lengths(40.0, 50.0),
            padding: Rect { left: length(3.0), right: length(3.0), top: length(5.0), bottom: length(5.0) },
            ..Default::default()
        },
        Style {
            display: Display::Block,
            position: Position::Absolute,
            inset,
            box_sizing: BoxSizing::ContentBox,
            size: Size::from_lengths(20.0, 55.0),
            max_size: Size { width: auto(), height: Dimension::stretch() },
            margin: Rect { left: zero(), right: zero(), top: length(2.0), bottom: length(3.0) },
            padding: Rect::length(2.0),
            border: Rect::length(3.0),
            ..Default::default()
        },
    )
}

#[test]
fn absolute_stretch_uses_insets_static_position_and_padding_box() {
    let zero_insets = layout_absolute_stretch(
        Display::FlowRoot,
        Rect { left: auto(), right: auto(), top: length(0.0), bottom: length(0.0) },
    );
    assert_eq!(zero_insets.size.height, 55.0);

    let static_position = layout_absolute_stretch(Display::FlowRoot, Rect::auto());
    assert_eq!(static_position.size.height, 50.0);

    let start_inset = layout_absolute_stretch(
        Display::FlowRoot,
        Rect { left: auto(), right: auto(), top: length(10.0), bottom: auto() },
    );
    assert_eq!(start_inset.size.height, 45.0);

    let past_edge = layout_absolute_stretch(
        Display::FlowRoot,
        Rect { left: auto(), right: auto(), top: auto(), bottom: length(55.0) },
    );
    assert_eq!(past_edge.size.height, 10.0);
}

#[test]
fn absolute_flex_and_grid_children_resolve_stretch_after_insets() {
    for display in [Display::Flex, Display::Grid] {
        let zero_insets = layout_absolute_stretch(
            display,
            Rect { left: auto(), right: auto(), top: length(0.0), bottom: length(0.0) },
        );
        assert_eq!(zero_insets.size.height, 55.0, "{display:?} zero insets");

        let start_inset =
            layout_absolute_stretch(display, Rect { left: auto(), right: auto(), top: length(10.0), bottom: auto() });
        assert_eq!(start_inset.size.height, 45.0, "{display:?} start inset");

        let past_edge =
            layout_absolute_stretch(display, Rect { left: auto(), right: auto(), top: auto(), bottom: length(55.0) });
        assert_eq!(past_edge.size.height, 10.0, "{display:?} past edge");
    }
}

#[test]
fn stretch_sizes_the_margin_box_when_parent_edges_do_not_collapse() {
    let child = layout_nested_stretch(
        Style { display: Display::FlowRoot, size: Size::from_lengths(100.0, 100.0), ..Default::default() },
        Style {
            display: Display::Block,
            size: Size { width: length(20.0), height: Dimension::stretch() },
            margin: Rect::length(10.0),
            ..Default::default()
        },
    );
    assert_eq!(child.size.height, 80.0);
    assert_eq!(child.location.y, 10.0);
}

#[test]
fn stretch_ignores_margins_at_collapsible_parent_edges() {
    let child = layout_nested_stretch(
        Style { display: Display::Block, size: Size::from_lengths(100.0, 100.0), ..Default::default() },
        Style {
            display: Display::Block,
            size: Size { width: length(20.0), height: Dimension::stretch() },
            margin: Rect::length(10.0),
            ..Default::default()
        },
    );
    assert_eq!(child.size.height, 100.0);
}

#[test]
fn stretch_margin_ignoring_follows_physical_edges_across_writing_modes() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::stretch(), height: length(20.0) },
            margin: Rect { left: length(7.0), right: length(3.0), top: length(1.0), bottom: length(5.0) },
            ..Default::default()
        })
        .unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalRl).unwrap();
    let parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size::from_lengths(50.0, 50.0),
                box_sizing: BoxSizing::ContentBox,
                border: Rect { left: length(0.0), right: length(5.0), top: length(0.0), bottom: length(0.0) },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.set_writing_mode(parent, WritingMode::VerticalRl).unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size::from_lengths(200.0, 200.0), ..Default::default() },
            &[parent],
        )
        .unwrap();
    tree.set_writing_mode(root, WritingMode::VerticalRl).unwrap();

    tree.compute_layout(root, Size { width: AvailableSpace::Definite(200.0), height: AvailableSpace::Definite(200.0) })
        .unwrap();
    assert_eq!(tree.layout(child).unwrap().size.width, 47.0);
}

#[test]
fn definite_stretch_transfers_through_the_preferred_aspect_ratio() {
    let child = layout_nested_stretch(
        Style { display: Display::Block, size: Size::from_lengths(100.0, 100.0), ..Default::default() },
        Style {
            display: Display::Block,
            size: Size { width: auto(), height: Dimension::stretch() },
            aspect_ratio: Some(2.0),
            ..Default::default()
        },
    );
    assert_eq!(child.size, Size { width: 200.0, height: 100.0 });
}
