//! Computes size using styles and measure functions

use crate::geometry::{Rect, Size};
use crate::style::{resolve_scrollbar_insets, AvailableSpace, Position};
use crate::tree::RunMode;
use crate::tree::{LayoutInput, LayoutOutput, SizingMode};
use crate::util::debug::debug_log;
use crate::util::sys::f32_max;
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, CoreStyle, ResolvedAspectRatio, WritingMode};
use core::unreachable;

use super::common::aspect_ratio::{
    apply_preferred_aspect_ratio, resolve_size_constraints, ResolvedAxisConstraints, SizeConstraintInput,
    TransferredSizesMode,
};
use super::common::intrinsic_size::{BlockSizeProperties, ContentBasedBlockSize};
use super::common::used_size::{resolve_used_axis, resolve_used_size};

/// Node-level values resolved by the embedding before leaf layout begins.
///
/// These values form one adapter boundary so browser integrations do not need
/// a separate leaf-layout entry point for every combination of inherited and
/// replaced-element state.
#[derive(Copy, Clone, Debug)]
pub struct LeafLayoutContext {
    /// The inherited writing mode that defines the node's logical axes.
    writing_mode: WritingMode,
    /// The node's used preferred ratio, including its sizing-box semantics.
    resolved_aspect_ratio: Option<ResolvedAspectRatio>,
    /// Physical space occupied by resolved scrollbar gutters.
    scrollbar_insets: Rect<f32>,
}

impl LeafLayoutContext {
    /// Creates a context from values already resolved at the layout node.
    pub const fn new(
        writing_mode: WritingMode,
        resolved_aspect_ratio: Option<ResolvedAspectRatio>,
        scrollbar_insets: Rect<f32>,
    ) -> Self {
        Self { writing_mode, resolved_aspect_ratio, scrollbar_insets }
    }

    /// Builds the default context from values exposed directly by the style.
    fn from_style(style: &impl CoreStyle) -> Self {
        let resolved_aspect_ratio =
            style.aspect_ratio().and_then(|ratio| ResolvedAspectRatio::new(ratio, style.box_sizing()));
        Self::new(style.writing_mode(), resolved_aspect_ratio, resolve_scrollbar_insets(style))
    }
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
    compute_leaf_layout_with_context(
        inputs,
        style,
        LeafLayoutContext::from_style(style),
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
    resolved_aspect_ratio: Option<ResolvedAspectRatio>,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    compute_leaf_layout_with_context(
        inputs,
        style,
        LeafLayoutContext::new(style.writing_mode(), resolved_aspect_ratio, resolve_scrollbar_insets(style)),
        resolve_calc_value,
        measure_function,
    )
}

/// Compute the size of a leaf node using scrollbar gutters that have already
/// been resolved to physical edges by the embedding.
///
/// This is the axis-independent counterpart to [`compute_leaf_layout`]. The
/// legacy entry point remains available and derives conventional end-edge
/// gutters from the style's scalar scrollbar width.
pub fn compute_leaf_layout_with_scrollbar_insets<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    scrollbar_insets: Rect<f32>,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    let resolved_aspect_ratio =
        style.aspect_ratio().and_then(|ratio| ResolvedAspectRatio::new(ratio, style.box_sizing()));
    compute_leaf_layout_with_context(
        inputs,
        style,
        LeafLayoutContext::new(style.writing_mode(), resolved_aspect_ratio, scrollbar_insets),
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
    resolved_aspect_ratio: Option<ResolvedAspectRatio>,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    compute_leaf_layout_with_context(
        inputs,
        style,
        LeafLayoutContext::new(writing_mode, resolved_aspect_ratio, resolve_scrollbar_insets(style)),
        resolve_calc_value,
        measure_function,
    )
}

/// Computes a leaf from the complete set of embedding-resolved inputs.
pub fn compute_leaf_layout_with_context<MeasureFunction>(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    context: LeafLayoutContext,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    measure_function: MeasureFunction,
) -> LayoutOutput
where
    MeasureFunction: FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    let LeafLayoutContext { writing_mode, resolved_aspect_ratio, scrollbar_insets } = context;
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { known_dimensions, parent_size, available_space, sizing_mode, run_mode, .. } = inputs;

