//! Computes size using styles and measure functions

use crate::geometry::Size;
use crate::style::{AvailableSpace, Overflow, Position, SizeContainment};
use crate::tree::RunMode;
use crate::tree::{LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, SizingMode};
use crate::util::debug::debug_log;
use crate::util::sys::f32_max;
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, CoreStyle, ResolvedAspectRatio, WritingMode};
use core::unreachable;

use super::common::aspect_ratio::{
    apply_preferred_aspect_ratio, resolve_size_constraints, SizeConstraintInput, TransferredSizesMode,
};
use super::common::intrinsic_size::{
    apply_contained_intrinsic_size_constraints, resolve_leaf_node_sizing, ResolvedNodeSizing,
};
use super::common::stretch::resolve_stretch_size_constraints;
use super::common::used_size::{resolve_used_axis, resolve_used_size};

/// Node-level sizing state supplied to leaf layout.
///
/// Embedding engines commonly resolve inherited writing modes, replaced-box
/// aspect ratios, and containment eligibility outside their numeric style
/// projection. Grouping those used values keeps the leaf boundary extensible
/// without growing a sequence of positional arguments.
#[derive(Clone, Copy, Debug)]
pub struct LeafSizingContext {
    /// Used writing mode for this node.
    pub writing_mode: WritingMode,
    /// Used preferred aspect ratio and its sizing box.
    pub aspect_ratio: ResolvedAspectRatio,
    /// Used physical size-containment state.
    pub size_containment: SizeContainment,
}

impl LeafSizingContext {
    /// Construct the used sizing state for a leaf node.
    #[inline(always)]
    pub const fn new(
        writing_mode: WritingMode,
        aspect_ratio: ResolvedAspectRatio,
        size_containment: SizeContainment,
    ) -> Self {
        Self { writing_mode, aspect_ratio, size_containment }
    }
}

/// Compute a leaf through a layout tree's complete node-sizing pipeline.
///
/// The tree owns intrinsic-keyword measurement and preferred-size resolution;
/// the embedding adapter supplies only an immutable style snapshot and its
/// content measurer. Keeping [`ResolvedNodeSizing`] inside this operation makes
/// the raw [`LayoutInput`] the sole cache key and prevents adapters from
/// pre-resolving child-owned geometry outside Taffy's cache-miss boundary.
///
/// `style` must be a snapshot of the style returned by
/// [`LayoutPartialTree::get_core_container_style`] for `node_id`. A snapshot is
/// explicit because content measurement may mutably re-enter `tree` after
/// node sizing has released its style borrow.
pub fn compute_leaf_layout_with_tree<Tree, MeasureFunction>(
    tree: &mut Tree,
    node_id: NodeId,
    inputs: LayoutInput,
    style: &impl CoreStyle,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    Tree: LayoutPartialTree,
    MeasureFunction: FnOnce(&mut Tree, Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    let sizing_context = LeafSizingContext::new(
        tree.get_writing_mode(node_id),
        tree.get_resolved_aspect_ratio(node_id),
        tree.get_size_containment(node_id),
    );
    let node_sizing = resolve_leaf_node_sizing(tree, node_id, inputs);
    compute_leaf_layout_with_resolved_node_sizing(
        inputs,
        style,
        sizing_context,
        Some(node_sizing),
        resolve_calc_value,
        |known_dimensions, available_space| measure_function(tree, known_dimensions, available_space),
    )
}

/// Compute the size of a leaf node (node with no children)
pub fn compute_leaf_layout<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    compute_leaf_layout_with_aspect_ratio(
        inputs,
        style,
        ResolvedAspectRatio { ratio: style.aspect_ratio(), box_sizing: style.box_sizing() },
        resolve_calc_value,
        measure_function,
    )
}

/// Compute the size of a leaf node using a node-level resolved aspect ratio.
///
/// Browser integrations should use this entry point when the used ratio may
/// depend on natural replaced-element sizing or when the ratio constrains a
/// different sizing box than authored width and height.
pub fn compute_leaf_layout_with_aspect_ratio<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    resolved_aspect_ratio: ResolvedAspectRatio,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    compute_leaf_layout_with_aspect_ratio_and_writing_mode(
        inputs,
        style,
        style.writing_mode(),
        resolved_aspect_ratio,
        resolve_calc_value,
        measure_function,
    )
}

/// Compute a leaf layout using an explicit node writing mode.
///
/// This is the browser-adapter seam for integrations that retain inherited
/// properties outside their numeric [`CoreStyle`] projection. The supplied
/// mode must match [`LayoutPartialTree::get_writing_mode`](crate::LayoutPartialTree::get_writing_mode)
/// for the same node.
pub fn compute_leaf_layout_with_aspect_ratio_and_writing_mode<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    writing_mode: WritingMode,
    resolved_aspect_ratio: ResolvedAspectRatio,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    compute_leaf_layout_with_sizing_context(
        inputs,
        style,
        LeafSizingContext::new(writing_mode, resolved_aspect_ratio, SizeContainment::NONE),
        resolve_calc_value,
        measure_function,
    )
}

