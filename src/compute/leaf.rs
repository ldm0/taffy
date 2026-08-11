//! Computes size using styles and measure functions

use crate::geometry::Size;
use crate::style::{AvailableSpace, Overflow, Position};
use crate::tree::RunMode;
use crate::tree::{LayoutInput, LayoutOutput, SizingMode};
use crate::util::debug::debug_log;
use crate::util::sys::f32_max;
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, CoreStyle, ResolvedAspectRatio};
use core::unreachable;

use super::common::aspect_ratio::{resolve_size_constraints, TransferredSizesMode};

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
    let LayoutInput { known_dimensions, parent_size, available_space, sizing_mode, run_mode, .. } = inputs;

    // Note: both horizontal and vertical percentage padding/borders are resolved against the container's inline size (i.e. width).
    // This is not a bug, but is how CSS is specified (see: https://developer.mozilla.org/en-US/docs/Web/CSS/padding#values)
    let margin = style.margin().resolve_or_zero(parent_size.width, &resolve_calc_value);
    let padding = style.padding().resolve_or_zero(parent_size.width, &resolve_calc_value);
    let border = style.border().resolve_or_zero(parent_size.width, &resolve_calc_value);
    let padding_border = padding + border;
    let pb_sum = padding_border.sum_axes();
    let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };

    // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
    // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
    let (node_size, node_min_size, node_max_size, aspect_ratio, applied_aspect_ratio) = match sizing_mode {
        SizingMode::ContentSize => {
            let node_size = known_dimensions;
            let node_min_size = Size::NONE;
            let node_max_size = Size::NONE;
            (node_size, node_min_size, node_max_size, resolved_aspect_ratio.disabled(), false)
        }
        SizingMode::InherentSize => {
            let raw_size = style.size();
            let resolved = resolve_size_constraints(
                raw_size.maybe_resolve(parent_size, &resolve_calc_value).maybe_add(box_sizing_adjustment),
                style.min_size().maybe_resolve(parent_size, &resolve_calc_value).maybe_add(box_sizing_adjustment),
                style.max_size().maybe_resolve(parent_size, &resolve_calc_value).maybe_add(box_sizing_adjustment),
                raw_size.map(|dimension| dimension.is_auto()),
                TransferredSizesMode::Normal,
                resolved_aspect_ratio,
                pb_sum,
            );
            let style_size = resolved.size;
            let style_min_size = resolved.min_size;
            let style_max_size = resolved.max_size;
            let preferred_inline_from_aspect_ratio = resolved.aspect_ratio_applied.width;

            // A parent formatting context may make exactly one border-box axis
            // definite (for example a stretched flex cross size). Resolve the
            // other axis through the preferred ratio at the leaf boundary just
            // like an authored one-axis size.
            let size_before_ratio = known_dimensions.or(style_size);
            let node_size = size_before_ratio.maybe_apply_aspect_ratio_with_box_sizing(
                resolved_aspect_ratio,
                BoxSizing::BorderBox,
                pb_sum,
            );
            let applied_aspect_ratio = run_mode == RunMode::ComputeSize
                && known_dimensions.width.is_none()
                && (preferred_inline_from_aspect_ratio
                    || (size_before_ratio.width.is_none() && node_size.width.is_some()));
            (node_size, style_min_size, style_max_size, resolved_aspect_ratio, applied_aspect_ratio)
        }
    };

    // Scrollbar gutters are reserved when the `overflow` property is set to `Overflow::Scroll`.
    // However, the axis are switched (transposed) because a node that scrolls vertically needs
    // *horizontal* space to be reserved for a scrollbar
    let scrollbar_gutter = style.overflow().transpose().map(|overflow| match overflow {
        Overflow::Scroll => style.scrollbar_width(),
        _ => 0.0,
    });
    // TODO: make side configurable based on the `direction` property
    let mut content_box_inset = padding_border;
    content_box_inset.right += scrollbar_gutter.x;
    content_box_inset.bottom += scrollbar_gutter.y;

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
        if let Size { width: Some(width), height: Some(height) } = node_size {
            let size = Size { width, height }
                .maybe_clamp(node_min_size, node_max_size)
                .maybe_max(padding_border.sum_axes().map(Some));
            return LayoutOutput::from_outer_size(size).with_applied_aspect_ratio(applied_aspect_ratio);
        };
    }

    // Compute available space
    let available_space = Size {
        width: known_dimensions
            .width
            .map(AvailableSpace::from)
            .unwrap_or(available_space.width)
            .maybe_sub(margin.horizontal_axis_sum())
            .maybe_set(known_dimensions.width)
            .maybe_set(node_size.width)
            .map_definite_value(|size| {
                size.maybe_clamp(node_min_size.width, node_max_size.width) - content_box_inset.horizontal_axis_sum()
            }),
        height: known_dimensions
            .height
            .map(AvailableSpace::from)
            .unwrap_or(available_space.height)
            .maybe_sub(margin.vertical_axis_sum())
            .maybe_set(known_dimensions.height)
            .maybe_set(node_size.height)
            .map_definite_value(|size| {
                size.maybe_clamp(node_min_size.height, node_max_size.height) - content_box_inset.vertical_axis_sum()
            }),
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
    let clamped_size = known_dimensions
        .or(node_size)
        .unwrap_or(measured_size + content_box_inset.sum_axes())
        .maybe_clamp(node_min_size, node_max_size);
    let ratio_height = Size { width: Some(clamped_size.width), height: None }
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, pb_sum)
        .height
        .unwrap_or(0.0);
    let size = Size { width: clamped_size.width, height: f32_max(clamped_size.height, ratio_height) };
    let size = size.maybe_max(padding_border.sum_axes().map(Some));

    let mut output = LayoutOutput::from_sizes(size, measured_size + padding.sum_axes());
    output.margins_can_collapse_through =
        !has_styles_preventing_being_collapsed_through && size.height == 0.0 && measured_size.height == 0.0;
    output.with_applied_aspect_ratio(applied_aspect_ratio)
}
