use crate::geometry::{
    AbsoluteAxis, LogicalSize, LogicalStaticPosition, Point, Rect, Size, StaticPositionEdge, WritingDirection,
};
use crate::style::AvailableSpace;
use crate::tree::{ChildLayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId};

/// Compute the physical size left by an absolutely positioned box's insets
/// and static position.
///
/// This is the positioned-layout equivalent of Blink's inset-modified
/// containing block. Both stretch sizing values and shrink-to-fit measurement
/// consume this geometry; keeping it shared prevents their available-space
/// rules from drifting apart.
#[inline]
pub(crate) fn inset_modified_containing_block_size(
    containing_block_size: Size<f32>,
    inset: Rect<Option<f32>>,
    static_position: Point<f32>,
    horizontal_start_is_reversed: bool,
) -> Size<f32> {
    #[inline(always)]
    fn axis_size(
        containing_size: f32,
        start: Option<f32>,
        end: Option<f32>,
        static_position: f32,
        start_is_reversed: bool,
    ) -> f32 {
        let inset_modified_size = match (start, end) {
            (Some(start), Some(end)) => containing_size - start - end,
            (Some(start), None) => containing_size - start,
            (None, Some(end)) => containing_size - end,
            (None, None) if start_is_reversed => static_position,
            (None, None) => containing_size - static_position,
        };
        inset_modified_size.max(0.0)
    }

    Size {
        width: axis_size(
            containing_block_size.width,
            inset.left,
            inset.right,
            static_position.x,
            horizontal_start_is_reversed,
        ),
        height: axis_size(containing_block_size.height, inset.top, inset.bottom, static_position.y, false),
    }
}

/// Compute the space available to an out-of-flow box from a flow-relative
/// static-position candidate.
///
/// Unlike [`inset_modified_containing_block_size`], this keeps the candidate's
/// start/center/end bias intact. That bias changes shrink-to-fit space before
/// the box's used size is known: start grows toward end, end grows toward
/// start, and center grows equally until it reaches the nearest edge.
#[inline]
pub(crate) fn logical_inset_modified_containing_block_size(
    containing_block_size: Size<f32>,
    inset: Rect<Option<f32>>,
    static_position: LogicalStaticPosition,
    writing_direction: WritingDirection,
) -> Size<f32> {
    #[inline(always)]
    fn axis_size(
        containing_size: f32,
        start: Option<f32>,
        end: Option<f32>,
        static_offset: f32,
        static_edge: StaticPositionEdge,
    ) -> f32 {
        let inset_modified_size = match (start, end) {
            (Some(start), Some(end)) => containing_size - start - end,
            (Some(start), None) => containing_size - start,
            (None, Some(end)) => containing_size - end,
            (None, None) => match static_edge {
                StaticPositionEdge::Start => containing_size - static_offset,
                StaticPositionEdge::Center => 2.0 * static_offset.min(containing_size - static_offset),
                StaticPositionEdge::End => static_offset,
            },
        };
        inset_modified_size.max(0.0)
    }

    let logical_size = writing_direction.mode.to_logical(containing_block_size);
    let logical_inset = writing_direction.to_logical_box_strut(inset);
    writing_direction.mode.to_physical(LogicalSize {
        inline_size: axis_size(
            logical_size.inline_size,
            logical_inset.inline_start,
            logical_inset.inline_end,
            static_position.offset.inline_offset,
            static_position.inline_edge,
        ),
        block_size: axis_size(
            logical_size.block_size,
            logical_inset.block_start,
            logical_inset.block_end,
            static_position.offset.block_offset,
            static_position.block_edge,
        ),
    })
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

#[cfg(test)]
mod tests {
    use super::logical_inset_modified_containing_block_size;
    use crate::{
        AbstractAxis, Direction, LogicalOffset, LogicalStaticPosition, Rect, Size, StaticPositionEdge,
        WritingDirection, WritingMode,
    };

    const AUTO_INSETS: Rect<Option<f32>> = Rect { left: None, right: None, top: None, bottom: None };

    fn candidate(inline_edge: StaticPositionEdge) -> LogicalStaticPosition {
        LogicalStaticPosition {
            offset: LogicalOffset { inline_offset: 20.0, block_offset: 30.0 },
            inline_edge,
            block_edge: StaticPositionEdge::Center,
            align_self_axis: AbstractAxis::Block,
        }
    }

    #[test]
    fn static_position_edge_controls_out_of_flow_available_space() {
        let direction = WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr);
        let containing_size = Size { width: 100.0, height: 80.0 };

        assert_eq!(
            logical_inset_modified_containing_block_size(
                containing_size,
                AUTO_INSETS,
                candidate(StaticPositionEdge::Start),
                direction,
            ),
            Size { width: 80.0, height: 60.0 },
        );
        assert_eq!(
            logical_inset_modified_containing_block_size(
                containing_size,
                AUTO_INSETS,
                candidate(StaticPositionEdge::Center),
                direction,
            ),
            Size { width: 40.0, height: 60.0 },
        );
        assert_eq!(
            logical_inset_modified_containing_block_size(
                containing_size,
                AUTO_INSETS,
                candidate(StaticPositionEdge::End),
                direction,
            ),
            Size { width: 20.0, height: 60.0 },
        );
    }

    #[test]
    fn logical_static_space_projects_through_vertical_writing_modes() {
        let result = logical_inset_modified_containing_block_size(
            Size { width: 80.0, height: 100.0 },
            AUTO_INSETS,
            candidate(StaticPositionEdge::End),
            WritingDirection::new(WritingMode::VerticalRl, Direction::Rtl),
        );
        assert_eq!(result, Size { width: 60.0, height: 20.0 },);
    }
}
