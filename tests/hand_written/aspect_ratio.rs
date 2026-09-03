use super::test_tree::{TestNode, TestTree};
use taffy::prelude::*;
#[cfg(feature = "float_layout")]
use taffy::Float;
use taffy::{
    AutoSizeBehavior, Overflow, Point, RequestedAxis, ResolvedAspectRatio, SizingMode, SizingPurpose, WritingMode,
};

#[test]
fn resolved_aspect_ratio_rejects_invalid_values() {
    for ratio in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(ResolvedAspectRatio::new(ratio, BoxSizing::ContentBox), None);
    }
}

#[test]
fn resolved_aspect_ratio_sizing_box_flows_through_block_flex_and_grid_items() {
    let edges = Rect { left: length(5.0), right: length(5.0), top: length(5.0), bottom: length(5.0) };
    let ratio_child = |sizing_box| {
        let mut node = TestNode::leaf(
            Style {
                box_sizing: BoxSizing::BorderBox,
                size: Size { width: length(100.0), height: auto() },
                padding: edges,
                border: edges,
                // Deliberately disagree with the node-level used ratio. This
                // proves each algorithm queries the integration seam instead
                // of reconstructing the ratio from Style.
                aspect_ratio: Some(4.0),
                align_self: Some(AlignSelf::FLEX_START),
                justify_self: Some(AlignSelf::FLEX_START),
                ..Style::default()
            },
            Size::ZERO,
        );
        node.resolved_aspect_ratio = ResolvedAspectRatio::new(2.0, sizing_box);
        node
    };

    for display in [Display::Block, Display::Flex, Display::Grid] {
        let root = TestNode::container(
            display,
            Style { size: Size { width: length(400.0), height: length(400.0) }, ..Style::default() },
            Rect::ZERO,
        );
        let mut tree = TestTree::new(root, ratio_child(BoxSizing::ContentBox));
        tree.nodes.push(ratio_child(BoxSizing::BorderBox));
        tree.nodes[0].children.push(2);
        tree.compute(Size::MAX_CONTENT);

        assert_eq!(tree.layout(1).size, Size { width: 100.0, height: 60.0 }, "{display:?} content-box ratio");
        assert_eq!(tree.layout(2).size, Size { width: 100.0, height: 50.0 }, "{display:?} border-box ratio");
    }
}

#[test]
fn border_box_insets_are_normalized_before_ratio_sizing() {
    let layout = |size: Size<Dimension>, max_size: Size<Dimension>, ratio| {
        let mut tree = TaffyTree::<()>::new();
        let node = tree
            .new_leaf(Style {
                display: Display::Block,
                box_sizing: BoxSizing::BorderBox,
                size,
                max_size,
                border: Rect { left: length(20.0), right: length(20.0), top: length(20.0), bottom: length(20.0) },
                aspect_ratio: Some(ratio),
                ..Style::default()
            })
            .unwrap();

        tree.compute_layout(node, Size::MAX_CONTENT).unwrap();
        tree.layout(node).unwrap().size
    };

    assert_eq!(
        layout(Size { width: auto(), height: length(20.0) }, Size::AUTO, 2.0,),
        Size { width: 80.0, height: 40.0 }
    );
    assert_eq!(
        layout(Size::AUTO, Size { width: auto(), height: length(20.0) }, 2.0,),
        Size { width: 80.0, height: 40.0 }
    );
    assert_eq!(
        layout(Size { width: length(20.0), height: auto() }, Size::AUTO, 0.5,),
        Size { width: 40.0, height: 80.0 }
    );
    assert_eq!(
        layout(Size::AUTO, Size { width: length(20.0), height: auto() }, 0.5,),
        Size { width: 40.0, height: 80.0 }
    );
}