    let margin = style.margin().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let padding = style.padding().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let border = style.border().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let padding_border = padding + border;
    let pb_sum = padding_border.sum_axes();
    let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };

    // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
    // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
    let (node_size, node_min_size, node_max_size, aspect_ratio, applied_aspect_ratio, block_limits) = match sizing_mode
    {
        SizingMode::ContentSize => {
            let node_size = known_dimensions;
            let node_min_size = Size::NONE;
            let node_max_size = Size::NONE;
            (node_size, node_min_size, node_max_size, None, false, ResolvedAxisConstraints::default())
        }
        SizingMode::InherentSize => {
            let raw_size = style.size();
            let resolved = resolve_size_constraints(SizeConstraintInput {
                size: raw_size.maybe_resolve(parent_size, &resolve_calc_value).maybe_add(box_sizing_adjustment),
                min_size: style
                    .min_size()
                    .maybe_resolve(parent_size, &resolve_calc_value)
                    .maybe_add(box_sizing_adjustment),
                max_size: style
                    .max_size()
                    .maybe_resolve(parent_size, &resolve_calc_value)
                    .maybe_add(box_sizing_adjustment),
                size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
                writing_mode,
                block_auto_behavior: inputs.block_auto_behavior,
                transferred_sizes_mode: TransferredSizesMode::Normal,
                aspect_ratio: resolved_aspect_ratio,
                padding_border: pb_sum,
            });
            let style_size = resolved.size;
            let style_min_size = resolved.min_size;
            let style_max_size = resolved.max_size;
            let preferred_inline_from_aspect_ratio = writing_mode.to_logical(resolved.aspect_ratio_applied).inline_size;

            // A parent formatting context may make exactly one border-box axis
            // definite (for example a stretched flex cross size). Resolve the
            // other axis through the preferred ratio at the leaf boundary just
            // like an authored one-axis size.
            let size_before_ratio = known_dimensions.or(style_size);
            let node_size = apply_preferred_aspect_ratio(
                size_before_ratio,
                raw_size.map(|dimension| dimension.is_auto()),
                writing_mode,
                inputs.block_auto_behavior,
                resolved_aspect_ratio,
                pb_sum,
            );
            let applied_aspect_ratio = run_mode == RunMode::ComputeSize
                && writing_mode.to_logical(known_dimensions).inline_size.is_none()
                && (preferred_inline_from_aspect_ratio
                    || (writing_mode.to_logical(size_before_ratio).inline_size.is_none()
                        && writing_mode.to_logical(node_size).inline_size.is_some()));
            (
                node_size,
                style_min_size,
                style_max_size,
                resolved_aspect_ratio,
                applied_aspect_ratio,
                resolved.block_axis_constraints(writing_mode),
            )
        }
    };

    // Measured block content (including an embedding's inline formatting
    // context) owns the same automatic minimum as a block subtree. Opaque
    // non-block leaves and replaced elements retain their own sizing rules.
    let block_property = writing_mode.to_logical(style.size()).block_size;
    let block_resolver = (style.is_block()
        && !style.is_compressible_replaced()
        && aspect_ratio.is_some()
        && writing_mode.to_logical(known_dimensions).block_size.is_none()
        && (block_property.is_auto() || block_property.is_intrinsic()))
    .then(|| {
        let overflow = style.overflow();
        ContentBasedBlockSize::new(
            BlockSizeProperties::new(
                block_property,
                writing_mode.to_logical(style.min_size()).block_size,
                writing_mode.to_logical(style.max_size()).block_size,
            ),
            aspect_ratio,
            pb_sum,
            inputs.block_auto_behavior.is_content_based(aspect_ratio.is_some()),
            overflow.x.is_scroll_container() || overflow.y.is_scroll_container(),
        )
    });

    let content_box_inset = padding_border + scrollbar_insets;
    let writing_direction = crate::WritingDirection::new(writing_mode, style.direction());
    let logical_padding = writing_direction.to_logical_box_strut(padding);
    let logical_border = writing_direction.to_logical_box_strut(border);

    let has_styles_preventing_being_collapsed_through = !style.is_block()
        || style.overflow().x.is_scroll_container()
        || style.overflow().y.is_scroll_container()
        || style.position() == Position::Absolute
        || logical_padding.block_start > 0.0
        || logical_padding.block_end > 0.0
        || logical_border.block_start > 0.0
        || logical_border.block_end > 0.0
        || matches!(writing_mode.to_logical(node_size).block_size, Some(size) if size > 0.0)
        || matches!(writing_mode.to_logical(node_min_size).block_size, Some(size) if size > 0.0);

    debug_log!("LEAF");
    debug_log!("node_size", dbg:node_size);
    debug_log!("min_size ", dbg:node_min_size);
    debug_log!("max_size ", dbg:node_max_size);

    // Return early if both width and height are known
    if run_mode == RunMode::ComputeSize
        && has_styles_preventing_being_collapsed_through
        && !block_resolver.is_some_and(|resolver| resolver.requires_intrinsic_measurement())
    {
        let used_size = resolve_used_size(known_dimensions, node_size, node_min_size, node_max_size, pb_sum);
        if let Size { width: Some(width), height: Some(height) } = used_size {
            let size = Size { width, height };
            return LayoutOutput::from_outer_size(size).with_applied_aspect_ratio(applied_aspect_ratio);
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
    let used_logical_size = writing_mode.to_logical(used_size);
    let ratio_size = writing_mode
        .to_physical(crate::LogicalSize { inline_size: Some(used_logical_size.inline_size), block_size: None })
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, pb_sum);
    let ratio_block_size = writing_mode.to_logical(ratio_size).block_size.unwrap_or(0.0);
    let size = writing_mode.to_physical(crate::LogicalSize {
        inline_size: used_logical_size.inline_size,
        block_size: if let Some(resolver) = block_resolver {
            let constraints = resolver
                .resolve(
                    writing_mode,
                    Some(used_logical_size.inline_size),
                    writing_mode.to_logical(measured_outer_size).block_size,
                )
                .resolve_against(None, block_limits);
            constraints
                .preferred
                .unwrap_or(used_logical_size.block_size)
                .maybe_clamp(constraints.min, constraints.max)
                .max(writing_mode.to_logical(pb_sum).block_size)
        } else if writing_mode.to_logical(known_dimensions).block_size.is_some() {
            used_logical_size.block_size
        } else {
            f32_max(used_logical_size.block_size, ratio_block_size)
        },
    });

    let mut output = LayoutOutput::from_sizes(size, measured_size + padding.sum_axes());
    output.margins_can_collapse_through = !has_styles_preventing_being_collapsed_through
        && writing_mode.to_logical(size).block_size == 0.0
        && writing_mode.to_logical(measured_size).block_size == 0.0;
    output.with_applied_aspect_ratio(applied_aspect_ratio)
}
