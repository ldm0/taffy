use taffy::prelude::*;
#[cfg(feature = "float_layout")]
use taffy::Float;
use taffy::{Direction, LogicalSize, Point, WritingMode};

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

#[test]
fn block_root_stretches_only_its_inline_axis_in_definite_space() {
    for (mode, expected_size, expected_location) in [
        (WritingMode::HorizontalTb, Size { width: 100.0, height: 30.0 }, Point::ZERO),
        (WritingMode::VerticalRl, Size { width: 20.0, height: 200.0 }, Point { x: 80.0, y: 0.0 }),
        (WritingMode::VerticalLr, Size { width: 20.0, height: 200.0 }, Point::ZERO),
        (WritingMode::SidewaysRl, Size { width: 20.0, height: 200.0 }, Point { x: 80.0, y: 0.0 }),
        (WritingMode::SidewaysLr, Size { width: 20.0, height: 200.0 }, Point::ZERO),
    ] {
        let mut tree = TaffyTree::<()>::new();
        let child = tree.new_leaf(fixed_child(20.0, 30.0)).unwrap();
        let root = tree.new_with_children(Style { display: Display::Block, ..Style::default() }, &[child]).unwrap();
        for node in [child, root] {
            tree.set_writing_mode(node, mode).unwrap();
        }
        tree.compute_layout(
            root,
            Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(200.0) },
        )
        .unwrap();
        assert_eq!(tree.layout(root).unwrap().size, expected_size, "{mode:?}");
        assert_eq!(tree.layout(root).unwrap().location, expected_location, "{mode:?}");
    }
}

#[test]
fn vertical_sibling_margins_collapse_positive_and_negative_struts() {
    for (mode, expected_x) in
        [(WritingMode::VerticalRl, [60.0, 20.0, 5.0]), (WritingMode::VerticalLr, [10.0, 60.0, 75.0])]
    {
        let direction = taffy::WritingDirection::new(mode, Direction::Ltr);
        let mut children = [fixed_child(30.0, 20.0), fixed_child(20.0, 20.0), fixed_child(20.0, 20.0)];
        for (child, (start, end)) in children.iter_mut().zip([(10.0, 15.0), (20.0, -5.0), (-3.0, 0.0)]) {
            child.margin = direction.to_physical_box_strut(taffy::LogicalBoxStrut {
                inline_start: length(0.0),
                inline_end: length(0.0),
                block_start: length(start),
                block_end: length(end),
            });
        }
        let layouts = block_layout(
            mode,
            Direction::Ltr,
            Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() },
            &children,
        );
        for (layout, x) in layouts.iter().zip(expected_x) {
            assert_eq!(layout.location, Point { x, y: 0.0 }, "{mode:?}");
        }
    }
}

#[test]
fn empty_vertical_leaf_collapses_through_its_block_axis() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let writing_direction = taffy::WritingDirection::new(mode, Direction::Ltr);
        let mut empty = fixed_child(0.0, 20.0);
        empty.margin = writing_direction.to_physical_box_strut(taffy::LogicalBoxStrut {
            inline_start: length(0.0),
            inline_end: length(0.0),
            block_start: length(30.0),
            block_end: length(40.0),
        });
        let layouts = block_layout(
            mode,
            Direction::Ltr,
            Style { size: Size::length(100.0), ..Style::default() },
            &[fixed_child(10.0, 20.0), empty, fixed_child(10.0, 20.0)],
        );
        assert_eq!(layouts[2].location.x, if mode == WritingMode::VerticalRl { 40.0 } else { 50.0 });
    }
}

#[test]
fn block_alignment_shifts_the_stack_in_the_block_axis() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr, WritingMode::SidewaysRl, WritingMode::SidewaysLr] {
        for (alignment, expected_block_offset) in [(AlignContent::CENTER, 35.0), (AlignContent::END, 70.0)] {
            let layouts = block_layout(
                mode,
                Direction::Ltr,
                Style {
                    size: Size { width: length(100.0), height: length(200.0) },
                    align_content: Some(alignment),
                    ..Style::default()
                },
                &[fixed_child(20.0, 10.0), fixed_child(10.0, 10.0)],
            );
            let expected_x =
                if mode.is_block_flow_reversed() { 80.0 - expected_block_offset } else { expected_block_offset };
            let expected_y = if mode == WritingMode::SidewaysLr { 190.0 } else { 0.0 };
            assert_eq!(layouts[0].location, Point { x: expected_x, y: expected_y });
            assert_eq!(
                layouts[1].location.x,
                if mode.is_block_flow_reversed() { expected_x - 10.0 } else { expected_x + 20.0 }
            );
        }
    }
}

