use super::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use super::intrinsic_size::{
    resolve_intrinsic_axis_constraints, resolve_ratio_dependent_inline_minimum, IntrinsicAxisInput,
};
use crate::geometry::{AbsoluteAxis, Line, LogicalSize, Rect, Size, WritingDirection};
use crate::style::AlignmentSafety;
use crate::style::AvailableSpace;
use crate::tree::{ChildLayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{AlignItemsKeyword, AutoSizeBehavior, BoxSizing, CoreStyle, Dimension, LayoutOutput, SizingMode};

/// The actual absolute containing block and the space contributed by the
/// static-position source. These need not be the same formatting context.
#[derive(Clone, Copy, Debug)]
pub struct AbsoluteConstraintSpace {
    /// Physical padding-box dimensions used for percentages and explicit insets.
    pub containing_block_size: Size<f32>,
    /// Flow direction of that containing block, not the static-position source.
    pub writing_direction: WritingDirection,
    /// Physical space available when both insets in an axis are auto. Owners
    /// derive this from their static-position rectangle before child sizing.
    pub static_available_space: Size<f32>,
}

/// A fully laid-out absolute child, before its owner publishes the position.
/// All box edges use the actual fragment size, including structural minimums
/// imposed by custom formatters such as tables.
pub struct AbsoluteLayoutOutput {
    /// The child formatter's authoritative result.
    pub output: LayoutOutput,
    /// Used physical margins after final-size resolution.
    pub margin: Rect<f32>,
    /// Used physical padding.
    pub padding: Rect<f32>,
    /// Used physical borders.
    pub border: Rect<f32>,
    /// Definite physical insets, or `None` for a static-position edge.
    pub inset: Rect<Option<f32>>,
}

/// Resolve and lay out one absolutely positioned box in logical sizing order.
/// Insets select available space; only explicit self-alignment supplies an
/// explicit stretch constraint. Preferred sizes, content contributions and
/// min/max are resolved before any dimension is published as fixed.
pub fn compute_absolute_layout(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    space: AbsoluteConstraintSpace,
) -> AbsoluteLayoutOutput {
    let writing_mode = tree.get_writing_mode(node);
    let aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let percentage_basis = space.writing_direction.mode.to_logical(space.containing_block_size).inline_size;
    let style = tree.get_core_container_style(node);
    let raw_size = writing_mode.to_logical(style.size());
    let raw_min = writing_mode.to_logical(style.min_size());
    let raw_max = writing_mode.to_logical(style.max_size());
    let replaced = style.is_compressible_replaced();
    let implicit_stretch = !replaced && !style.is_table_wrapper();
    let raw_margin =
        style.margin().map(|value| value.maybe_resolve(percentage_basis, |value, basis| tree.calc(value, basis)));
    let margin = raw_margin.map(|value| value.unwrap_or(0.0));
    let padding = style.padding().resolve_or_zero(Some(percentage_basis), |value, basis| tree.calc(value, basis));
    let border = style.border().resolve_or_zero(Some(percentage_basis), |value, basis| tree.calc(value, basis));
    let padding_border = (padding + border).sum_axes();
    let adjustment = if style.box_sizing() == BoxSizing::ContentBox { padding_border } else { Size::ZERO };
    let resolve = |value: Size<Dimension>| {
        value.maybe_resolve(space.containing_block_size, |value, basis| tree.calc(value, basis)).maybe_add(adjustment)
    };
    let numeric_size = resolve(style.size());
    let mut constraints = SizeConstraintInput {
        size: numeric_size,
        min_size: resolve(style.min_size()),
        max_size: resolve(style.max_size()),
        size_is_auto: style.size().map(|value| value.is_auto()),
        writing_mode,
        block_auto_behavior: AutoSizeBehavior::FitContent,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border,
    };
    let inset = Rect {
        left: style
            .inset()
            .left
            .maybe_resolve(space.containing_block_size.width, |value, basis| tree.calc(value, basis)),
        right: style
            .inset()
            .right
            .maybe_resolve(space.containing_block_size.width, |value, basis| tree.calc(value, basis)),
        top: style
            .inset()
            .top
            .maybe_resolve(space.containing_block_size.height, |value, basis| tree.calc(value, basis)),
        bottom: style
            .inset()
            .bottom
            .maybe_resolve(space.containing_block_size.height, |value, basis| tree.calc(value, basis)),
    };
    let alignment = style.positioned_alignment();
    drop(style);

    let axis_input = |axis: AbsoluteAxis| {
        let edges = match axis {
            AbsoluteAxis::Horizontal => inset.horizontal_components(),
            AbsoluteAxis::Vertical => inset.vertical_components(),
        };
        let alignment =
            if axis == space.writing_direction.mode.inline_axis() { alignment.inline } else { alignment.block };
        let behavior = if edges.start.is_none() || edges.end.is_none() {
            AutoSizeBehavior::FitContent
        } else {
            match alignment.map(|value| value.keyword()) {
                Some(AlignItemsKeyword::Stretch) => AutoSizeBehavior::StretchExplicit,
                None | Some(AlignItemsKeyword::Normal) if implicit_stretch => AutoSizeBehavior::StretchImplicit,
                _ => AutoSizeBehavior::FitContent,
            }
        };
        let extent = if edges.start.is_none() && edges.end.is_none() {
            space.static_available_space.get_abs(axis)
        } else {
            space.containing_block_size.get_abs(axis) - edges.start.unwrap_or(0.0) - edges.end.unwrap_or(0.0)
        } - margin.sum_axes().get_abs(axis);
        (behavior, extent.max(0.0))
    };
    let (inline_behavior, available_inline) = axis_input(writing_mode.inline_axis());
    let (block_behavior, available_block) = axis_input(writing_mode.block_axis());
    let available =
        writing_mode.to_physical(LogicalSize { inline_size: available_inline, block_size: available_block });
    let mut numeric = writing_mode.to_logical(numeric_size);
    let block_stretches_first = block_behavior == AutoSizeBehavior::StretchExplicit
        || (block_behavior == AutoSizeBehavior::StretchImplicit
            && (aspect_ratio.is_none()
                || (raw_size.inline_size.is_auto() && inline_behavior == AutoSizeBehavior::FitContent)));
    if raw_size.block_size.is_auto() && block_stretches_first {
        numeric.block_size = Some(available_block);
    }
    let block_size_is_resolved = numeric.block_size.is_some();
    let ratio_from_block =
        aspect_ratio.is_some() && block_size_is_resolved && inline_behavior != AutoSizeBehavior::StretchExplicit;
    if raw_size.inline_size.is_auto()
        && (inline_behavior == AutoSizeBehavior::StretchExplicit
            || (inline_behavior == AutoSizeBehavior::StretchImplicit && !ratio_from_block))
    {
        numeric.inline_size = Some(available_inline);
    }
    constraints.size = writing_mode.to_physical(numeric);
    constraints.block_auto_behavior = block_behavior;
    let child_input = |known| {
        ChildLayoutInput::new(
            known,
            space.containing_block_size.map(Some),
            space.writing_direction.mode,
            available.map(AvailableSpace::Definite),
            SizingMode::ContentSize,
            Line::FALSE,
        )
        .with_block_auto_behavior(block_behavior)
    };
    let intrinsic = resolve_intrinsic_axis_constraints(
        tree,
        node,
        child_input(writing_mode.to_physical(LogicalSize { inline_size: None, block_size: numeric.block_size })),
        IntrinsicAxisInput {
            preferred: raw_size.inline_size,
            min: raw_min.inline_size,
            max: raw_max.inline_size,
            available_space: AvailableSpace::Definite(available_inline),
            axis: writing_mode.inline_axis(),
        },
    );
    let merge_inline = |size: Size<Option<f32>>, inline| {
        let mut logical = writing_mode.to_logical(size);
        logical.inline_size = logical.inline_size.or(inline);
        writing_mode.to_physical(logical)
    };
    constraints.min_size = merge_inline(constraints.min_size, intrinsic.min);
    constraints.max_size = merge_inline(constraints.max_size, intrinsic.max);
    let mut resolved = resolve_size_constraints(constraints);
    let mut dependency = intrinsic.depends_on_block_constraints;
    dependency |= resolve_ratio_dependent_inline_minimum(
        tree,
        node,
        child_input(resolved.size.maybe_clamp(resolved.min_size, resolved.max_size)),
        &mut resolved,
    );
    let preferred_inline = writing_mode.to_logical(resolved.size).inline_size.or(intrinsic.preferred);
    let inline_size = if let Some(preferred) = preferred_inline {
        preferred
    } else {
        let fit = resolve_intrinsic_axis_constraints(
            tree,
            node,
            child_input(resolved.size),
            IntrinsicAxisInput {
                preferred: Dimension::fit_content(),
                min: Dimension::auto(),
                max: Dimension::auto(),
                available_space: AvailableSpace::Definite(available_inline),
                axis: writing_mode.inline_axis(),
            },
        );
        dependency |= fit.depends_on_block_constraints;
        fit.preferred.expect("fit-content always resolves an intrinsic size")
    }
    .maybe_clamp(
        writing_mode.to_logical(resolved.min_size).inline_size,
        writing_mode.to_logical(resolved.max_size).inline_size,
    )
    .max(writing_mode.to_logical(padding_border).inline_size);

    // Only an independently resolved block constraint may cross the child
    // boundary as fixed. An automatic/content-derived block size belongs to
    // the child's own formatting algorithm, not to a generic content probe.
    let block_needs_content =
        [raw_size.block_size, raw_min.block_size, raw_max.block_size].into_iter().any(|value| value.is_intrinsic());
    let known_block = if block_needs_content {
        None
    } else {
        numeric.block_size.maybe_clamp(
            writing_mode.to_logical(resolved.min_size).block_size,
            writing_mode.to_logical(resolved.max_size).block_size,
        )
    };
    let output = tree
        .perform_child_layout(
            node,
            ChildLayoutInput {
                sizing_mode: SizingMode::InherentSize,
                ..child_input(
                    writing_mode.to_physical(LogicalSize { inline_size: Some(inline_size), block_size: known_block }),
                )
            },
        )
        .with_block_constraint_dependency(dependency);
    let margin =
        resolve_absolute_margins(raw_margin, inset, space.containing_block_size, output.size, space.writing_direction);
    AbsoluteLayoutOutput { output, margin, padding, border, inset }
}

/// Which physical margin-box edge is anchored by a static position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticPositionEdge {
    /// The low-coordinate edge (left or top).
    Min,
    /// The center of the margin box.
    Center,
    /// The high-coordinate edge (right or bottom).
    Max,
}

