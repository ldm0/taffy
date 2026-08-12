use taffy::prelude::*;
use taffy::WritingMode;

fn measure_wrapping_content(
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    _node_id: NodeId,
    _node_context: Option<&mut ()>,
    _style: &Style,
) -> Size<f32> {
    let resolve_axis = |known: Option<f32>, available: AvailableSpace| {
        known.unwrap_or_else(|| match available {
            AvailableSpace::Definite(value) => value.min(1000.0),
            AvailableSpace::MinContent => 50.0,
            AvailableSpace::MaxContent => 1000.0,
        })
    };

    Size {
        width: resolve_axis(known_dimensions.width, available_space.width),
        height: resolve_axis(known_dimensions.height, available_space.height),
    }
}

fn layout_orthogonal_leaf(
    root_writing_mode: WritingMode,
    child_writing_mode: WritingMode,
    root_size: Size<Dimension>,
) -> (Layout, Layout) {
    let mut tree = TaffyTree::<()>::new();
    let child = tree.new_leaf_with_context(Style { display: Display::Block, ..Default::default() }, ()).unwrap();
    tree.set_writing_mode(child, child_writing_mode).unwrap();
    let root = tree
        .new_with_children(Style { display: Display::Block, size: root_size, ..Default::default() }, &[child])
        .unwrap();
    tree.set_writing_mode(root, root_writing_mode).unwrap();

    tree.compute_layout_with_measure(
        root,
        Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) },
        measure_wrapping_content,
    )
    .unwrap();

    (*tree.layout(root).unwrap(), *tree.layout(child).unwrap())
}

#[test]
fn orthogonal_auto_inline_size_uses_initial_containing_block_fallback() {
    let (root, child) = layout_orthogonal_leaf(
        WritingMode::HorizontalTb,
        WritingMode::VerticalLr,
        Size { width: length(800.0), height: auto() },
    );

    assert_eq!(child.size, Size { width: 800.0, height: 600.0 });
    assert_eq!(root.size, Size { width: 800.0, height: 600.0 });
}

#[test]
fn definite_parent_block_size_precedes_initial_containing_block_fallback() {
    let (root, child) = layout_orthogonal_leaf(
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        Size { width: length(800.0), height: length(300.0) },
    );

    assert_eq!(child.size, Size { width: 800.0, height: 300.0 });
    assert_eq!(root.size, Size { width: 800.0, height: 300.0 });
}

#[test]
fn orthogonal_fallback_follows_the_parents_physical_block_axis() {
    let (root, child) = layout_orthogonal_leaf(
        WritingMode::VerticalRl,
        WritingMode::HorizontalTb,
        Size { width: auto(), height: length(600.0) },
    );

    assert_eq!(child.size, Size { width: 800.0, height: 600.0 });
    assert_eq!(root.size, Size { width: 800.0, height: 600.0 });
}

#[test]
fn initial_containing_block_fallback_survives_parallel_ancestors() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree.new_leaf_with_context(Style { display: Display::Block, ..Default::default() }, ()).unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalLr).unwrap();
    let intermediate =
        tree.new_with_children(Style { display: Display::Block, ..Default::default() }, &[child]).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(800.0), height: auto() },
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

    assert_eq!(tree.layout(child).unwrap().size, Size { width: 800.0, height: 600.0 });
    assert_eq!(tree.layout(intermediate).unwrap().size, Size { width: 800.0, height: 600.0 });
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 800.0, height: 600.0 });
}

#[test]
fn parallel_writing_modes_do_not_use_the_orthogonal_fallback() {
    let (root, child) = layout_orthogonal_leaf(
        WritingMode::HorizontalTb,
        WritingMode::HorizontalTb,
        Size { width: length(800.0), height: auto() },
    );

    assert_eq!(child.size, Size { width: 800.0, height: 1000.0 });
    assert_eq!(root.size, Size { width: 800.0, height: 1000.0 });
}

#[test]
fn orthogonal_fallback_recomputes_after_initial_containing_block_resize() {
    let mut tree = TaffyTree::<()>::new();
    let child = tree.new_leaf_with_context(Style { display: Display::Block, ..Default::default() }, ()).unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalLr).unwrap();
    let intermediate =
        tree.new_with_children(Style { display: Display::Block, ..Default::default() }, &[child]).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(800.0), height: auto() },
                ..Default::default()
            },
            &[intermediate],
        )
        .unwrap();

    for viewport_height in [600.0, 400.0] {
        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(viewport_height) },
            measure_wrapping_content,
        )
        .unwrap();

        assert_eq!(tree.layout(child).unwrap().size.height, viewport_height);
        assert_eq!(tree.layout(intermediate).unwrap().size.height, viewport_height);
        assert_eq!(tree.layout(root).unwrap().size.height, viewport_height);
    }
}
