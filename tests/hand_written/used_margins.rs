use taffy::prelude::*;

#[test]
fn block_layout_exports_the_box_own_margin_before_descendant_collapse() {
    let mut tree = TaffyTree::<()>::new();
    let descendant = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(10.0), height: length(10.0) },
            margin: Rect { top: length(50.0), ..Rect::zero() },
            ..Style::default()
        })
        .unwrap();
    let child = tree
        .new_with_children(
            Style { display: Display::Block, margin: Rect { top: length(10.0), ..Rect::zero() }, ..Style::default() },
            &[descendant],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: length(100.0) },
                ..Style::default()
            },
            &[child],
        )
        .unwrap();

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(child).unwrap().margin.top, 10.0);
    assert_eq!(tree.layout(descendant).unwrap().margin.top, 50.0);
}

#[test]
fn grid_layout_keeps_alignment_space_out_of_exported_auto_margins() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: length(40.0), height: length(20.0) },
            margin: Rect::auto(),
            ..Style::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(200.0), height: length(100.0) },
                grid_template_columns: vec![fr(1.0)],
                grid_template_rows: vec![fr(1.0)],
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    let layout = tree.layout(item).unwrap();
    assert_eq!((layout.location.x, layout.location.y), (80.0, 40.0));
    assert_eq!(layout.margin, Rect::zero());
}

#[test]
fn grid_layout_exports_resolved_non_auto_margins() {
    let mut tree = TaffyTree::<()>::new();
    let item = tree
        .new_leaf(Style {
            size: Size { width: length(40.0), height: length(20.0) },
            margin: Rect { top: percent(0.2), right: auto(), bottom: length(-5.0), left: percent(0.1) },
            ..Style::default()
        })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(200.0), height: length(100.0) },
                grid_template_columns: vec![fr(1.0)],
                grid_template_rows: vec![fr(1.0)],
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().margin, Rect { top: 40.0, right: 0.0, bottom: -5.0, left: 20.0 });
}
