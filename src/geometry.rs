//! Geometric primitives useful for layout

use crate::util::sys::f32_max;
use crate::CompactLength;
use crate::{
    style::{BoxSizing, Dimension, Direction, ResolvedAspectRatio},
    util::sys::f32_min,
};
use core::ops::{Add, Sub};

#[cfg(feature = "flexbox")]
use crate::style::FlexDirection;

/// The simple absolute horizontal and vertical axis
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AbsoluteAxis {
    /// The horizontal axis
    Horizontal,
    /// The vertical axis
    Vertical,
}

/// The physical orientation and block-flow direction established by CSS
/// `writing-mode`.
///
/// Layout algorithms should use [`LogicalSize`] while resolving constraints
/// and convert to [`Size`] only at physical tree and fragment boundaries.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum WritingMode {
    /// Horizontal lines whose blocks progress from top to bottom.
    #[default]
    HorizontalTb,
    /// Vertical lines whose blocks progress from right to left.
    VerticalRl,
    /// Vertical lines whose blocks progress from left to right.
    VerticalLr,
    /// Sideways lines whose blocks progress from right to left.
    SidewaysRl,
    /// Sideways lines whose blocks progress from left to right.
    SidewaysLr,
}

/// The CSS writing mode and inline text direction of a formatting context.
///
/// These values jointly determine the physical location of all four logical
/// sides. Keeping them together prevents layout code from projecting sizes
/// with a writing mode while accidentally positioning boxes with an unrelated
/// or implicit direction.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WritingDirection {
    /// The orientation and block progression of the formatting context.
    pub mode: WritingMode,
    /// The inline progression of the formatting context.
    pub direction: Direction,
}

impl WritingDirection {
    /// Create a writing direction from its two CSS components.
    #[inline(always)]
    pub const fn new(mode: WritingMode, direction: Direction) -> Self {
        Self { mode, direction }
    }

    /// Whether the logical inline start lies at the high physical coordinate.
    #[inline(always)]
    pub const fn is_inline_flow_reversed(self) -> bool {
        self.mode.is_inline_flow_reversed(self.direction)
    }

    /// Whether the logical block start lies at the high physical coordinate.
    #[inline(always)]
    pub const fn is_block_flow_reversed(self) -> bool {
        self.mode.is_block_flow_reversed()
    }

    /// Whether logical start lies at the high physical coordinate for `axis`.
    #[inline(always)]
    pub const fn is_logical_axis_reversed(self, axis: AbstractAxis) -> bool {
        match axis {
            AbstractAxis::Inline => self.is_inline_flow_reversed(),
            AbstractAxis::Block => self.is_block_flow_reversed(),
        }
    }

    /// Create a converter whose offsets are relative to `outer_size`.
    #[inline(always)]
    pub const fn converter<T>(self, outer_size: Size<T>) -> WritingModeConverter<T> {
        WritingModeConverter::new(self, outer_size)
    }

    /// Convert physical box edges into logical start/end edges.
    ///
    /// Edge conversion does not depend on a containing size, so callers do
    /// not need to construct a full [`WritingModeConverter`] for struts.
    pub fn to_logical_box_strut<T: Copy>(self, rect: Rect<T>) -> LogicalBoxStrut<T> {
        let (inline_low, inline_high, block_low, block_high) = if self.mode.is_horizontal() {
            (rect.left, rect.right, rect.top, rect.bottom)
        } else {
            (rect.top, rect.bottom, rect.left, rect.right)
        };
        let (inline_start, inline_end) =
            if self.is_inline_flow_reversed() { (inline_high, inline_low) } else { (inline_low, inline_high) };
        let (block_start, block_end) =
            if self.is_block_flow_reversed() { (block_high, block_low) } else { (block_low, block_high) };
        LogicalBoxStrut { inline_start, inline_end, block_start, block_end }
    }

    /// Convert logical start/end edges back into physical box edges.
    pub fn to_physical_box_strut<T: Copy>(self, rect: LogicalBoxStrut<T>) -> Rect<T> {
        let (inline_low, inline_high) = if self.is_inline_flow_reversed() {
            (rect.inline_end, rect.inline_start)
        } else {
            (rect.inline_start, rect.inline_end)
        };
        let (block_low, block_high) = if self.is_block_flow_reversed() {
            (rect.block_end, rect.block_start)
        } else {
            (rect.block_start, rect.block_end)
        };
        if self.mode.is_horizontal() {
            Rect { left: inline_low, right: inline_high, top: block_low, bottom: block_high }
        } else {
            Rect { left: block_low, right: block_high, top: inline_low, bottom: inline_high }
        }
    }
}