/// Compute a leaf layout using node-level used sizing state.
///
/// This is the preferred browser-adapter seam. In particular,
/// [`LeafSizingContext::size_containment`] may differ from the raw style when
/// containment is ineligible for the generated box or an `auto` remembered
/// size has been selected by the embedding engine.
pub fn compute_leaf_layout_with_sizing_context<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    sizing_context: LeafSizingContext,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    compute_leaf_layout_with_resolved_node_sizing(
        inputs,
        style,
        sizing_context,
        None,
        resolve_calc_value,
        measure_function,
    )
}

/// Shared leaf implementation after an optional tree-owned sizing pass.
fn compute_leaf_layout_with_resolved_node_sizing<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    sizing_context: LeafSizingContext,
    node_sizing: Option<ResolvedNodeSizing>,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    let LeafSizingContext { writing_mode, aspect_ratio: resolved_aspect_ratio, size_containment } = sizing_context;
    let node_sizing_dependency = node_sizing.is_some_and(|sizing| sizing.depends_on_block_constraints);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { known_dimensions, parent_size, available_space, sizing_mode, run_mode, .. } = inputs;

    let margin = style.margin().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let padding = style.padding().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let border = style.border().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let padding_border = padding + border;
    let pb_sum = padding_border.sum_axes();
    let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };

    // Scrollbar gutters are reserved when the `overflow` property is set to `Overflow::Scroll`.
    // However, the axes are switched (transposed) because a node that scrolls vertically needs
    // *horizontal* space to be reserved for a scrollbar.
    let scrollbar_gutter = style.overflow().transpose().map(|overflow| match overflow {
        Overflow::Scroll => style.scrollbar_width(),
        _ => 0.0,
    });
    // TODO: make side configurable based on the `direction` property
    let mut content_box_inset = padding_border;
    content_box_inset.right += scrollbar_gutter.x;
    content_box_inset.bottom += scrollbar_gutter.y;
    let contained_outer_size = size_containment.resolve_outer_size(Size::ZERO, content_box_inset.sum_axes());

    // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
    // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
    let (node_size, node_min_size, node_max_size, aspect_ratio, applied_aspect_ratio) = match (sizing_mode, node_sizing)
    {
        (SizingMode::ContentSize, Some(node_sizing)) => {
            (node_sizing.outer_size, Size::NONE, Size::NONE, resolved_aspect_ratio.disabled(), false)
        }
        (SizingMode::ContentSize, None) => {
            (known_dimensions.or(contained_outer_size), Size::NONE, Size::NONE, resolved_aspect_ratio.disabled(), false)
        }
        (SizingMode::InherentSize, Some(node_sizing)) => (
            node_sizing.outer_size,
            node_sizing.min_size,
            node_sizing.max_size,
            resolved_aspect_ratio,
            run_mode == RunMode::ComputeSize && node_sizing.applied_aspect_ratio,
        ),
        (SizingMode::InherentSize, None) => {
            let raw_size = style.size();
            let raw_min_size = style.min_size();
            let raw_max_size = style.max_size();
            let stretch = resolve_stretch_size_constraints(
                raw_size,
                raw_min_size,
                raw_max_size,
                available_space.into_options(),
                pb_sum,
            );
            let resolved = apply_contained_intrinsic_size_constraints(
                resolve_size_constraints(SizeConstraintInput {
                    size: raw_size
                        .maybe_resolve(parent_size, &resolve_calc_value)
                        .maybe_add(box_sizing_adjustment)
                        .or(stretch.preferred),
                    min_size: raw_min_size
                        .maybe_resolve(parent_size, &resolve_calc_value)
                        .maybe_add(box_sizing_adjustment)
                        .or(stretch.min),
                    max_size: raw_max_size
                        .maybe_resolve(parent_size, &resolve_calc_value)
                        .maybe_add(box_sizing_adjustment)
                        .or(stretch.max),
                    size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
                    writing_mode,
                    inline_auto_behavior: inputs.inline_auto_behavior,
                    block_auto_behavior: inputs.block_auto_behavior,
                    transferred_sizes_mode: TransferredSizesMode::Normal,
                    aspect_ratio: resolved_aspect_ratio,
                    padding_border: pb_sum,
                }),
                raw_size,
                raw_min_size,
                raw_max_size,
                contained_outer_size,
            );
            let style_size = resolved.size;
            let style_min_size = resolved.min_size;
            let style_max_size = resolved.max_size;
            let preferred_inline_from_aspect_ratio = resolved.aspect_ratio_applied.get_abs(writing_mode.inline_axis());

            // A parent formatting context may make exactly one border-box axis
            // definite (for example a stretched flex cross size). Resolve the
            // other axis through the preferred ratio at the leaf boundary just
            // like an authored one-axis size.
            let contained_size = contained_outer_size.maybe_clamp(style_min_size, style_max_size);
            let size_before_ratio = known_dimensions.or(style_size).or(contained_size);
            let size_after_ratio = apply_preferred_aspect_ratio(
                size_before_ratio,
                raw_size.map(|dimension| dimension.is_auto()),
                writing_mode,
                inputs.inline_auto_behavior,
                inputs.block_auto_behavior,
                resolved_aspect_ratio,
                pb_sum,
            );
            let inline_axis = writing_mode.inline_axis();
            let applied_aspect_ratio = run_mode == RunMode::ComputeSize
                && known_dimensions.get_abs(inline_axis).is_none()
                && (preferred_inline_from_aspect_ratio
                    || (size_before_ratio.get_abs(inline_axis).is_none()
                        && size_after_ratio.get_abs(inline_axis).is_some()));
            (size_after_ratio, style_min_size, style_max_size, resolved_aspect_ratio, applied_aspect_ratio)
        }
    };

    let has_styles_preventing_being_collapsed_through = !style.is_block()
        || style.overflow().x.is_scroll_container()
        || style.overflow().y.is_scroll_container()
        || style.position() == Position::Absolute
        || padding.top > 0.0
        || padding.bottom > 0.0
        || border.top > 0.0
        || border.bottom > 0.0
        || matches!(node_size.height, Some(h) if h > 0.0)
        || matches!(node_min_size.height, Some(h) if h > 0.0);

    debug_log!("LEAF");
    debug_log!("node_size", dbg:node_size);
    debug_log!("min_size ", dbg:node_min_size);
    debug_log!("max_size ", dbg:node_max_size);

    // Return early if both width and height are known
    if run_mode == RunMode::ComputeSize && has_styles_preventing_being_collapsed_through {
        let used_size = resolve_used_size(known_dimensions, node_size, node_min_size, node_max_size, pb_sum);
        if let Size { width: Some(width), height: Some(height) } = used_size {
            let size = Size { width, height };
            return LayoutOutput::from_outer_size(size)
                .with_block_constraint_dependency(node_sizing_dependency)
                .with_applied_aspect_ratio(applied_aspect_ratio);
        };
    }

    // Compute available space
    let resolve_available_axis = |known_dimension: Option<f32>,
                                  node_size: Option<f32>,
                                  available_space: AvailableSpace,
                                  margin_sum: f32,
                                  min_size: Option<f32>,
                                  max_size: Option<f32>,
                                  minimum_border_box_size: f32,
                                  content_box_inset: f32| {
        let resolved_size = resolve_used_axis(known_dimension, node_size, min_size, max_size, minimum_border_box_size);
        available_space.maybe_sub(margin_sum).maybe_set(resolved_size).map_definite_value(|size| {
            let outer_size = if resolved_size.is_some() {
                size
            } else {
                size.maybe_clamp(min_size, max_size).max(minimum_border_box_size)
            };
            outer_size - content_box_inset
        })
    };
    let available_space = Size {
        width: resolve_available_axis(
            known_dimensions.width,
            node_size.width,
            available_space.width,
            margin.horizontal_axis_sum(),
            node_min_size.width,
            node_max_size.width,
            pb_sum.width,
            content_box_inset.horizontal_axis_sum(),
        ),
        height: resolve_available_axis(
            known_dimensions.height,
            node_size.height,
            available_space.height,
            margin.vertical_axis_sum(),
            node_min_size.height,
            node_max_size.height,
            pb_sum.height,
            content_box_inset.vertical_axis_sum(),
        ),
    };

    // Measure node
    let measured_size = measure_function(
        match run_mode {
            RunMode::ComputeSize => known_dimensions,
            RunMode::PerformLayout => Size::NONE,
            RunMode::PerformHiddenLayout => unreachable!(),
        },
        available_space,
    );
    let measured_outer_size = measured_size + content_box_inset.sum_axes();
    let used_size = resolve_used_size(
        known_dimensions,
        node_size.or(measured_outer_size.map(Some)),
        node_min_size,
        node_max_size,
        pb_sum,
    )
    .unwrap_or(measured_outer_size);
    let ratio_height = Size { width: Some(used_size.width), height: None }
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, pb_sum)
        .height
        .unwrap_or(0.0);
    let size = Size {
        width: used_size.width,
        height: known_dimensions.height.unwrap_or_else(|| f32_max(used_size.height, ratio_height)),
    };

    let mut output = LayoutOutput::from_sizes(size, measured_size + padding.sum_axes());
    output.margins_can_collapse_through =
        !has_styles_preventing_being_collapsed_through && size.height == 0.0 && measured_size.height == 0.0;
    output.with_block_constraint_dependency(node_sizing_dependency).with_applied_aspect_ratio(applied_aspect_ratio)
}
