use taffy::prelude::*;
use taffy::WritingMode;

fn fixed_size(width: f32, height: f32) -> Size<Dimension> {
    Size { width: length(width), height: length(height) }
}

fn vertical_margins(top: f32, bottom: f32) -> Rect<LengthPercentageAuto> {
    Rect { top: length(top), bottom: length(bottom), ..Rect::zero() }
}

fn nested_parent(
    tree: &mut TaffyTree<()>,
    parent_style: Style,
    child: NodeId,
    writing_mode: WritingMode,
) -> (NodeId, NodeId) {
    let parent = tree.new_with_children(parent_style, &[child]).unwrap();
    tree.set_writing_mode(parent, writing_mode).unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: fixed_size(400.0, 400.0), ..Style::default() },
            &[parent],
        )
        .unwrap();
    tree.set_writing_mode(root, writing_mode).unwrap();
    (root, parent)
}

#[test]
fn definite_block_containing_size_resolves_preferred_minimum_and_maximum_stretch() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        for (size, min_size, max_size, content_height) in [
            (Size { width: length(20.0), height: Dimension::stretch() }, Size::auto(), Size::auto(), 20.0),
            (Size::auto(), Size { width: Dimension::auto(), height: Dimension::stretch() }, Size::auto(), 20.0),
            (
                fixed_size(20.0, 500.0),
                Size::auto(),
                Size { width: Dimension::auto(), height: Dimension::stretch() },
                120.0,
            ),
        ] {
            let mut tree = TaffyTree::<()>::new();
            let content = tree.new_leaf(Style { size: fixed_size(20.0, content_height), ..Style::default() }).unwrap();
            let child = tree
                .new_with_children(Style { display, size, min_size, max_size, ..Style::default() }, &[content])
                .unwrap();
            let (root, _) = nested_parent(
                &mut tree,
                Style { display: Display::Block, size: fixed_size(200.0, 100.0), ..Style::default() },
                child,
                WritingMode::HorizontalTb,
            );

            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

            assert_eq!(tree.layout(child).unwrap().size.height, 100.0, "{display:?}");
        }
    }
}

#[test]
fn indefinite_block_containing_size_keeps_stretch_content_based() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree.new_leaf(Style { size: fixed_size(20.0, 20.0), ..Style::default() }).unwrap();
    let child = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(20.0), height: Dimension::stretch() },
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let (root, parent) = nested_parent(
        &mut tree,
        Style {
            display: Display::Block,
            size: Size { width: length(200.0), height: Dimension::auto() },
            ..Style::default()
        },
        child,
        WritingMode::HorizontalTb,
    );

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().size.height, 20.0);
    assert_eq!(tree.layout(parent).unwrap().size.height, 20.0);
}

#[test]
fn min_and_max_constraints_do_not_make_an_auto_containing_block_definite() {
    for (min_height, max_height, content_height, expected_parent_height) in
        [(length(100.0), Dimension::auto(), 20.0, 100.0), (Dimension::auto(), length(100.0), 120.0, 100.0)]
    {
        let mut tree = TaffyTree::<()>::new();
        let content = tree.new_leaf(Style { size: fixed_size(20.0, content_height), ..Style::default() }).unwrap();
        let child = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: length(20.0), height: Dimension::stretch() },
                    ..Style::default()
                },
                &[content],
            )
            .unwrap();
        let (root, parent) = nested_parent(
            &mut tree,
            Style {
                display: Display::Block,
                size: Size { width: length(200.0), height: Dimension::auto() },
                min_size: Size { width: Dimension::auto(), height: min_height },
                max_size: Size { width: Dimension::auto(), height: max_height },
                ..Style::default()
            },
            child,
            WritingMode::HorizontalTb,
        );

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(child).unwrap().size.height, content_height);
        assert_eq!(tree.layout(parent).unwrap().size.height, expected_parent_height);
    }
}

#[test]
fn collapsed_parent_edges_are_ignored_only_for_explicit_stretch() {
    let cases = [
        (Rect::zero(), Rect::zero(), 100.0),
        (Rect { bottom: length(5.0), ..Rect::zero() }, Rect::zero(), 90.0),
        (Rect::zero(), Rect { top: length(5.0), ..Rect::zero() }, 90.0),
    ];

    for (border, padding, expected_height) in cases {
        let mut tree = TaffyTree::<()>::new();
        let child = tree
            .new_leaf(Style {
                display: Display::Block,
                size: Size { width: length(20.0), height: Dimension::stretch() },
                margin: vertical_margins(10.0, 10.0),
                ..Style::default()
            })
            .unwrap();
        let (root, _) = nested_parent(
            &mut tree,
            Style {
                display: Display::Block,
                box_sizing: BoxSizing::ContentBox,
                size: fixed_size(200.0, 100.0),
                border,
                padding,
                ..Style::default()
            },
            child,
            WritingMode::HorizontalTb,
        );

        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(child).unwrap().size.height, expected_height);
    }
}

#[test]
fn a_new_block_formatting_context_accounts_for_both_stretch_margins() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(20.0), height: Dimension::stretch() },
            margin: vertical_margins(10.0, 10.0),
            ..Style::default()
        })
        .unwrap();
    let parent = tree
        .new_with_children(
            Style { display: Display::Block, size: fixed_size(200.0, 100.0), ..Style::default() },
            &[child],
        )
        .unwrap();

    tree.compute_layout(parent, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().size.height, 80.0);
}

#[test]
fn block_stretch_is_definite_for_percentage_descendants() {
    let mut tree = TaffyTree::<()>::new();
    let percentage_child =
        tree.new_leaf(Style { size: Size { width: length(20.0), height: percent(0.5) }, ..Style::default() }).unwrap();
    let stretched = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(20.0), height: Dimension::stretch() },
                ..Style::default()
            },
            &[percentage_child],
        )
        .unwrap();
    let (root, _) = nested_parent(
        &mut tree,
        Style { display: Display::Block, size: fixed_size(200.0, 100.0), ..Style::default() },
        stretched,
        WritingMode::HorizontalTb,
    );

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(stretched).unwrap().size.height, 100.0);
    assert_eq!(tree.layout(percentage_child).unwrap().size.height, 50.0);
}

#[test]
fn block_stretch_resolves_before_preferred_aspect_ratio_transfer() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::auto(), height: Dimension::stretch() },
            aspect_ratio: Some(2.0),
            ..Style::default()
        })
        .unwrap();
    let (root, _) = nested_parent(
        &mut tree,
        Style { display: Display::Block, size: fixed_size(300.0, 100.0), ..Style::default() },
        child,
        WritingMode::HorizontalTb,
    );

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().size, Size { width: 200.0, height: 100.0 });
}

#[test]
fn vertical_block_stretch_uses_the_physical_collapsing_edge_mask() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::stretch(), height: length(20.0) },
            margin: Rect { left: length(7.0), right: length(3.0), top: zero(), bottom: zero() },
            ..Style::default()
        })
        .unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalRl).unwrap();
    let (root, _) = nested_parent(
        &mut tree,
        Style {
            display: Display::Block,
            box_sizing: BoxSizing::ContentBox,
            size: fixed_size(50.0, 50.0),
            border: Rect { right: length(5.0), ..Rect::zero() },
            ..Style::default()
        },
        child,
        WritingMode::VerticalRl,
    );

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().size.width, 47.0);
}