impl WritingMode {
    /// Whether the inline axis is the physical horizontal axis.
    #[inline(always)]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::HorizontalTb)
    }

    /// Whether the line-under edge lies at logical block-start rather than
    /// logical block-end.
    ///
    /// This is line-relative rather than flow-relative: `vertical-lr` flips
    /// line-over/line-under while retaining left-to-right block progression.
    #[inline(always)]
    pub const fn is_line_direction_flipped(self) -> bool {
        matches!(self, Self::VerticalLr)
    }

    /// Whether this writing mode is orthogonal to `other`.
    #[inline(always)]
    pub const fn is_orthogonal_to(self, other: Self) -> bool {
        self.is_horizontal() != other.is_horizontal()
    }

    /// The physical axis corresponding to the logical inline axis.
    #[inline(always)]
    pub const fn inline_axis(self) -> AbsoluteAxis {
        if self.is_horizontal() {
            AbsoluteAxis::Horizontal
        } else {
            AbsoluteAxis::Vertical
        }
    }

    /// The physical axis corresponding to the logical block axis.
    #[inline(always)]
    pub const fn block_axis(self) -> AbsoluteAxis {
        self.inline_axis().other_axis()
    }

    /// Whether block progression runs in the reverse physical direction.
    #[inline(always)]
    pub const fn is_block_flow_reversed(self) -> bool {
        matches!(self, Self::VerticalRl | Self::SidewaysRl)
    }

    /// Whether inline progression runs in the reverse physical direction.
    ///
    /// `direction: rtl` reverses the usual inline progression in horizontal,
    /// vertical and `sideways-rl` modes. `sideways-lr` has the opposite base
    /// progression, so its inline axis is reversed for `direction: ltr`.
    #[inline(always)]
    pub const fn is_inline_flow_reversed(self, direction: Direction) -> bool {
        match self {
            Self::SidewaysLr => matches!(direction, Direction::Ltr),
            _ => matches!(direction, Direction::Rtl),
        }
    }

    /// Whether this writing mode's start side lies at the high coordinate of
    /// `axis`.
    ///
    /// The physical axis may represent either the inline or block axis. This
    /// projection is used when an item's `self-start`/`self-end` edges are
    /// compared with the containing block's start/end edges.
    #[inline(always)]
    pub const fn is_axis_flow_reversed(self, axis: AbsoluteAxis, direction: Direction) -> bool {
        let axis_is_inline =
            matches!((self.is_horizontal(), axis), (true, AbsoluteAxis::Horizontal) | (false, AbsoluteAxis::Vertical));
        if axis_is_inline {
            self.is_inline_flow_reversed(direction)
        } else {
            self.is_block_flow_reversed()
        }
    }

    /// Project a physical size into this writing mode's logical axes.
    #[inline(always)]
    pub fn to_logical<T>(self, size: Size<T>) -> LogicalSize<T> {
        if self.is_horizontal() {
            LogicalSize { inline_size: size.width, block_size: size.height }
        } else {
            LogicalSize { inline_size: size.height, block_size: size.width }
        }
    }

    /// Project a logical size in this writing mode back to physical axes.
    #[inline(always)]
    pub fn to_physical<T>(self, size: LogicalSize<T>) -> Size<T> {
        if self.is_horizontal() {
            Size { width: size.inline_size, height: size.block_size }
        } else {
            Size { width: size.block_size, height: size.inline_size }
        }
    }
}

impl AbsoluteAxis {
    /// Returns the other variant of the enum
    #[inline]
    pub const fn other_axis(&self) -> Self {
        match *self {
            AbsoluteAxis::Horizontal => AbsoluteAxis::Vertical,
            AbsoluteAxis::Vertical => AbsoluteAxis::Horizontal,
        }
    }
}

impl<T> Size<T> {
    #[inline(always)]
    /// Get either the width or height depending on the AbsoluteAxis passed in
    pub fn get_abs(self, axis: AbsoluteAxis) -> T {
        match axis {
            AbsoluteAxis::Horizontal => self.width,
            AbsoluteAxis::Vertical => self.height,
        }
    }
}

impl<T: Add> Rect<T> {
    #[inline(always)]
    /// Get either the width or height depending on the AbsoluteAxis passed in
    pub fn grid_axis_sum(self, axis: AbsoluteAxis) -> <T as Add>::Output {
        match axis {
            AbsoluteAxis::Horizontal => self.left + self.right,
            AbsoluteAxis::Vertical => self.top + self.bottom,
        }
    }
}

/// The CSS abstract axis
/// <https://www.w3.org/TR/css-writing-modes-3/#abstract-axes>
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AbstractAxis {
    /// The axis in the inline dimension, i.e. the horizontal axis in horizontal writing modes and the vertical axis in vertical writing modes.
    Inline,
    /// The axis in the block dimension, i.e. the vertical axis in horizontal writing modes and the horizontal axis in vertical writing modes.
    Block,
}

impl AbstractAxis {
    /// Returns the other variant of the enum
    #[inline]
    pub const fn other(&self) -> AbstractAxis {
        match *self {
            AbstractAxis::Inline => AbstractAxis::Block,
            AbstractAxis::Block => AbstractAxis::Inline,
        }
    }

    /// Project this logical axis into the physical axis selected by
    /// `writing_mode`.
    #[inline]
    pub const fn to_absolute(self, writing_mode: WritingMode) -> AbsoluteAxis {
        match (self, writing_mode.is_horizontal()) {
            (AbstractAxis::Inline, true) | (AbstractAxis::Block, false) => AbsoluteAxis::Horizontal,
            (AbstractAxis::Block, true) | (AbstractAxis::Inline, false) => AbsoluteAxis::Vertical,
        }
    }
}

/// Container that holds one value for each CSS logical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InBothAbstractAxis<T> {
    /// Value for the inline/column axis.
    pub inline: T,
    /// Value for the block/row axis.
    pub block: T,
}

impl<T: Copy> InBothAbstractAxis<T> {
    /// Get the value for `axis`.
    pub const fn get(&self, axis: AbstractAxis) -> T {
        match axis {
            AbstractAxis::Inline => self.inline,
            AbstractAxis::Block => self.block,
        }
    }

    /// Mutably borrow the value for `axis`.
    pub const fn get_mut(&mut self, axis: AbstractAxis) -> &mut T {
        match axis {
            AbstractAxis::Inline => &mut self.inline,
            AbstractAxis::Block => &mut self.block,
        }
    }
}

/// Container that holds an item in each absolute axis without specifying
/// what kind of item it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InBothAbsAxis<T> {
    /// The item in the horizontal axis
    pub horizontal: T,
    /// The item in the vertical axis
    pub vertical: T,
}

/// An axis-aligned UI rectangle
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Rect<T> {
    /// This can represent either the x-coordinate of the starting edge,
    /// or the amount of padding on the starting side.
    ///
    /// The starting edge is the left edge when working with LTR text,
    /// and the right edge when working with RTL text.
    pub left: T,
    /// This can represent either the x-coordinate of the ending edge,
    /// or the amount of padding on the ending side.
    ///
    /// The ending edge is the right edge when working with LTR text,
    /// and the left edge when working with RTL text.
    pub right: T,
    /// This can represent either the y-coordinate of the top edge,
    /// or the amount of padding on the top side.
    pub top: T,
    /// This can represent either the y-coordinate of the bottom edge,
    /// or the amount of padding on the bottom side.
    pub bottom: T,
}