#[test]
fn vertical_auto_margins_center_on_inline_axis() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr, WritingMode::SidewaysRl, WritingMode::SidewaysLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let layouts = block_layout(
                mode,
                direction,
                Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() },
                &[Style { margin: Rect::auto(), ..fixed_child(20.0, 40.0) }],
            );
            assert_eq!(
                layouts[0].location,
                Point { x: if mode.is_block_flow_reversed() { 80.0 } else { 0.0 }, y: 80.0 }
            );
            assert_eq!(layouts[0].margin, Rect { top: 80.0, bottom: 80.0, left: 0.0, right: 0.0 });
        }
    }
}

#[test]
fn orthogonal_auto_block_axis_is_content_sized_not_parent_stretched() {
    let mut tree = TaffyTree::<()>::new();
    let grandchild = tree.new_leaf(fixed_child(20.0, 30.0)).unwrap();
    let child = tree.new_with_children(Style { display: Display::Block, ..Style::default() }, &[grandchild]).unwrap();
    let root = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: length(200.0), height: auto() }, ..Style::default() },
            &[child],
        )
        .unwrap();
    for node in [grandchild, child] {
        tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
    }
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 20.0, height: 30.0 });
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 200.0, height: 30.0 });
}

#[test]
fn writing_mode_roots_do_not_export_margins_in_the_wrong_direction() {
    for child_mode in [WritingMode::HorizontalTb, WritingMode::VerticalLr] {
        let mut tree = TaffyTree::<()>::new();
        // A nonempty child forces block layout, whose own margin struts use
        // the child's writing direction rather than the parent's.
        let grandchild = tree.new_leaf(fixed_child(5.0, 5.0)).unwrap();
        let child = tree
            .new_with_children(
                Style {
                    margin: Rect { left: length(2.0), right: length(7.0), top: length(40.0), bottom: length(50.0) },
                    ..fixed_child(20.0, 30.0)
                },
                &[grandchild],
            )
            .unwrap();
        let following = tree.new_leaf(fixed_child(10.0, 10.0)).unwrap();
        let root = tree.new_with_children(fixed_child(100.0, 200.0), &[child, following]).unwrap();
        for node in [root, following] {
            tree.set_writing_mode(node, WritingMode::VerticalRl).unwrap();
        }
        for node in [child, grandchild] {
            tree.set_writing_mode(node, child_mode).unwrap();
        }
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(child).unwrap().location, Point { x: 73.0, y: 40.0 }, "{child_mode:?}");
        assert_eq!(tree.layout(following).unwrap().location, Point { x: 61.0, y: 0.0 }, "{child_mode:?}");
    }
}

#[test]
fn intrinsic_root_inline_keywords_do_not_fill_available_height() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for intrinsic in [Dimension::min_content(), Dimension::max_content(), Dimension::fit_content()] {
            let mut tree = TaffyTree::<()>::new();
            let child = tree.new_leaf(fixed_child(20.0, 30.0)).unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        display: Display::Block,
                        size: Size { width: auto(), height: intrinsic },
                        ..Style::default()
                    },
                    &[child],
                )
                .unwrap();
            for node in [child, root] {
                tree.set_writing_mode(node, mode).unwrap();
            }
            tree.compute_layout(
                root,
                Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(200.0) },
            )
            .unwrap();
            assert_eq!(tree.layout(root).unwrap().size, Size { width: 20.0, height: 30.0 }, "{mode:?} {intrinsic:?}");
        }
    }
}

#[test]
fn vertical_percentage_box_edges_resolve_against_parent_inline_size() {
    let layouts = block_layout(
        WritingMode::VerticalRl,
        Direction::Ltr,
        Style { size: Size { width: length(100.0), height: length(200.0) }, ..Style::default() },
        &[Style {
            box_sizing: BoxSizing::ContentBox,
            padding: Rect::percent(0.05),
            margin: Rect::percent(0.10),
            ..fixed_child(20.0, 30.0)
        }],
    );
    assert_eq!(layouts[0].size, Size { width: 40.0, height: 50.0 });
    assert_eq!(layouts[0].location, Point { x: 40.0, y: 20.0 });
    assert_eq!(layouts[0].padding, Rect::length(10.0));
    assert_eq!(layouts[0].margin, Rect::length(20.0));
}