/// A static-position contribution in one physical axis of the actual absolute
/// containing block. Its source formatting context may be a different box.
#[derive(Clone, Copy, Debug)]
pub struct StaticPositionAxis {
    /// Static offset relative to the containing block's padding-box origin.
    pub offset: f32,
    /// The margin-box edge anchored at the offset.
    pub edge: StaticPositionEdge,
    /// Overflow safety from the positioned child's self-alignment.
    pub safety: AlignmentSafety,
}

impl StaticPositionAxis {
    /// Bounds of the inset-modified containing block. Centered positions grow
    /// symmetrically until they reach the nearest containing-block edge.
    pub fn inset_modified_bounds(&self, available: f32) -> Line<f32> {
        match self.edge {
            StaticPositionEdge::Min => Line { start: self.offset, end: available },
            StaticPositionEdge::Max => Line { start: 0.0, end: self.offset },
            StaticPositionEdge::Center => {
                let half = self.offset.min(available - self.offset);
                Line { start: self.offset - half, end: self.offset + half }
            }
        }
    }

    /// Resolve the border-box origin after sizing and auto-margin resolution.
    /// Safe overflow biases toward the containing block's flow start, not the
    /// source formatting context's flex-start or content edge.
    pub fn border_box_start(
        &self,
        available: f32,
        size: f32,
        margin: Line<f32>,
        containing_start_reversed: bool,
    ) -> f32 {
        let bounds = self.inset_modified_bounds(available);
        let margin_box_size = size + margin.start + margin.end;
        if self.safety == AlignmentSafety::Safe && margin_box_size > bounds.end - bounds.start {
            return if containing_start_reversed {
                bounds.end - size - margin.end
            } else {
                bounds.start + margin.start
            };
        }
        match self.edge {
            StaticPositionEdge::Min => self.offset + margin.start,
            StaticPositionEdge::Center => self.offset - size / 2.0 + (margin.start - margin.end) / 2.0,
            StaticPositionEdge::Max => self.offset - size - margin.end,
        }
    }
}