/// A margin, border, padding or inset strut in CSS flow-relative coordinates.
///
/// This is the logical counterpart of the physical edge values in [`Rect`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LogicalBoxStrut<T> {
    /// Edge at the beginning of the inline axis.
    pub inline_start: T,
    /// Edge at the end of the inline axis.
    pub inline_end: T,
    /// Edge at the beginning of the block axis.
    pub block_start: T,
    /// Edge at the end of the block axis.
    pub block_end: T,
}

impl<U, T: Add<U>> Add<LogicalBoxStrut<U>> for LogicalBoxStrut<T> {
    type Output = LogicalBoxStrut<T::Output>;

    fn add(self, rhs: LogicalBoxStrut<U>) -> Self::Output {
        LogicalBoxStrut {
            inline_start: self.inline_start + rhs.inline_start,
            inline_end: self.inline_end + rhs.inline_end,
            block_start: self.block_start + rhs.block_start,
            block_end: self.block_end + rhs.block_end,
        }
    }
}

impl<T> LogicalBoxStrut<T> {
    /// Applies `f` to all four logical sides.
    pub fn map<R, F>(self, f: F) -> LogicalBoxStrut<R>
    where
        F: Fn(T) -> R,
    {
        LogicalBoxStrut {
            inline_start: f(self.inline_start),
            inline_end: f(self.inline_end),
            block_start: f(self.block_start),
            block_end: f(self.block_end),
        }
    }
}

impl<T, U> LogicalBoxStrut<T>
where
    T: Add<Output = U> + Copy,
{
    /// Sum of the two inline-axis sides.
    #[inline(always)]
    pub fn inline_axis_sum(&self) -> U {
        self.inline_start + self.inline_end
    }

    /// Sum of the two block-axis sides.
    #[inline(always)]
    pub fn block_axis_sum(&self) -> U {
        self.block_start + self.block_end
    }

    /// Sum both pairs of sides as a logical size.
    #[inline(always)]
    pub fn sum_axes(&self) -> LogicalSize<U> {
        LogicalSize { inline_size: self.inline_axis_sum(), block_size: self.block_axis_sum() }
    }
}

impl<U, T: Add<U>> Add<Rect<U>> for Rect<T> {
    type Output = Rect<T::Output>;

