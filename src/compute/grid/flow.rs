use crate::geometry::{AbstractAxis, InBothAbsAxis, InBothAbstractAxis, Line, LogicalSize, Rect, Size};
use crate::{Direction, WritingDirection, WritingMode};

/// Maps CSS Grid's column/row coordinate space to physical fragments.
///
/// Track sizing and placement stay in logical inline/block axes. Only box
/// decorations and the final fragment rectangle cross this boundary.
#[derive(Clone, Copy, Debug)]
pub(super) struct GridFlow {
    /// The container's writing mode and inline direction.
    writing_direction: WritingDirection,
}

impl GridFlow {
    /// Create the coordinate mapping for a Grid formatting context.
    pub(super) const fn new(writing_mode: WritingMode, direction: Direction) -> Self {
        Self { writing_direction: WritingDirection::new(writing_mode, direction) }
    }

    /// Return the container's writing mode.
    pub(super) const fn writing_mode(self) -> WritingMode {
        self.writing_direction.mode
    }

    /// Return the complete writing direction.
    pub(super) const fn writing_direction(self) -> WritingDirection {
        self.writing_direction
    }

    /// Whether logical start is the high physical coordinate for `axis`.
    pub(super) const fn axis_is_reversed(self, axis: AbstractAxis) -> bool {
        self.writing_direction.is_logical_axis_reversed(axis)
    }

    /// Project a physical size into the Grid container's logical axes.
    pub(super) fn to_logical_size<T>(self, size: Size<T>) -> LogicalSize<T> {
        self.writing_mode().to_logical(size)
    }

    /// Project a logical Grid size back into physical axes.
    pub(super) fn to_physical_size<T>(self, size: LogicalSize<T>) -> Size<T> {
        self.writing_mode().to_physical(size)
    }

    /// Select the physical low/high sides for a logical track axis.
    /// Track offsets are stored in ascending physical coordinates even when
    /// the logical start edge is reversed.
    pub(super) fn physical_axis_line<T: Copy>(self, rect: Rect<T>, axis: AbstractAxis) -> Line<T> {
        match axis.to_absolute(self.writing_mode()) {
            crate::AbsoluteAxis::Horizontal => Line { start: rect.left, end: rect.right },
            crate::AbsoluteAxis::Vertical => Line { start: rect.top, end: rect.bottom },
        }
    }

    /// Add reserved space at the logical end edge of an axis while retaining
    /// the low/high physical line representation used by track offsets.
    pub(super) fn add_to_axis_end(self, mut line: Line<f32>, axis: AbstractAxis, amount: f32) -> Line<f32> {
        if self.axis_is_reversed(axis) {
            line.start += amount;
        } else {
            line.end += amount;
        }
        line
    }

    /// Build a physical rectangle from low-to-high coordinates in the grid's
    /// inline and block track axes.
    pub(super) fn to_physical_rect<T: Copy>(self, inline: Line<T>, block: Line<T>) -> Rect<T> {
        if self.writing_mode().is_horizontal() {
            Rect { left: inline.start, right: inline.end, top: block.start, bottom: block.end }
        } else {
            Rect { left: block.start, right: block.end, top: inline.start, bottom: inline.end }
        }
    }

    /// Reorder logical-axis values into horizontal/vertical values.
    pub(super) fn to_physical_axes<T: Copy>(self, axes: InBothAbstractAxis<T>) -> InBothAbsAxis<T> {
        if self.writing_mode().is_horizontal() {
            InBothAbsAxis { horizontal: axes.inline, vertical: axes.block }
        } else {
            InBothAbsAxis { horizontal: axes.block, vertical: axes.inline }
        }
    }
}