#[test]
fn grid_intrinsic_probes_keep_item_owned_sizes_out_of_known_dimensions() {
    let grid = TestNode::container(
        Display::Grid,
        Style {
            size: Size { width: length(100.0), height: auto() },
            grid_template_columns: vec![auto()],
            ..Style::default()
        },
        Rect::ZERO,
    );
    let item = TestNode::leaf(
        Style {
            item_is_replaced: true,
            size: Size { width: length(200.0), height: auto() },
            max_size: Size { width: percent(1.0), height: auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        },
        Size { width: 500.0, height: 500.0 },
    );
    let mut tree = TestTree::new(grid, item);

    tree.compute(Size::MAX_CONTENT);

    let intrinsic_inputs: Vec<_> = tree
        .layout_inputs
        .iter()
        .filter_map(|(node, input)| {
            (*node == 1
                && input.sizing_mode == SizingMode::InherentSize
                && input.sizing_purpose == SizingPurpose::IntrinsicContribution
                && input.axis == RequestedAxis::Horizontal)
                .then_some(*input)
        })
        .collect();
    assert!(
        intrinsic_inputs.iter().any(|input| input.available_space.width == AvailableSpace::MinContent),
        "Grid must request the item's min-content contribution"
    );
    assert!(
        intrinsic_inputs.iter().any(|input| input.available_space.width == AvailableSpace::MaxContent),
        "Grid must request the item's max-content contribution"
    );
    assert!(
        intrinsic_inputs.iter().all(|input| input.known_dimensions == Size::NONE),
        "authored item sizes must remain child-owned during Grid intrinsic probes: {intrinsic_inputs:#?}"
    );
    assert!(
        intrinsic_inputs.iter().all(|input| {
            input.inline_auto_behavior == AutoSizeBehavior::FitContent
                && input.block_auto_behavior == AutoSizeBehavior::FitContent
        }),
        "Grid must carry replaced-aware auto-size policy instead of pre-resolving it"
    );
}

#[test]
fn grid_auto_repeat_uses_final_ratio_resolved_size_under_max_content_constraint() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let item = tree.new_leaf(Style::default()).unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                min_size: Size { width: auto(), height: length(60.0) },
                aspect_ratio: Some(1.0),
                grid_template_columns: vec![repeat(RepetitionCount::AutoFill, vec![length(50.0)])],
                ..Style::default()
            },
            &[item],
        )
        .unwrap();

    tree.compute_layout(grid, Size::MAX_CONTENT).unwrap();

    // The 60px block-axis minimum transfers through the 1:1 ratio. Grid then
    // needs two 50px auto-repeat tracks, and the final 100px inline size
    // transfers back to the block axis. Eager shared fit-content resolution
    // would stop after the first 60px ratio transfer.
    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 100.0, height: 100.0 });
}

fn layout_block_ratio_item(writing_mode: WritingMode, mut item_style: Style, mut content_style: Style) -> Size<f32> {
    let mut tree = TaffyTree::<()>::new();
    item_style.display = Display::Block;
    content_style.display = Display::Block;
    let content = tree.new_leaf(content_style).unwrap();
    let item = tree.new_with_children(item_style, &[content]).unwrap();
    let container = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: length(300.0), height: auto() }, ..Style::default() },
            &[item],
        )
        .unwrap();
    tree.set_writing_mode(content, writing_mode).unwrap();
    tree.set_writing_mode(item, writing_mode).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();
    tree.layout(item).unwrap().size
}

fn horizontal_ratio_item_style() -> Style {
    Style { size: Size { width: auto(), height: length(100.0) }, aspect_ratio: Some(0.5), ..Style::default() }
}

fn horizontal_ratio_content_style() -> Style {
    Style { size: Size { width: length(100.0), height: auto() }, ..Style::default() }
}

/// Regression for WPT css/css-sizing/aspect-ratio/block-aspect-ratio-015.html.
#[test]
fn ratio_dependent_inline_size_observes_the_content_based_automatic_minimum() {
    let size = layout_block_ratio_item(
        WritingMode::HorizontalTb,
        horizontal_ratio_item_style(),
        horizontal_ratio_content_style(),
    );

    assert_eq!(size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn ratio_dependent_automatic_minimum_contributes_to_a_content_sized_parent() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: length(100.0), height: auto() },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: auto(), height: length(100.0) },
                aspect_ratio: Some(0.5),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree.new_with_children(Style { display: Display::Block, ..Style::default() }, &[item]).unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(container).unwrap().size.width, 100.0);
    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-grid/alignment/grid-content-distribution-029.html.
#[test]
fn grid_ratio_block_size_observes_its_content_based_automatic_minimum() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();

    let item = tree
        .new_leaf(Style { size: Size { width: length(100.0), height: length(100.0) }, ..Style::default() })
        .unwrap();
    let grid = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size { width: length(100.0), height: auto() },
                max_size: Size { width: length(50.0), height: auto() },
                aspect_ratio: Some(2.0),
                align_content: Some(AlignContent::CENTER),
                ..Style::default()
            },
            &[item],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style { display: Display::Block, size: Size { width: length(800.0), height: auto() }, ..Style::default() },
            &[grid],
        )
        .unwrap();

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(grid).unwrap().size, Size { width: 50.0, height: 100.0 });
    assert_eq!(tree.layout(item).unwrap().location, Point::ZERO);
}