    fn add(self, rhs: Rect<U>) -> Self::Output {
        Rect {
            left: self.left + rhs.left,
            right: self.right + rhs.right,
            top: self.top + rhs.top,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl<T> Rect<T> {
    /// Applies the function `f` to all four sides of the rect
    ///
    /// When applied to the left and right sides, the width is used
    /// as the second parameter of `f`.
    /// When applied to the top or bottom sides, the height is used instead.
    #[cfg(any(feature = "flexbox", feature = "block_layout"))]
    pub(crate) fn zip_size<R, F, U>(self, size: Size<U>, f: F) -> Rect<R>
    where
        F: Fn(T, U) -> R,
        U: Copy,
    {
        Rect {
            left: f(self.left, size.width),
            right: f(self.right, size.width),
            top: f(self.top, size.height),
            bottom: f(self.bottom, size.height),
        }
    }

    /// Applies the function `f` to the left, right, top, and bottom properties
    ///
    /// This is used to transform a `Rect<T>` into a `Rect<R>`.
    pub fn map<R, F>(self, f: F) -> Rect<R>
    where
        F: Fn(T) -> R,
    {
        Rect { left: f(self.left), right: f(self.right), top: f(self.top), bottom: f(self.bottom) }
    }

    /// Returns a `Line<T>` representing the left and right properties of the Rect
    pub fn horizontal_components(self) -> Line<T> {
        Line { start: self.left, end: self.right }
    }

    /// Returns a `Line<T>` containing the top and bottom properties of the Rect
    pub fn vertical_components(self) -> Line<T> {
        Line { start: self.top, end: self.bottom }
    }
}

impl<T, U> Rect<T>
where
    T: Add<Output = U> + Copy + Clone,
{
    /// The sum of [`Rect.start`](Rect) and [`Rect.end`](Rect)
    ///
    /// This is typically used when computing total padding.
    ///
    /// **NOTE:** this is *not* the width of the rectangle.
    #[inline(always)]
    pub fn horizontal_axis_sum(&self) -> U {
        self.left + self.right
    }

    /// The sum of [`Rect.top`](Rect) and [`Rect.bottom`](Rect)
    ///
    /// This is typically used when computing total padding.
    ///
    /// **NOTE:** this is *not* the height of the rectangle.
    #[inline(always)]
    pub fn vertical_axis_sum(&self) -> U {
        self.top + self.bottom
    }

    /// Both horizontal_axis_sum and vertical_axis_sum as a `Size<T>`
    ///
    /// **NOTE:** this is *not* the width/height of the rectangle.
    #[inline(always)]
    #[allow(dead_code)] // Fixes spurious clippy warning: this function is used!
    pub fn sum_axes(&self) -> Size<U> {
        Size { width: self.horizontal_axis_sum(), height: self.vertical_axis_sum() }
    }

    /// The sum of the two fields of the [`Rect`] representing the main axis.
    ///
    /// This is typically used when computing total padding.
    ///
    /// If the [`FlexDirection`] is [`FlexDirection::Row`] or [`FlexDirection::RowReverse`], this is [`Rect::horizontal`].
    /// Otherwise, this is [`Rect::vertical`].
    #[cfg(feature = "flexbox")]
    pub(crate) fn main_axis_sum(&self, direction: FlexDirection) -> U {
        if direction.is_row() {
            self.horizontal_axis_sum()
        } else {
            self.vertical_axis_sum()
        }
    }

    /// The sum of the two fields of the [`Rect`] representing the cross axis.
    ///
    /// If the [`FlexDirection`] is [`FlexDirection::Row`] or [`FlexDirection::RowReverse`], this is [`Rect::vertical`].
    /// Otherwise, this is [`Rect::horizontal`].
    #[cfg(feature = "flexbox")]
    pub(crate) fn cross_axis_sum(&self, direction: FlexDirection) -> U {
        if direction.is_row() {
            self.vertical_axis_sum()
        } else {
            self.horizontal_axis_sum()
        }
    }
}

impl<T> Rect<T>
where
    T: Copy + Clone,
{
    /// The `start` or `top` value of the [`Rect`], from the perspective of the main layout axis
    #[cfg(feature = "flexbox")]
    pub(crate) const fn main_start(&self, direction: FlexDirection) -> T {
        if direction.is_row() {
            self.left
        } else {
            self.top
        }
    }

    /// The `end` or `bottom` value of the [`Rect`], from the perspective of the main layout axis
    #[cfg(feature = "flexbox")]
    pub(crate) const fn main_end(&self, direction: FlexDirection) -> T {
        if direction.is_row() {
            self.right
        } else {
            self.bottom
        }
    }

    /// The `start` or `top` value of the [`Rect`], from the perspective of the cross layout axis
    #[cfg(feature = "flexbox")]
    pub(crate) const fn cross_start(&self, direction: FlexDirection) -> T {
        if direction.is_row() {
            self.top
        } else {
            self.left
        }
    }

    /// The `end` or `bottom` value of the [`Rect`], from the perspective of the main layout axis
    #[cfg(feature = "flexbox")]
    pub(crate) const fn cross_end(&self, direction: FlexDirection) -> T {
        if direction.is_row() {
            self.bottom
        } else {
            self.right
        }
    }
}

impl Rect<f32> {
    /// Creates a new Rect with `0.0` as all parameters
    pub const ZERO: Rect<f32> = Self { left: 0.0, right: 0.0, top: 0.0, bottom: 0.0 };

    /// Creates a new Rect
    #[must_use]
    pub const fn new(start: f32, end: f32, top: f32, bottom: f32) -> Self {
        Self { left: start, right: end, top, bottom }
    }
}

/// An abstract "line". Represents any type that has a start and an end
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Line<T> {
    /// The start position of a line
    pub start: T,
    /// The end position of a line
    pub end: T,
}

impl<T> Line<T> {
    /// Applies the function `f` to both the width and height
    ///
    /// This is used to transform a `Line<T>` into a `Line<R>`.
    pub fn map<R, F>(self, f: F) -> Line<R>
    where
        F: Fn(T) -> R,
    {
        Line { start: f(self.start), end: f(self.end) }
    }
}

impl Line<bool> {
    /// A `Line<bool>` with both start and end set to `true`
    pub const TRUE: Self = Line { start: true, end: true };
    /// A `Line<bool>` with both start and end set to `false`
    pub const FALSE: Self = Line { start: false, end: false };
}

impl<T: Add + Copy> Line<T> {
    /// Adds the start and end values together and returns the result
    pub fn sum(&self) -> <T as Add>::Output {
        self.start + self.end
    }
}

/// The width and height of a [`Rect`]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Size<T> {
    /// The x extent of the rectangle
    pub width: T,
    /// The y extent of the rectangle
    pub height: T,
}

/// A size expressed in CSS flow-relative axes.
///
/// Unlike [`Size`], these fields retain their meaning when the writing mode is
/// vertical. This mirrors the logical geometry consumed by browser layout
/// algorithms before their results are converted into physical fragments.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LogicalSize<T> {
    /// Extent along the inline axis.
    pub inline_size: T,
    /// Extent along the block axis.
    pub block_size: T,
}

impl<T> LogicalSize<T> {
    /// Applies `f` to both logical dimensions.
    pub fn map<R, F>(self, f: F) -> LogicalSize<R>
    where
        F: Fn(T) -> R,
    {
        LogicalSize { inline_size: f(self.inline_size), block_size: f(self.block_size) }
    }

    /// Get the component for `axis`.
    pub fn get(self, axis: AbstractAxis) -> T {
        match axis {
            AbstractAxis::Inline => self.inline_size,
            AbstractAxis::Block => self.block_size,
        }
    }

    /// Replace the component for `axis`.
    pub fn set(&mut self, axis: AbstractAxis, value: T) {
        match axis {
            AbstractAxis::Inline => self.inline_size = value,
            AbstractAxis::Block => self.block_size = value,
        }
    }

    /// Return a copy with the component for `axis` replaced by `value`.
    pub fn with(mut self, axis: AbstractAxis, value: T) -> Self {
        self.set(axis, value);
        self
    }
}

impl LogicalSize<f32> {
    /// A logical size whose inline and block extents are zero.
    pub const ZERO: Self = Self { inline_size: 0.0, block_size: 0.0 };

    /// Component-wise maximum that preserves CSS floating-point semantics.
    pub(crate) fn f32_max(self, other: Self) -> Self {
        Self {
            inline_size: f32_max(self.inline_size, other.inline_size),
            block_size: f32_max(self.block_size, other.block_size),
        }
    }
}

impl<U, T: Add<U>> Add<LogicalSize<U>> for LogicalSize<T> {
    type Output = LogicalSize<T::Output>;

    fn add(self, rhs: LogicalSize<U>) -> Self::Output {
        LogicalSize { inline_size: self.inline_size + rhs.inline_size, block_size: self.block_size + rhs.block_size }
    }
}

impl<U, T: Sub<U>> Sub<LogicalSize<U>> for LogicalSize<T> {
    type Output = LogicalSize<T::Output>;

