use taffy::prelude::*;
#[cfg(feature = "float_layout")]
use taffy::Float;
use taffy::{Direction, Point, WritingMode};

fn block_layout(
    writing_mode: WritingMode,
    direction: Direction,
    container_style: Style,
    child_styles: &[Style],
) -> Vec<Layout> {
    let mut tree = TaffyTree::<()>::new();
    let children = child_styles
        .iter()
        .cloned()
        .map(|style| {
            let node = tree.new_leaf(style).unwrap();
            tree.set_writing_mode(node, writing_mode).unwrap();
            node
        })
        .collect::<Vec<_>>();
    let container =
        tree.new_with_children(Style { display: Display::Block, direction, ..container_style }, &children).unwrap();
    tree.set_writing_mode(container, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    children.iter().map(|child| *tree.layout(*child).unwrap()).collect()
}

fn fixed_child(width: f32, height: f32) -> Style {
    Style { display: Display::Block, size: Size { width: length(width), height: length(height) }, ..Style::default() }
}

#[test]
fn vertical_block_flow_stacks_from_block_start() {
    let container = Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() };
    let children = [fixed_child(30.0, 10.0), fixed_child(20.0, 15.0)];

    let vertical_rl = block_layout(WritingMode::VerticalRl, Direction::Ltr, container.clone(), &children);
    assert_eq!(vertical_rl[0].location, Point { x: 70.0, y: 0.0 });
    assert_eq!(vertical_rl[1].location, Point { x: 50.0, y: 0.0 });

    let vertical_lr = block_layout(WritingMode::VerticalLr, Direction::Ltr, container, &children);
    assert_eq!(vertical_lr[0].location, Point { x: 0.0, y: 0.0 });
    assert_eq!(vertical_lr[1].location, Point { x: 30.0, y: 0.0 });
}

#[test]
fn vertical_block_flow_projects_inline_direction() {
    let layouts = block_layout(
        WritingMode::VerticalRl,
        Direction::Rtl,
        Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() },
        &[fixed_child(30.0, 10.0), fixed_child(20.0, 15.0)],
    );

    assert_eq!(layouts[0].location, Point { x: 70.0, y: 190.0 });
    assert_eq!(layouts[1].location, Point { x: 50.0, y: 185.0 });
}

#[test]
fn vertical_block_flow_uses_logical_padding_and_margins() {
    let mut child = fixed_child(20.0, 10.0);
    child.margin = Rect { top: length(3.0), right: length(7.0), ..Rect::zero() };
    let layouts = block_layout(
        WritingMode::VerticalRl,
        Direction::Ltr,
        Style {
            size: Size { width: length(100.0), height: length(200.0) },
            padding: Rect { top: length(5.0), right: length(10.0), ..Rect::zero() },
            ..Style::default()
        },
        &[child],
    );

    assert_eq!(layouts[0].location, Point { x: 63.0, y: 8.0 });
}

#[test]
fn vertical_block_child_stretches_in_the_inline_axis() {
    let layouts = block_layout(
        WritingMode::VerticalLr,
        Direction::Ltr,
        Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() },
        &[Style { display: Display::Block, size: Size { width: length(20.0), height: auto() }, ..Style::default() }],
    );

    assert_eq!(layouts[0].size, Size { width: 20.0, height: 200.0 });
    assert_eq!(layouts[0].location, Point::ZERO);
}

fn layout_orthogonal_percentage_child_in_ratio_parent(percentage: f32) -> (Size<f32>, Size<f32>) {
    let mut tree = TaffyTree::<()>::new();
    let child = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: percent(1.0), height: percent(percentage) },
            ..Style::default()
        })
        .unwrap();
    tree.set_writing_mode(child, WritingMode::VerticalLr).unwrap();

    let ratio_parent = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(100.0), height: auto() },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            &[child],
        )
        .unwrap();
    let body = tree.new_with_children(Style { display: Display::Block, ..Style::default() }, &[ratio_parent]).unwrap();
    let document = tree.new_with_children(Style { display: Display::Block, ..Style::default() }, &[body]).unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(800.0), height: length(600.0) },
                ..Style::default()
            },
            &[document],
        )
        .unwrap();

    tree.compute_layout(root, Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) })
        .unwrap();

    (tree.layout(ratio_parent).unwrap().size, tree.layout(child).unwrap().size)
}