/// Resolve absolute auto margins against the actual containing block, after
/// definite insets and the final border-box size have been accounted for.
pub fn resolve_absolute_margins(
    margin: Rect<Option<f32>>,
    inset: Rect<Option<f32>>,
    area_size: Size<f32>,
    box_size: Size<f32>,
    writing_direction: WritingDirection,
) -> Rect<f32> {
    let resolve = |axis, margin, inset| {
        resolve_absolute_axis_margins(
            margin,
            inset,
            area_size.get_abs(axis),
            box_size.get_abs(axis),
            axis == writing_direction.mode.block_axis(),
            !writing_direction.mode.is_axis_flow_reversed(axis, writing_direction.direction),
        )
    };
    let horizontal = resolve(
        AbsoluteAxis::Horizontal,
        Line { start: margin.left, end: margin.right },
        Line { start: inset.left, end: inset.right },
    );
    let vertical = resolve(
        AbsoluteAxis::Vertical,
        Line { start: margin.top, end: margin.bottom },
        Line { start: inset.top, end: inset.bottom },
    );
    Rect { left: horizontal.start, right: horizontal.end, top: vertical.start, bottom: vertical.end }
}

/// Auto margins require two definite insets. Negative inline-axis space belongs
/// to the non-dominant edge; negative block-axis space is shared equally.
fn resolve_absolute_axis_margins(
    margin: Line<Option<f32>>,
    inset: Line<Option<f32>>,
    area_size: f32,
    box_size: f32,
    share_negative_space: bool,
    start_is_dominant: bool,
) -> Line<f32> {
    if inset.start.is_none() || inset.end.is_none() {
        return Line { start: margin.start.unwrap_or(0.0), end: margin.end.unwrap_or(0.0) };
    }
    let free_space = area_size
        - inset.start.unwrap()
        - inset.end.unwrap()
        - box_size
        - margin.start.unwrap_or(0.0)
        - margin.end.unwrap_or(0.0);
    match (margin.start, margin.end) {
        (Some(start), Some(end)) => Line { start, end },
        (None, Some(end)) => Line { start: free_space, end },
        (Some(start), None) => Line { start, end: free_space },
        (None, None) if free_space > 0.0 || share_negative_space => {
            let start = free_space / 2.0;
            Line { start, end: free_space - start }
        }
        (None, None) if start_is_dominant => Line { start: 0.0, end: free_space },
        (None, None) => Line { start: free_space, end: 0.0 },
    }
}