    fn sub(self, rhs: LogicalSize<U>) -> Self::Output {
        LogicalSize { inline_size: self.inline_size - rhs.inline_size, block_size: self.block_size - rhs.block_size }
    }
}

// Generic Add impl for Size<T> + Size<U> where T + U has an Add impl
impl<U, T: Add<U>> Add<Size<U>> for Size<T> {
    type Output = Size<<T as Add<U>>::Output>;

    fn add(self, rhs: Size<U>) -> Self::Output {
        Size { width: self.width + rhs.width, height: self.height + rhs.height }
    }
}

// Generic Sub impl for Size<T> + Size<U> where T + U has an Sub impl
impl<U, T: Sub<U>> Sub<Size<U>> for Size<T> {
    type Output = Size<<T as Sub<U>>::Output>;

    fn sub(self, rhs: Size<U>) -> Self::Output {
        Size { width: self.width - rhs.width, height: self.height - rhs.height }
    }
}

// Note: we allow dead_code here as we want to provide a complete API of helpers that is symmetrical in all axes,
// but sometimes we only currently have a use for the helper in a single axis
#[allow(dead_code)]
impl<T> Size<T> {
    /// Applies the function `f` to both the width and height
    ///
    /// This is used to transform a `Size<T>` into a `Size<R>`.
    pub fn map<R, F>(self, f: F) -> Size<R>
    where
        F: Fn(T) -> R,
    {
        Size { width: f(self.width), height: f(self.height) }
    }

    /// Applies the function `f` to the width
    pub fn map_width<F>(self, f: F) -> Size<T>
    where
        F: Fn(T) -> T,
    {
        Size { width: f(self.width), height: self.height }
    }

    /// Applies the function `f` to the height
    pub fn map_height<F>(self, f: F) -> Size<T>
    where
        F: Fn(T) -> T,
    {
        Size { width: self.width, height: f(self.height) }
    }

    /// Applies the function `f` to both the width and height
    /// of this value and another passed value
    pub fn zip_map<Other, Ret, Func>(self, other: Size<Other>, f: Func) -> Size<Ret>
    where
        Func: Fn(T, Other) -> Ret,
    {
        Size { width: f(self.width, other.width), height: f(self.height, other.height) }
    }

    /// Sets the extent of the main layout axis
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn set_main(&mut self, direction: FlexDirection, value: T) {
        if direction.is_row() {
            self.width = value
        } else {
            self.height = value
        }
    }

    /// Sets the extent of the cross layout axis
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn set_cross(&mut self, direction: FlexDirection, value: T) {
        if direction.is_row() {
            self.height = value
        } else {
            self.width = value
        }
    }

    /// Creates a new value of type Self with the main axis set to value provided
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn with_main(self, direction: FlexDirection, value: T) -> Self {
        let mut new = self;
        if direction.is_row() {
            new.width = value
        } else {
            new.height = value
        }
        new
    }

    /// Creates a new value of type Self with the cross axis set to value provided
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn with_cross(self, direction: FlexDirection, value: T) -> Self {
        let mut new = self;
        if direction.is_row() {
            new.height = value
        } else {
            new.width = value
        }
        new
    }

    /// Creates a new value of type Self with the main axis modified by the callback provided
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn map_main(self, direction: FlexDirection, mapper: impl FnOnce(T) -> T) -> Self {
        let mut new = self;
        if direction.is_row() {
            new.width = mapper(new.width);
        } else {
            new.height = mapper(new.height);
        }
        new
    }

    /// Creates a new value of type Self with the cross axis modified by the callback provided
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn map_cross(self, direction: FlexDirection, mapper: impl FnOnce(T) -> T) -> Self {
        let mut new = self;
        if direction.is_row() {
            new.height = mapper(new.height);
        } else {
            new.width = mapper(new.width);
        }
        new
    }

    /// Gets the extent of the main layout axis
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn main(self, direction: FlexDirection) -> T {
        if direction.is_row() {
            self.width
        } else {
            self.height
        }
    }

    /// Gets the extent of the cross layout axis
    ///
    /// Whether this is the width or height depends on the `direction` provided
    #[cfg(feature = "flexbox")]
    pub(crate) fn cross(self, direction: FlexDirection) -> T {
        if direction.is_row() {
            self.height
        } else {
            self.width
        }
    }
}

impl Size<f32> {
    /// A [`Size`] with zero width and height
    pub const ZERO: Size<f32> = Self { width: 0.0, height: 0.0 };

    /// Applies f32_max to each component separately
    #[inline(always)]
    pub fn f32_max(self, rhs: Size<f32>) -> Size<f32> {
        Size { width: f32_max(self.width, rhs.width), height: f32_max(self.height, rhs.height) }
    }

    /// Applies f32_min to each component separately
    #[inline(always)]
    pub fn f32_min(self, rhs: Size<f32>) -> Size<f32> {
        Size { width: f32_min(self.width, rhs.width), height: f32_min(self.height, rhs.height) }
    }

