use crate::geometry::{AbsoluteAxis, Line, Size};
use crate::style::AvailableSpace;
use crate::tree::{LayoutPartialTree, LayoutPartialTreeExt, NodeId, SizingMode};

/// Resolves the fit-content width used by an auto-width absolutely positioned box.
///
/// CSS 2 defines this as `min(max(min-content, available), max-content)`. A single
/// measurement with definite available space is insufficient: nested block and flex
/// containers may return their max-content contribution while they are being measured.
#[inline]
pub(crate) fn fit_content_width(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    known_dimensions: Size<Option<f32>>,
    parent_size: Size<Option<f32>>,
    available_height: AvailableSpace,
    available_width: f32,
    sizing_mode: SizingMode,
) -> f32 {
    let min_content = tree.measure_child_size(
        node,
        known_dimensions,
        parent_size,
        Size { width: AvailableSpace::MinContent, height: available_height },
        sizing_mode,
        AbsoluteAxis::Horizontal,
        Line::FALSE,
    );
    let max_content = tree.measure_child_size(
        node,
        known_dimensions,
        parent_size,
        Size { width: AvailableSpace::MaxContent, height: available_height },
        sizing_mode,
        AbsoluteAxis::Horizontal,
        Line::FALSE,
    );

    available_width.max(0.0).max(min_content).min(max_content)
}
