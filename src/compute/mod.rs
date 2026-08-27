//! Low-level access to the layout algorithms themselves. For a higher-level API, see the [`TaffyTree`](crate::TaffyTree) struct.
//!
//! ### Layout functions
//!
//! The layout functions all take an [`&mut impl LayoutPartialTree`](crate::LayoutPartialTree) parameter, which represents a single container node and it's direct children.
//!
//! | Function                          | Purpose                                                                                                                                                                                            |
//! | ---                               | ---                                                                                                                                                                                                |
//! | [`compute_flexbox_layout`]        | Layout a Flexbox container and it's direct children                                                                                                                                                |
//! | [`compute_grid_layout`]           | Layout a CSS Grid container and it's direct children                                                                                                                                               |
//! | [`compute_block_layout`]          | Layout a Block container and it's direct children                                                                                                                                                  |
//! | [`compute_leaf_layout`]           | Applies common properties like padding/border/aspect-ratio to a node before deferring to a passed closure to determine it's size. Can be applied to nodes like text or image nodes.                |
//! | [`compute_root_layout`]           | Layout the root node of a tree (regardless of it's layout mode). This function is typically called once to begin a layout run.                                                                     |                                                                      |
//! | [`compute_hidden_layout`]         | Mark a node as hidden during layout (like `Display::None`)                                                                                                                                         |
//! | [`compute_cached_layout`]         | Attempts to find a cached layout for the specified node and layout inputs. Uses the provided closure to compute the layout (and then stores the result in the cache) if no cached layout is found. |
//!
//! ### Other functions
//!
//! | Function                          | Requires                                                                                                                                                                                           | Purpose                                                              |
//! | ---                               | ---                                                                                                                                                                                                | ---                                                                  |
//! | [`round_layout`]                  | [`RoundTree`]                                                                                                                                                                                      | Round a tree of float-valued layouts to integer pixels               |
//! | [`resolve_content_alignment_fallback`] | none                                                                                                                                                                                         | Resolve content-distribution and baseline fallbacks to a position    |
//! | [`compute_content_alignment_offset`] | none                                                                                                                                                                                            | Compute the offset for an already-resolved content alignment         |
//! | [`print_tree`](crate::print_tree) | [`PrintTree`](crate::PrintTree)                                                                                                                                                                    | Print a debug representation of a node tree and it's computed layout |
//!
pub(crate) mod common;
pub(crate) mod leaf;
pub(crate) mod replaced;

#[cfg(feature = "block_layout")]
pub(crate) mod block;

#[cfg(feature = "float_layout")]
pub(crate) mod float;

#[cfg(feature = "flexbox")]
pub(crate) mod flexbox;

#[cfg(feature = "grid")]
pub(crate) mod grid;

pub use leaf::{
    compute_leaf_layout, compute_leaf_layout_with_aspect_ratio, compute_leaf_layout_with_aspect_ratio_and_writing_mode,
    compute_leaf_layout_with_scrollbar_insets, compute_leaf_layout_with_sizing_context, compute_leaf_layout_with_tree,
    LeafSizingContext,
};

pub use replaced::{compute_replaced_layout, ReplacedNaturalSizing, ReplacedSizingContext};

pub use self::common::alignment::{
    apply_alignment_fallback as resolve_content_alignment_fallback,
    compute_alignment_offset as compute_content_alignment_offset, ResolvedAlignContentKeyword,
};
pub use self::common::baseline::FontBaseline;

#[cfg(feature = "block_layout")]
pub use self::block::{compute_block_layout, BlockContext, BlockFormattingContext};

#[cfg(all(feature = "block_layout", feature = "float_layout"))]
pub use self::block::BfcOffset;

#[cfg(feature = "flexbox")]
pub use self::flexbox::compute_flexbox_layout;

#[cfg(feature = "grid")]
pub use self::grid::compute_grid_layout;

#[cfg(feature = "float_layout")]
pub use self::float::{BfcSlot, ContentSlot, FloatContext, FloatIntrinsicWidthCalculator};

use crate::geometry::{Line, LogicalStaticPosition, Point, Size};
use crate::style::{AvailableSpace, CoreStyle};
use crate::tree::{
    ChildLayoutInput, IntrinsicSizeResult, Layout, LayoutInput, LayoutOutput, LayoutPartialTree, LayoutPartialTreeExt,
    NodeId, OutOfFlowContainingBlock, RoundTree, RunMode, SizingMode, SizingPurpose,
};
use crate::util::debug::{debug_log, debug_log_node, debug_pop_node, debug_push_node};
use crate::util::sys::round;
use crate::util::ResolveOrZero;
use crate::{AutoSizeBehavior, CacheTree, RequestedAxis};

#[cfg(feature = "block_layout")]
use crate::{AbsoluteAxis, MaybeMath};