    /// Return true if both width and height are greater than 0 else false
    #[inline(always)]
    pub fn has_non_zero_area(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

impl Size<Option<f32>> {
    /// A [`Size`] with `None` width and height
    pub const NONE: Size<Option<f32>> = Self { width: None, height: None };

    /// A [`Size<Option<f32>>`] with `Some(width)` and `Some(height)` as parameters
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Size { width: Some(width), height: Some(height) }
    }

    /// Creates a new [`Size<Option<f32>>`] with either the width or height set based on the provided `direction`
    #[cfg(feature = "flexbox")]
    pub const fn from_cross(direction: FlexDirection, value: Option<f32>) -> Self {
        let mut new = Self::NONE;
        if direction.is_row() {
            new.height = value
        } else {
            new.width = value
        }
        new
    }

    /// Applies aspect_ratio (if one is supplied) to the Size:
    ///   - If width is `Some` but height is `None`, then height is computed from width and aspect_ratio
    ///   - If height is `Some` but width is `None`, then width is computed from height and aspect_ratio
    ///
    /// If aspect_ratio is `None` then this function simply returns self.
    pub fn maybe_apply_aspect_ratio(self, aspect_ratio: Option<f32>) -> Size<Option<f32>> {
        match aspect_ratio {
            Some(ratio) => match (self.width, self.height) {
                (Some(width), None) => Size { width: Some(width), height: Some(width / ratio) },
                (None, Some(height)) => Size { width: Some(height * ratio), height: Some(height) },
                _ => self,
            },
            None => self,
        }
    }

    /// Applies an aspect ratio while allowing the source dimensions and the
    /// ratio itself to refer to different CSS sizing boxes.
    ///
    /// This distinction is observable for `aspect-ratio: auto <ratio>` and
    /// intrinsic replaced-element ratios: those constrain the content box,
    /// even when `box-sizing: border-box` makes authored sizes refer to the
    /// border box.
    pub fn maybe_apply_aspect_ratio_with_box_sizing(
        self,
        aspect_ratio: Option<ResolvedAspectRatio>,
        source_box_sizing: BoxSizing,
        padding_border: Size<f32>,
    ) -> Size<Option<f32>> {
        let Some(aspect_ratio) = aspect_ratio else {
            return self;
        };
        let ratio = aspect_ratio.ratio();
        let convert = |value: f32, inset: f32, from: BoxSizing, to: BoxSizing| match (from, to) {
            (BoxSizing::ContentBox, BoxSizing::BorderBox) => value + inset,
            (BoxSizing::BorderBox, BoxSizing::ContentBox) => (value - inset).max(0.0),
            _ => value,
        };
        match (self.width, self.height) {
            (Some(width), None) => {
                let ratio_width = convert(width, padding_border.width, source_box_sizing, aspect_ratio.sizing_box());
                let ratio_height = ratio_width / ratio;
                let source_height =
                    convert(ratio_height, padding_border.height, aspect_ratio.sizing_box(), source_box_sizing);
                Size { width: Some(width), height: source_height.is_finite().then_some(source_height.max(0.0)) }
            }
            (None, Some(height)) => {
                let ratio_height = convert(height, padding_border.height, source_box_sizing, aspect_ratio.sizing_box());
                let ratio_width = ratio_height * ratio;
                let source_width =
                    convert(ratio_width, padding_border.width, aspect_ratio.sizing_box(), source_box_sizing);
                Size { width: source_width.is_finite().then_some(source_width.max(0.0)), height: Some(height) }
            }
            _ => self,
        }
    }
}

#[cfg(test)]
mod aspect_ratio_tests {
    use super::Size;
    use crate::{BoxSizing, ResolvedAspectRatio};

    #[test]
    fn aspect_ratio_uses_its_own_sizing_box() {
        let border_box_width = Size { width: Some(100.0), height: None };
        let padding_border = Size { width: 20.0, height: 20.0 };

        let content_box_ratio = border_box_width.maybe_apply_aspect_ratio_with_box_sizing(
            ResolvedAspectRatio::new(2.0, BoxSizing::ContentBox),
            BoxSizing::BorderBox,
            padding_border,
        );
        let border_box_ratio = border_box_width.maybe_apply_aspect_ratio_with_box_sizing(
            ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox),
            BoxSizing::BorderBox,
            padding_border,
        );

        assert_eq!(content_box_ratio, Size { width: Some(100.0), height: Some(60.0) });
        assert_eq!(border_box_ratio, Size { width: Some(100.0), height: Some(50.0) });
    }
}

impl<T> Size<Option<T>> {
    /// Performs Option::unwrap_or on each component separately
    pub fn unwrap_or(self, alt: Size<T>) -> Size<T> {
        Size { width: self.width.unwrap_or(alt.width), height: self.height.unwrap_or(alt.height) }
    }

    /// Performs Option::or on each component separately
    pub fn or(self, alt: Size<Option<T>>) -> Size<Option<T>> {
        Size { width: self.width.or(alt.width), height: self.height.or(alt.height) }
    }

    /// Return true if both components are Some, else false.
    #[inline(always)]
    pub fn both_axis_defined(&self) -> bool {
        self.width.is_some() && self.height.is_some()
    }
}

impl Size<Dimension> {
    /// Generates a [`Size<Dimension>`] using length values
    #[must_use]
    pub const fn from_lengths(width: f32, height: f32) -> Self {
        Size { width: Dimension(CompactLength::length(width)), height: Dimension(CompactLength::length(height)) }
    }

    /// Generates a [`Size<Dimension>`] using percentage values
    #[must_use]
    pub const fn from_percent(width: f32, height: f32) -> Self {
        Size { width: Dimension(CompactLength::percent(width)), height: Dimension(CompactLength::percent(height)) }
    }
}

/// A two-dimensional offset expressed in CSS flow-relative coordinates.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LogicalOffset<T> {
    /// Offset from the inline-start edge.
    pub inline_offset: T,
    /// Offset from the block-start edge.
    pub block_offset: T,
}

impl<U, T: Add<U>> Add<LogicalOffset<U>> for LogicalOffset<T> {
    type Output = LogicalOffset<T::Output>;

    fn add(self, rhs: LogicalOffset<U>) -> Self::Output {
        LogicalOffset {
            inline_offset: self.inline_offset + rhs.inline_offset,
            block_offset: self.block_offset + rhs.block_offset,
        }
    }
}

impl<U, T: Sub<U>> Sub<LogicalOffset<U>> for LogicalOffset<T> {
    type Output = LogicalOffset<T::Output>;

    fn sub(self, rhs: LogicalOffset<U>) -> Self::Output {
        LogicalOffset {
            inline_offset: self.inline_offset - rhs.inline_offset,
            block_offset: self.block_offset - rhs.block_offset,
        }
    }
}