#[test]
fn orthogonal_child_percentages_resolve_against_ratio_derived_parent_block_size() {
    for (percentage, parent_height, child_height) in [(0.5, 100.0, 50.0), (1.0, 100.0, 100.0), (2.0, 200.0, 200.0)] {
        let (parent, child) = layout_orthogonal_percentage_child_in_ratio_parent(percentage);
        assert_eq!(parent, Size { width: 100.0, height: parent_height });
        assert_eq!(child, Size { width: 100.0, height: child_height });
    }
}

#[test]
fn sideways_lr_uses_bottom_inline_start_and_left_block_start() {
    let layouts = block_layout(
        WritingMode::SidewaysLr,
        Direction::Ltr,
        Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() },
        &[fixed_child(30.0, 10.0), fixed_child(20.0, 15.0)],
    );

    assert_eq!(layouts[0].location, Point { x: 0.0, y: 190.0 });
    assert_eq!(layouts[1].location, Point { x: 30.0, y: 185.0 });
}

#[cfg(feature = "float_layout")]
#[test]
fn vertical_floats_use_bfc_line_sides_before_logical_conversion() {
    let mut tree = TaffyTree::<()>::new();
    let float_style = |float| Style {
        display: Display::Block,
        float,
        size: Size { width: length(20.0), height: length(30.0) },
        ..Style::default()
    };
    let line_left = tree.new_leaf(float_style(Float::Left)).unwrap();
    let line_right = tree.new_leaf(float_style(Float::Right)).unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Block,
                direction: Direction::Rtl,
                size: Size { width: length(100.0), height: length(200.0) },
                ..Style::default()
            },
            &[line_left, line_right],
        )
        .unwrap();
    for node in [line_left, line_right, container] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    // BFC line-left/right are the physical top/bottom edges in vertical-rl.
    // They stay direction-agnostic even though RTL reverses logical inline offsets.
    assert_eq!(tree.layout(line_left).unwrap().location, Point { x: 80.0, y: 0.0 });
    assert_eq!(tree.layout(line_right).unwrap().location, Point { x: 80.0, y: 170.0 });
}

#[cfg(feature = "float_layout")]
#[test]
fn orthogonal_block_child_establishes_its_own_float_context() {
    let mut tree = TaffyTree::<()>::new();
    let floated = tree
        .new_leaf(Style {
            display: Display::Block,
            float: Float::Right,
            size: Size { width: length(20.0), height: length(30.0) },
            ..Style::default()
        })
        .unwrap();
    let orthogonal = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(50.0), height: length(100.0) },
                ..Style::default()
            },
            &[floated],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: length(200.0), height: auto() }, ..Style::default() },
            &[orthogonal],
        )
        .unwrap();
    for node in [floated, orthogonal] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(floated).unwrap().location, Point { x: 30.0, y: 70.0 });
}

#[cfg(feature = "float_layout")]
#[test]
fn reversed_block_flow_child_establishes_its_own_float_context() {
    let mut tree = TaffyTree::<()>::new();
    let preceding_float = tree
        .new_leaf(Style {
            display: Display::Block,
            float: Float::Left,
            size: Size { width: length(50.0), height: length(80.0) },
            ..Style::default()
        })
        .unwrap();
    let floated = tree
        .new_leaf(Style {
            display: Display::Block,
            float: Float::Right,
            size: Size { width: length(20.0), height: length(30.0) },
            ..Style::default()
        })
        .unwrap();
    let mode_root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(50.0), height: length(100.0) },
                ..Style::default()
            },
            &[floated],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: length(200.0), height: length(200.0) },
                ..Style::default()
            },
            &[preceding_float, mode_root],
        )
        .unwrap();
    for node in [preceding_float, root] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }
    for node in [floated, mode_root] {
        tree.set_writing_mode(node, WritingMode::VerticalLr).unwrap();
    }

    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(mode_root).unwrap().location, Point { x: 150.0, y: 80.0 });
    assert_eq!(tree.layout(floated).unwrap().location, Point { x: 0.0, y: 70.0 });
}
