use crate::geometry::{AbsoluteAxis, Point, Rect, Size};
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