/// The automatic-minimum rule is logical-axis based, not width-specific.
/// This is the vertical counterpart of WPT block-aspect-ratio-015.
#[test]
fn vertical_ratio_dependent_inline_size_observes_the_content_based_automatic_minimum() {
    let size = layout_block_ratio_item(
        WritingMode::VerticalLr,
        Style { size: Size { width: length(100.0), height: auto() }, aspect_ratio: Some(2.0), ..Style::default() },
        Style { size: Size { width: auto(), height: length(100.0) }, ..Style::default() },
    );

    assert_eq!(size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn authored_inline_constraints_control_the_ratio_dependent_automatic_minimum() {
    let mut capped = horizontal_ratio_item_style();
    capped.max_size.width = length(80.0);
    let capped_size = layout_block_ratio_item(WritingMode::HorizontalTb, capped, horizontal_ratio_content_style());
    assert_eq!(capped_size, Size { width: 80.0, height: 100.0 });

    let mut disabled = horizontal_ratio_item_style();
    disabled.min_size.width = length(0.0);
    let disabled_size = layout_block_ratio_item(WritingMode::HorizontalTb, disabled, horizontal_ratio_content_style());
    assert_eq!(disabled_size, Size { width: 50.0, height: 100.0 });

    let transferred_maximum = Style {
        size: Size { width: auto(), height: length(200.0) },
        max_size: Size { width: auto(), height: length(100.0) },
        aspect_ratio: Some(0.5),
        ..Style::default()
    };
    let transferred_maximum_size =
        layout_block_ratio_item(WritingMode::HorizontalTb, transferred_maximum, horizontal_ratio_content_style());
    assert_eq!(transferred_maximum_size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn scroll_and_replaced_boxes_bypass_the_non_replaced_automatic_minimum() {
    let mut scroll_container = horizontal_ratio_item_style();
    scroll_container.overflow.x = Overflow::Hidden;
    let scroll_size =
        layout_block_ratio_item(WritingMode::HorizontalTb, scroll_container, horizontal_ratio_content_style());
    assert_eq!(scroll_size, Size { width: 50.0, height: 100.0 });

    let mut replaced = horizontal_ratio_item_style();
    replaced.item_is_replaced = true;
    let replaced_size = layout_block_ratio_item(WritingMode::HorizontalTb, replaced, horizontal_ratio_content_style());
    assert_eq!(replaced_size, Size { width: 50.0, height: 100.0 });

    let mut clipped = horizontal_ratio_item_style();
    clipped.overflow.x = Overflow::Clip;
    let clipped_size = layout_block_ratio_item(WritingMode::HorizontalTb, clipped, horizontal_ratio_content_style());
    assert_eq!(clipped_size, Size { width: 100.0, height: 100.0 });
}

#[cfg(feature = "float_layout")]
#[test]
fn floated_ratio_dependent_box_observes_the_same_automatic_minimum() {
    let mut floated = horizontal_ratio_item_style();
    floated.float = Float::Left;
    let size = layout_block_ratio_item(WritingMode::HorizontalTb, floated, horizontal_ratio_content_style());

    assert_eq!(size, Size { width: 100.0, height: 100.0 });
}

#[test]
fn vertical_intrinsic_inline_keyword_uses_the_ratio_content_contribution() {
    let mut tree = TaffyTree::<()>::new();
    let content = tree
        .new_leaf(Style {
            display: Display::Block,
            size: Size { width: Dimension::length(0.0), height: Dimension::length(0.0) },
            ..Style::default()
        })
        .unwrap();
    let item = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: Dimension::length(100.0), height: Dimension::min_content() },
                aspect_ratio: Some(1.0),
                ..Style::default()
            },
            &[content],
        )
        .unwrap();
    let container = tree
        .new_with_children(
            Style {
                display: Display::Block,
                size: Size { width: Dimension::length(300.0), height: Dimension::length(300.0) },
                ..Style::default()
            },
            &[item],
        )
        .unwrap();
    for node in [container, item, content] {
        tree.set_writing_mode(node, WritingMode::VerticalLr).unwrap();
    }

    tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

    assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 });
}

/// Regression for WPT css/css-sizing/aspect-ratio/block-aspect-ratio-021.html.
#[test]
fn opposite_axis_maximum_constrains_intrinsic_inline_size_in_layout_algorithms() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let mut tree = TaffyTree::<()>::new();
        let content = tree
            .new_leaf(Style {
                display: Display::Block,
                size: Size { width: length(200.0), height: auto() },
                ..Style::default()
            })
            .unwrap();
        let item = tree
            .new_with_children(
                Style {
                    display: Display::Block,
                    size: Size { width: Dimension::max_content(), height: auto() },
                    max_size: Size { width: auto(), height: length(100.0) },
                    aspect_ratio: Some(1.0),
                    align_self: Some(AlignSelf::FLEX_START),
                    justify_self: Some(AlignSelf::FLEX_START),
                    ..Style::default()
                },
                &[content],
            )
            .unwrap();
        let container = tree
            .new_with_children(
                Style {
                    display,
                    size: Size { width: length(300.0), height: auto() },
                    align_items: Some(AlignItems::FLEX_START),
                    justify_items: Some(AlignItems::FLEX_START),
                    ..Style::default()
                },
                &[item],
            )
            .unwrap();

        tree.compute_layout(container, Size::MAX_CONTENT).unwrap();

        assert_eq!(tree.layout(item).unwrap().size, Size { width: 100.0, height: 100.0 }, "{display:?}");
    }
}