pub use self::common::intrinsic_size::ResolvedNodeSizing;

/// Size and place one absolutely positioned descendant in an explicitly
/// supplied containing area.
///
/// Custom formatting contexts should emit the size-independent static
/// position first, then call this function from the actual containing block.
/// This keeps inset, intrinsic-size, aspect-ratio and auto-margin semantics in
/// the same resolver used by Taffy's block, flex and grid algorithms.
#[cfg(any(feature = "block_layout", feature = "flexbox"))]
pub fn compute_out_of_flow_layout(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    order: u32,
    static_position: LogicalStaticPosition,
    containing_block: OutOfFlowContainingBlock,
) -> Option<Size<f32>> {
    common::absolute::layout_out_of_flow_item(
        tree,
        common::absolute::OutOfFlowItem { node, order, static_position },
        containing_block,
    )
    .map(|output| output.content_size)
}

/// Compute layout for the root node in the tree
pub fn compute_root_layout(tree: &mut impl LayoutPartialTree, root: NodeId, available_space: Size<AvailableSpace>) {
    let root_writing_mode = tree.get_writing_mode(root);
    let root_inputs = LayoutInput {
        run_mode: RunMode::PerformLayout,
        sizing_mode: SizingMode::InherentSize,
        sizing_purpose: SizingPurpose::Layout,
        axis: RequestedAxis::Both,
        inline_auto_behavior: AutoSizeBehavior::FitContent,
        block_auto_behavior: AutoSizeBehavior::FitContent,
        known_dimensions: Size::NONE,
        definite_dimensions: Size::NONE,
        parent_size: available_space.into_options(),
        parent_writing_mode: root_writing_mode,
        available_space,
        block_margins_are_collapsible: Line::FALSE,
        table_cell: None,
    };
    let percentage_basis = root_inputs.constraint_space(root_writing_mode).margin_padding_percentage_basis();
    let (root_padding, root_border) = {
        let style = tree.get_core_container_style(root);
        (
            style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis)),
            style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis)),
        )
    };
    let mut root_available_space = available_space;
    let mut root_inline_auto_behavior = AutoSizeBehavior::FitContent;

    #[cfg(feature = "block_layout")]
    {
        let style = tree.get_core_container_style(root);

        let inline_axis = root_writing_mode.inline_axis();
        if style.is_block() {
            let margin = style.margin().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
            let available_inline_space =
                available_space.get_abs(inline_axis).maybe_sub(margin.grid_axis_sum(inline_axis));
            match inline_axis {
                AbsoluteAxis::Horizontal => root_available_space.width = available_inline_space,
                AbsoluteAxis::Vertical => root_available_space.height = available_inline_space,
            }
            if style.size().get_abs(inline_axis).is_auto() {
                root_inline_auto_behavior = AutoSizeBehavior::StretchImplicit;
            }
        }
    }

    // Recursively compute node layout
    let output = tree.perform_child_layout(
        root,
        ChildLayoutInput::new(
            Size::NONE,
            available_space.into_options(),
            root_writing_mode,
            root_available_space,
            SizingMode::InherentSize,
            Line::FALSE,
        )
        .with_inline_auto_behavior(root_inline_auto_behavior),
    );
    let scrollbar_size = tree.get_scrollbar_insets(root).sum_axes();
    let style = tree.get_core_container_style(root);
    let margin = style.margin().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let location = Point {
        x: if style.direction().is_rtl() {
            available_space.width.into_option().map_or(0.0, |available_width| available_width - output.size.width)
        } else {
            0.0
        },
        y: 0.0,
    };
    drop(style);

    tree.set_unrounded_layout(
        root,
        &Layout {
            order: 0,
            location,
            size: output.size,
            #[cfg(feature = "content_size")]
            content_size: output.content_size,
            scrollbar_size,
            padding: root_padding,
            border: root_border,
            // TODO: support auto margins for root node?
            margin,
        },
    );
}

/// Attempts to find a cached layout for the specified node and layout inputs.
///
/// Uses the provided closure to compute the layout (and then stores the result in the cache) if no cached layout is found.
#[inline(always)]
pub fn compute_cached_layout<Tree: CacheTree + LayoutPartialTree + ?Sized, ComputeFunction>(
    tree: &mut Tree,
    node: NodeId,
    inputs: LayoutInput,
    compute_uncached: ComputeFunction,
) -> LayoutOutput
where
    ComputeFunction: FnOnce(&mut Tree, NodeId, LayoutInput) -> LayoutOutput,
{
    debug_push_node!(node);

    // First we check if we have a cached result for the given input
    let cache_entry = tree.cache_get(node, &inputs);
    if let Some(cached_size_and_baselines) = cache_entry {
        debug_log_node!(inputs);
        debug_log!("RESULT (CACHED)", dbg:cached_size_and_baselines.size);
        debug_pop_node!();
        return cached_size_and_baselines;
    }

    debug_log_node!(inputs);

    let mut computed_size_and_baselines = compute_uncached(tree, node, inputs);

    computed_size_and_baselines.set_block_constraint_dependency(node_block_constraint_dependency(
        tree,
        node,
        inputs,
        computed_size_and_baselines.block_constraint_dependency(),
    ));

    // Cache result
    tree.cache_store(node, &inputs, computed_size_and_baselines);

    debug_log!("RESULT", dbg:computed_size_and_baselines.size);
    debug_pop_node!();

    computed_size_and_baselines
}

