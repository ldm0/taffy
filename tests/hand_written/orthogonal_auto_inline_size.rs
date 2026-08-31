use taffy::prelude::*;
use taffy::WritingMode;

fn measure_wrapping_content(
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    _node_id: NodeId,
    node_context: Option<&mut Size<f32>>,
    _style: &Style,
) -> Size<f32> {
    let intrinsic = node_context.copied().unwrap_or(Size { width: 18.0, height: 24.0 });
    Size {
        width: known_dimensions.width.unwrap_or(match available_space.width {
            AvailableSpace::Definite(limit) => intrinsic.width.min(limit),
            AvailableSpace::MinContent | AvailableSpace::MaxContent => intrinsic.width,
        }),
        height: known_dimensions.height.unwrap_or(match available_space.height {
            AvailableSpace::Definite(limit) => intrinsic.height.min(limit),
            AvailableSpace::MinContent => intrinsic.height.min(24.0),
            AvailableSpace::MaxContent => intrinsic.height,
        }),
    }
}

fn orthogonal_tree(intrinsic_inline_size: f32, root_height: Dimension) -> (TaffyTree<Size<f32>>, NodeId, NodeId) {
    let mut tree = TaffyTree::<Size<f32>>::new();
    tree.disable_rounding();
    let child = tree
        .new_leaf_with_context(
            Style { display: Display::Block, ..Default::default() },
            Size { width: 18.0, height: intrinsic_inline_size },
        )
        .unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalLr).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: root_height },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    (tree, root, child)
}

#[test]
fn orthogonal_fallback_constrains_auto_inline_size_without_stretching_it() {
    let (mut tree, root, child) = orthogonal_tree(24.0, auto());
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) },
        measure_wrapping_content,
    )
    .unwrap();

    assert_eq!(tree.layout(child).unwrap().size, Size { width: 18.0, height: 24.0 });
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 100.0, height: 24.0 });
}

#[test]
fn immediate_definite_block_size_precedes_the_initial_containing_block() {
    let (mut tree, root, child) = orthogonal_tree(1000.0, length(300.0));
    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) },
        measure_wrapping_content,
    )
    .unwrap();

    assert_eq!(tree.layout(child).unwrap().size.height, 300.0);
}

#[test]
fn orthogonal_fallback_is_recomputed_when_the_layout_view_resizes() {
    let (mut tree, root, child) = orthogonal_tree(1000.0, auto());
    for viewport_height in [600.0, 400.0] {
        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(viewport_height) },
            measure_wrapping_content,
        )
        .unwrap();
        assert_eq!(tree.layout(child).unwrap().size.height, viewport_height);
    }
}

#[test]
fn orthogonal_fallback_survives_parallel_ancestors_without_becoming_a_size() {
    let mut tree = TaffyTree::<Size<f32>>::new();
    tree.disable_rounding();
    let child = tree
        .new_leaf_with_context(
            Style { display: Display::Block, ..Default::default() },
            Size { width: 18.0, height: 24.0 },
        )
        .unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalRl).unwrap();
    let intermediate =
        tree.new_with_children(Style { display: Display::Block, ..Default::default() }, &[child]).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[intermediate],
        )
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) },
        measure_wrapping_content,
    )
    .unwrap();

    assert_eq!(tree.layout(child).unwrap().size, Size { width: 18.0, height: 24.0 });
    assert_eq!(tree.layout(intermediate).unwrap().size, Size { width: 100.0, height: 24.0 });
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 100.0, height: 24.0 });
}

#[test]
fn parallel_block_children_still_stretch_their_auto_inline_size() {
    let mut tree = TaffyTree::<Size<f32>>::new();
    tree.disable_rounding();
    let child = tree
        .new_leaf_with_context(
            Style { display: Display::Block, ..Default::default() },
            Size { width: 18.0, height: 24.0 },
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                ..Default::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) },
        measure_wrapping_content,
    )
    .unwrap();

    assert_eq!(tree.layout(child).unwrap().size.width, 100.0);
}