#[test]
fn vertical_relative_percentage_block_insets_require_a_definite_basis() {
    for (definite, expected_offset) in [(false, 0.0), (true, 50.0)] {
        let layouts = block_layout(
            WritingMode::VerticalRl,
            Direction::Ltr,
            Style {
                size: Size { width: if definite { length(100.0) } else { auto() }, height: length(200.0) },
                min_size: Size { width: length(100.0), height: auto() },
                ..Style::default()
            },
            &[Style { inset: Rect { right: percent(0.5), ..Rect::auto() }, ..fixed_child(20.0, 30.0) }],
        );
        assert_eq!(layouts[0].location.x, 80.0 - expected_offset);
    }
}

#[test]
fn vertical_anonymous_wrappers_forward_content_box_percentage_basis() {
    use super::test_tree::{TestNode, TestTree};
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut root = TestNode::container(
            Display::Block,
            Style {
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: length(200.0), height: length(240.0) },
                padding: Rect::length(10.0),
                border: Rect::length(5.0),
                ..Style::default()
            },
            Rect::ZERO,
        );
        root.writing_mode = mode;
        let mut wrapper = TestNode::container(Display::Block, Style::default(), Rect::ZERO);
        wrapper.writing_mode = mode;
        wrapper.anonymous_block = true;
        wrapper.children.push(2);
        let mut child = TestNode::leaf(
            Style {
                box_sizing: BoxSizing::ContentBox,
                size: Size { width: percent(0.5), height: length(100.0) },
                border: Rect::length(10.0),
                ..Style::default()
            },
            Size::ZERO,
        );
        child.writing_mode = mode;
        let mut tree = TestTree::new(root, wrapper);
        tree.nodes.push(child);
        tree.compute(Size::MAX_CONTENT);
        assert_eq!(tree.layout(2).size, Size { width: 120.0, height: 120.0 });
        assert_eq!(tree.layout(1).size, Size { width: 120.0, height: 240.0 });
    }
}

#[test]
fn vertical_anonymous_intrinsic_measurements_retain_block_dependencies() {
    use super::test_tree::{TestNode, TestTree};
    use taffy::{Cache, LayoutInput, RunMode, SizingMode, SizingPurpose};
    let mode = WritingMode::VerticalRl;
    let mut wrapper = TestNode::container(Display::Block, Style::default(), Rect::ZERO);
    wrapper.writing_mode = mode;
    wrapper.anonymous_block = true;
    let mut child = TestNode::leaf(
        Style { size: Size { width: percent(0.5), height: length(40.0) }, ..Style::default() },
        Size::ZERO,
    );
    child.writing_mode = mode;
    let mut tree = TestTree::new(wrapper, child);
    let mut inputs = LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: SizingMode::InherentSize,
        sizing_purpose: SizingPurpose::IntrinsicContribution,
        parent_writing_mode: mode,
        known_dimensions: mode.to_physical(LogicalSize { inline_size: Some(240.0), block_size: None }),
        parent_size: Size { width: Some(200.0), height: Some(240.0) },
        ..LayoutInput::HIDDEN
    };
    let output = tree.compute_child_layout(NodeId::from(0_usize), inputs);
    assert_eq!(output.size.width, 100.0);
    let mut cache = Cache::new();
    cache.store(&inputs, output);
    assert!(cache.get(&inputs).is_some());
    inputs.parent_size.width = Some(320.0);
    assert!(cache.get(&inputs).is_none());
    assert_eq!(tree.compute_child_layout(NodeId::from(0_usize), inputs).size.width, 160.0);
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

#[cfg(all(feature = "float_layout", feature = "content_size"))]
#[test]
fn block_alignment_keeps_unshifted_float_overflow() {
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut tree = TaffyTree::<()>::new();
        let floated = tree
            .new_leaf(Style {
                display: Display::Block,
                float: Float::Left,
                size: mode.to_physical(LogicalSize { inline_size: length(10.0), block_size: length(150.0) }),
                ..Style::default()
            })
            .unwrap();
        let child = tree
            .new_leaf(Style {
                display: Display::Block,
                size: mode.to_physical(LogicalSize { inline_size: length(20.0), block_size: length(10.0) }),
                ..Style::default()
            })
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size::length(100.0),
                    align_content: Some(AlignContent::START),
                    ..Style::default()
                },
                &[floated, child],
            )
            .unwrap();
        for node in [root, floated, child] {
            tree.set_writing_mode(node, mode).unwrap();
        }
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(mode.to_logical(tree.layout(root).unwrap().content_size).block_size, 150.0, "{mode:?}");
    }
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