/// Compute or retrieve a dedicated intrinsic size result for a node.
///
/// This is the sizing counterpart to [`compute_cached_layout`]. Its cache
/// stores only sizing geometry and measurement provenance; final fragment
/// state remains in the layout cache.
pub fn compute_cached_size<Tree: CacheTree + LayoutPartialTree + ?Sized, ComputeFunction>(
    tree: &mut Tree,
    node: NodeId,
    inputs: LayoutInput,
    compute_uncached: ComputeFunction,
) -> IntrinsicSizeResult
where
    ComputeFunction: FnOnce(&mut Tree, NodeId, LayoutInput) -> IntrinsicSizeResult,
{
    debug_assert_eq!(inputs.run_mode, RunMode::ComputeSize);
    debug_push_node!(node);

    if let Some(cached_result) = tree.cache_get_size(node, &inputs) {
        debug_log_node!(inputs);
        debug_log!("RESULT (CACHED)", dbg:cached_result.size);
        debug_pop_node!();
        return cached_result;
    }

    debug_log_node!(inputs);
    let mut result = compute_uncached(tree, node, inputs);
    result.depends_on_block_constraints =
        node_block_constraint_dependency(tree, node, inputs, result.depends_on_block_constraints);
    tree.cache_store_size(node, &inputs, result);

    debug_log!("RESULT", dbg:result.size);
    debug_pop_node!();
    result
}

/// Gate a formatting algorithm's reported dependency at the node sizing
/// boundary.
///
/// Content-only probes forward descendant dependency unchanged. Inherent-size
/// probes report it to their parent only when this node consumes the parent's
/// block constraint and the requested result can observe the change.
fn node_block_constraint_dependency(
    tree: &(impl LayoutPartialTree + ?Sized),
    node: NodeId,
    inputs: LayoutInput,
    reported_dependency: bool,
) -> bool {
    if inputs.run_mode != RunMode::ComputeSize {
        return false;
    }
    if inputs.sizing_mode != SizingMode::InherentSize {
        return reported_dependency;
    }

    let writing_mode = tree.get_writing_mode(node);
    let has_aspect_ratio = tree.get_resolved_aspect_ratio(node).has_ratio();
    let style_depends_on_parent_block_size = {
        let style = tree.get_core_container_style(node);
        let size = writing_mode.to_logical(style.size());
        let min_size = writing_mode.to_logical(style.min_size());
        let max_size = writing_mode.to_logical(style.max_size());
        [size.block_size, min_size.block_size, max_size.block_size]
            .into_iter()
            .any(|value| value.may_have_percentage_dependence() || value.is_stretch())
    };
    let requested_block_size = inputs.axis.contains(writing_mode.block_axis());
    style_depends_on_parent_block_size && (requested_block_size || has_aspect_ratio || reported_dependency)
}

