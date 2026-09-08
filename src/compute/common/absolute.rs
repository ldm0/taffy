use crate::geometry::{AbsoluteAxis, Line, Rect, Size, WritingDirection};
use crate::style::AlignmentSafety;
use crate::style::AvailableSpace;
use crate::tree::{ChildLayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId};

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

/// Resolves the fit-content width used by an auto-width absolutely positioned box.
///
/// CSS 2 defines this as `min(max(min-content, available), max-content)`. A single
/// measurement with definite available space is insufficient: nested block and flex
/// containers may return their max-content contribution while they are being measured.
#[inline]
pub(crate) fn fit_content_width(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    mut inputs: ChildLayoutInput,
    available_width: f32,
) -> f32 {
    inputs.available_space.width = AvailableSpace::MinContent;
    let min_content = tree.measure_child_size(node, inputs, AbsoluteAxis::Horizontal);
    inputs.available_space.width = AvailableSpace::MaxContent;
    let max_content = tree.measure_child_size(node, inputs, AbsoluteAxis::Horizontal);

    available_width.max(0.0).max(min_content).min(max_content)
}