impl<T> LogicalOffset<T> {
    /// Applies `f` to both logical offsets.
    pub fn map<R, F>(self, f: F) -> LogicalOffset<R>
    where
        F: Fn(T) -> R,
    {
        LogicalOffset { inline_offset: f(self.inline_offset), block_offset: f(self.block_offset) }
    }
}

impl LogicalOffset<f32> {
    /// A logical point at inline-start/block-start.
    pub const ZERO: Self = Self { inline_offset: 0.0, block_offset: 0.0 };
}

/// A 2-dimensional coordinate.
///
/// When used in association with a [`Rect`], represents the top-left corner.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Point<T> {
    /// The x-coordinate
    pub x: T,
    /// The y-coordinate
    pub y: T,
}

impl Point<f32> {
    /// A [`Point`] with values (0,0), representing the origin
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

impl Point<Option<f32>> {
    /// A [`Point`] with values (None, None)
    pub const NONE: Self = Self { x: None, y: None };
}

// Generic Add impl for Point<T> + Point<U> where T + U has an Add impl
impl<U, T: Add<U>> Add<Point<U>> for Point<T> {
    type Output = Point<<T as Add<U>>::Output>;

    fn add(self, rhs: Point<U>) -> Self::Output {
        Point { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl<T> Point<T> {
    /// Applies the function `f` to both the x and y
    ///
    /// This is used to transform a `Point<T>` into a `Point<R>`.
    pub fn map<R, F>(self, f: F) -> Point<R>
    where
        F: Fn(T) -> R,
    {
        Point { x: f(self.x), y: f(self.y) }
    }

    /// Gets the extent of the specified layout axis
    /// Whether this is the width or height depends on the `GridAxis` provided
    #[cfg(feature = "grid")]
    pub fn get(self, axis: AbstractAxis) -> T {
        match axis {
            AbstractAxis::Inline => self.x,
            AbstractAxis::Block => self.y,
        }
    }

    /// Swap x and y components
    pub fn transpose(self) -> Point<T> {
        Point { x: self.y, y: self.x }
    }

    /// Sets the extent of the specified layout axis
    /// Whether this is the width or height depends on the `GridAxis` provided
    #[cfg(feature = "grid")]
    pub fn set(&mut self, axis: AbstractAxis, value: T) {
        match axis {
            AbstractAxis::Inline => self.x = value,
            AbstractAxis::Block => self.y = value,
        }
    }
}

impl<T> From<Point<T>> for Size<T> {
    fn from(value: Point<T>) -> Self {
        Size { width: value.x, height: value.y }
    }
}

/// Converts geometry between physical and CSS flow-relative coordinate spaces.
///
/// Offset conversion needs the containing rectangle's size because inline or
/// block start can lie on the physical right or bottom edge. It also needs the
/// child size so that the returned physical point is always the child's
/// top-left corner, matching [`Layout::location`](crate::tree::Layout::location).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WritingModeConverter<T> {
    /// Logical axes and their physical progression directions.
    writing_direction: WritingDirection,
    /// Physical containing size used to resolve reversed offsets.
    outer_size: Size<T>,
}

impl<T> WritingModeConverter<T> {
    /// Create a converter for descendants of a physical rectangle.
    #[inline(always)]
    pub const fn new(writing_direction: WritingDirection, outer_size: Size<T>) -> Self {
        Self { writing_direction, outer_size }
    }

    /// The writing mode and direction used by this converter.
    #[inline(always)]
    pub const fn writing_direction(&self) -> WritingDirection {
        self.writing_direction
    }

    /// The physical size offsets are relative to.
    #[inline(always)]
    pub const fn outer_size(&self) -> &Size<T> {
        &self.outer_size
    }
}

impl<T: Copy> WritingModeConverter<T> {
    /// Project a physical size into the converter's logical axes.
    #[inline(always)]
    pub fn to_logical_size(&self, size: Size<T>) -> LogicalSize<T> {
        self.writing_direction.mode.to_logical(size)
    }

    /// Project a logical size back into physical axes.
    #[inline(always)]
    pub fn to_physical_size(&self, size: LogicalSize<T>) -> Size<T> {
        self.writing_direction.mode.to_physical(size)
    }

    /// Convert physical box edges into logical start/end edges.
    pub fn to_logical_box_strut(&self, rect: Rect<T>) -> LogicalBoxStrut<T> {
        self.writing_direction.to_logical_box_strut(rect)
    }

    /// Convert logical start/end edges back into physical box edges.
    pub fn to_physical_box_strut(&self, rect: LogicalBoxStrut<T>) -> Rect<T> {
        self.writing_direction.to_physical_box_strut(rect)
    }
}

impl<T> WritingModeConverter<T>
where
    T: Copy + Sub<Output = T>,
{
    /// Convert a logical child offset to its physical top-left point.
    pub fn to_physical_point(&self, offset: LogicalOffset<T>, inner_size: Size<T>) -> Point<T> {
        let outer_size = self.to_logical_size(self.outer_size);
        let inner_size = self.to_logical_size(inner_size);
        let inline_offset = if self.writing_direction.is_inline_flow_reversed() {
            outer_size.inline_size - offset.inline_offset - inner_size.inline_size
        } else {
            offset.inline_offset
        };
        let block_offset = if self.writing_direction.is_block_flow_reversed() {
            outer_size.block_size - offset.block_offset - inner_size.block_size
        } else {
            offset.block_offset
        };
        if self.writing_direction.mode.is_horizontal() {
            Point { x: inline_offset, y: block_offset }
        } else {
            Point { x: block_offset, y: inline_offset }
        }
    }

    /// Convert a physical child top-left point to its logical offset.
    pub fn to_logical_point(&self, offset: Point<T>, inner_size: Size<T>) -> LogicalOffset<T> {
        let outer_size = self.to_logical_size(self.outer_size);
        let inner_size = self.to_logical_size(inner_size);
        let (inline_offset, block_offset) =
            if self.writing_direction.mode.is_horizontal() { (offset.x, offset.y) } else { (offset.y, offset.x) };
        LogicalOffset {
            inline_offset: if self.writing_direction.is_inline_flow_reversed() {
                outer_size.inline_size - inline_offset - inner_size.inline_size
            } else {
                inline_offset
            },
            block_offset: if self.writing_direction.is_block_flow_reversed() {
                outer_size.block_size - block_offset - inner_size.block_size
            } else {
                block_offset
            },
        }
    }
}

#[cfg(test)]
mod writing_mode_tests {
    use super::{LogicalBoxStrut, LogicalOffset, Point, Rect, Size, WritingDirection, WritingMode};
    use crate::Direction;

    const OUTER_SIZE: Size<i32> = Size { width: 300, height: 400 };
    const INNER_SIZE: Size<i32> = Size { width: 5, height: 65 };

    const WRITING_DIRECTIONS: [WritingDirection; 10] = [
        WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
        WritingDirection::new(WritingMode::HorizontalTb, Direction::Rtl),
        WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr),
        WritingDirection::new(WritingMode::VerticalRl, Direction::Rtl),
        WritingDirection::new(WritingMode::VerticalLr, Direction::Ltr),
        WritingDirection::new(WritingMode::VerticalLr, Direction::Rtl),
        WritingDirection::new(WritingMode::SidewaysRl, Direction::Ltr),
        WritingDirection::new(WritingMode::SidewaysRl, Direction::Rtl),
        WritingDirection::new(WritingMode::SidewaysLr, Direction::Ltr),
        WritingDirection::new(WritingMode::SidewaysLr, Direction::Rtl),
    ];

    #[test]
    fn logical_offsets_convert_to_physical_top_left_points() {
        let logical = LogicalOffset { inline_offset: 20, block_offset: 30 };
        let expected = [
            Point { x: 20, y: 30 },
            Point { x: 275, y: 30 },
            Point { x: 265, y: 20 },
            Point { x: 265, y: 315 },
            Point { x: 30, y: 20 },
            Point { x: 30, y: 315 },
            Point { x: 265, y: 20 },
            Point { x: 265, y: 315 },
            Point { x: 30, y: 315 },
            Point { x: 30, y: 20 },
        ];

        for (writing_direction, expected) in WRITING_DIRECTIONS.into_iter().zip(expected) {
            assert_eq!(
                writing_direction.converter(OUTER_SIZE).to_physical_point(logical, INNER_SIZE),
                expected,
                "{writing_direction:?}",
            );
        }
    }

    #[test]
    fn physical_top_left_points_convert_to_logical_offsets() {
        let physical = Point { x: 20, y: 30 };
        let expected = [
            LogicalOffset { inline_offset: 20, block_offset: 30 },
            LogicalOffset { inline_offset: 275, block_offset: 30 },
            LogicalOffset { inline_offset: 30, block_offset: 275 },
            LogicalOffset { inline_offset: 305, block_offset: 275 },
            LogicalOffset { inline_offset: 30, block_offset: 20 },
            LogicalOffset { inline_offset: 305, block_offset: 20 },
            LogicalOffset { inline_offset: 30, block_offset: 275 },
            LogicalOffset { inline_offset: 305, block_offset: 275 },
            LogicalOffset { inline_offset: 305, block_offset: 20 },
            LogicalOffset { inline_offset: 30, block_offset: 20 },
        ];

        for (writing_direction, expected) in WRITING_DIRECTIONS.into_iter().zip(expected) {
            assert_eq!(
                writing_direction.converter(OUTER_SIZE).to_logical_point(physical, INNER_SIZE),
                expected,
                "{writing_direction:?}",
            );
        }
    }

    #[test]
    fn physical_and_logical_box_edges_round_trip() {
        let physical = Rect { left: 1, right: 2, top: 3, bottom: 4 };
        let expected = [
            LogicalBoxStrut { inline_start: 1, inline_end: 2, block_start: 3, block_end: 4 },
            LogicalBoxStrut { inline_start: 2, inline_end: 1, block_start: 3, block_end: 4 },
            LogicalBoxStrut { inline_start: 3, inline_end: 4, block_start: 2, block_end: 1 },
            LogicalBoxStrut { inline_start: 4, inline_end: 3, block_start: 2, block_end: 1 },
            LogicalBoxStrut { inline_start: 3, inline_end: 4, block_start: 1, block_end: 2 },
            LogicalBoxStrut { inline_start: 4, inline_end: 3, block_start: 1, block_end: 2 },
            LogicalBoxStrut { inline_start: 3, inline_end: 4, block_start: 2, block_end: 1 },
            LogicalBoxStrut { inline_start: 4, inline_end: 3, block_start: 2, block_end: 1 },
            LogicalBoxStrut { inline_start: 4, inline_end: 3, block_start: 1, block_end: 2 },
            LogicalBoxStrut { inline_start: 3, inline_end: 4, block_start: 1, block_end: 2 },
        ];

        for (writing_direction, expected) in WRITING_DIRECTIONS.into_iter().zip(expected) {
            let converter = writing_direction.converter(OUTER_SIZE);
            let logical = converter.to_logical_box_strut(physical);
            assert_eq!(logical, expected, "{writing_direction:?}");
            assert_eq!(converter.to_physical_box_strut(logical), physical, "{writing_direction:?}");
        }
    }

    #[test]
    fn vertical_rl_block_start_maps_to_physical_right() {
        let converter =
            WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr).converter(Size { width: 100, height: 200 });
        assert_eq!(
            converter
                .to_physical_point(LogicalOffset { inline_offset: 0, block_offset: 0 }, Size { width: 50, height: 10 },),
            Point { x: 50, y: 0 },
        );
    }
}

/// Generic struct which holds a "min" value and a "max" value
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MinMax<Min, Max> {
    /// The value representing the minimum
    pub min: Min,
    /// The value representing the maximum
    pub max: Max,
}