/// Rounds the calculated layout to exact pixel values
///
/// In order to ensure that no gaps in the layout are introduced we:
///   - Always round based on the cumulative x/y coordinates (relative to the viewport) rather than
///     parent-relative coordinates
///   - Compute width/height by first rounding the top/bottom/left/right and then computing the difference
///     rather than rounding the width/height directly
///
/// See <https://github.com/facebook/yoga/commit/aa5b296ac78f7a22e1aeaf4891243c6bb76488e2> for more context
///
/// In order to prevent innacuracies caused by rounding already-rounded values, we read from `unrounded_layout`
/// and write to `final_layout`.
pub fn round_layout(tree: &mut impl RoundTree, node_id: NodeId) {
    return round_layout_inner(tree, node_id, 0.0, 0.0);

    /// Recursive function to apply rounding to all descendents
    fn round_layout_inner(tree: &mut impl RoundTree, node_id: NodeId, cumulative_x: f32, cumulative_y: f32) {
        let unrounded_layout = tree.get_unrounded_layout(node_id);
        let mut layout = unrounded_layout;

        let cumulative_x = cumulative_x + unrounded_layout.location.x;
        let cumulative_y = cumulative_y + unrounded_layout.location.y;

        layout.location.x = round(unrounded_layout.location.x);
        layout.location.y = round(unrounded_layout.location.y);
        layout.size.width = round(cumulative_x + unrounded_layout.size.width) - round(cumulative_x);
        layout.size.height = round(cumulative_y + unrounded_layout.size.height) - round(cumulative_y);
        layout.scrollbar_size.width = round(unrounded_layout.scrollbar_size.width);
        layout.scrollbar_size.height = round(unrounded_layout.scrollbar_size.height);
        layout.border.left = round(cumulative_x + unrounded_layout.border.left) - round(cumulative_x);
        layout.border.right = round(cumulative_x + unrounded_layout.size.width)
            - round(cumulative_x + unrounded_layout.size.width - unrounded_layout.border.right);
        layout.border.top = round(cumulative_y + unrounded_layout.border.top) - round(cumulative_y);
        layout.border.bottom = round(cumulative_y + unrounded_layout.size.height)
            - round(cumulative_y + unrounded_layout.size.height - unrounded_layout.border.bottom);
        layout.padding.left = round(cumulative_x + unrounded_layout.padding.left) - round(cumulative_x);
        layout.padding.right = round(cumulative_x + unrounded_layout.size.width)
            - round(cumulative_x + unrounded_layout.size.width - unrounded_layout.padding.right);
        layout.padding.top = round(cumulative_y + unrounded_layout.padding.top) - round(cumulative_y);
        layout.padding.bottom = round(cumulative_y + unrounded_layout.size.height)
            - round(cumulative_y + unrounded_layout.size.height - unrounded_layout.padding.bottom);

        #[cfg(feature = "content_size")]
        round_content_size(&mut layout, unrounded_layout.content_size, cumulative_x, cumulative_y);

        tree.set_final_layout(node_id, &layout);

        let child_count = tree.child_count(node_id);
        for index in 0..child_count {
            let child = tree.get_child_id(node_id, index);
            round_layout_inner(tree, child, cumulative_x, cumulative_y);
        }
    }

    #[cfg(feature = "content_size")]
    #[inline(always)]
    /// Round content size variables.
    /// This is split into a separate function to make it easier to feature flag.
    fn round_content_size(
        layout: &mut Layout,
        unrounded_content_size: Size<f32>,
        cumulative_x: f32,
        cumulative_y: f32,
    ) {
        layout.content_size.width = round(cumulative_x + unrounded_content_size.width) - round(cumulative_x);
        layout.content_size.height = round(cumulative_y + unrounded_content_size.height) - round(cumulative_y);
    }
}

/// Creates a layout for this node and its children, recursively.
/// Each hidden node has zero size and is placed at the origin
pub fn compute_hidden_layout(tree: &mut (impl LayoutPartialTree + CacheTree), node: NodeId) -> LayoutOutput {
    // Clear cache and set zeroed-out layout for the node
    tree.cache_clear(node);
    tree.set_unrounded_layout(node, &Layout::with_order(0));

    // Perform hidden layout on all children
    for index in 0..tree.child_count(node) {
        let child_id = tree.get_child_id(node, index);
        tree.compute_child_layout(child_id, LayoutInput::HIDDEN);
    }

    LayoutOutput::HIDDEN
}

/// A module for unified re-exports of detailed layout info structs, used by low level API
#[cfg(feature = "detailed_layout_info")]
pub mod detailed_info {
    #[cfg(feature = "grid")]
    pub use super::grid::{DetailedGridInfo, DetailedGridItemsInfo, DetailedGridTracksInfo};
}

#[cfg(test)]
mod tests {
    use super::compute_hidden_layout;
    use crate::geometry::{Point, Size};
    use crate::style::{Display, Style};
    use crate::TaffyTree;

    #[test]
    fn hidden_layout_should_hide_recursively() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let style: Style = Style { display: Display::Flex, size: Size::from_lengths(50.0, 50.0), ..Default::default() };

        let grandchild_00 = taffy.new_leaf(style.clone()).unwrap();
        let grandchild_01 = taffy.new_leaf(style.clone()).unwrap();
        let child_00 = taffy.new_with_children(style.clone(), &[grandchild_00, grandchild_01]).unwrap();

        let grandchild_02 = taffy.new_leaf(style.clone()).unwrap();
        let child_01 = taffy.new_with_children(style.clone(), &[grandchild_02]).unwrap();

        let root = taffy
            .new_with_children(
                Style { display: Display::None, size: Size::from_lengths(50.0, 50.0), ..Default::default() },
                &[child_00, child_01],
            )
            .unwrap();

        compute_hidden_layout(&mut taffy.as_layout_tree(), root);

        // Whatever size and display-mode the nodes had previously,
        // all layouts should resolve to ZERO due to the root's DISPLAY::NONE

        for node in [root, child_00, child_01, grandchild_00, grandchild_01, grandchild_02] {
            let layout = taffy.layout(node).unwrap();
            assert_eq!(layout.size, Size::zero());
            assert_eq!(layout.location, Point::zero());
        }
    }
}
