//! Computes the [flexbox](https://css-tricks.com/snippets/css/a-guide-to-flexbox/) layout algorithm on [`TaffyTree`](crate::TaffyTree) according to the [spec](https://www.w3.org/TR/css-flexbox-1/)
use crate::compute::common::alignment::compute_alignment_offset;
use crate::compute::common::baseline::{
    determine_baseline_group, determine_baseline_writing_mode, logical_block_baseline,
    logical_block_baseline_or_synthesize, physical_baseline, synthesized_logical_baseline, BaselineGroup, FontBaseline,
};
use crate::geometry::{
    AbsoluteAxis, AbstractAxis, Line, LogicalOffset, LogicalSize, LogicalStaticPosition, Point, Rect, Size,
    StaticPositionEdge, WritingDirection, WritingMode,
};
use crate::style::{
    AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword, AlignSelf, AvailableSpace, Dimension, FlexWrap,
    JustifyContent, LengthPercentage, LengthPercentageAuto, Overflow, Position, ResolvedAspectRatio,
};
use crate::style::{CoreStyle, FlexDirection, FlexboxContainerStyle, FlexboxItemStyle};
use crate::style_helpers::{TaffyMaxContent, TaffyMinContent};
use crate::tree::{
    AutoSizeBehavior, ChildLayoutInput, Layout, LayoutInput, LayoutOutput, RunMode, SizingMode, SizingPurpose,
};
use crate::tree::{LayoutFlexboxContainer, LayoutPartialTreeExt, NodeId};
use crate::util::debug::debug_log;
use crate::util::sys::{f32_max, new_vec_with_capacity, Vec};
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{BoxGenerationMode, BoxSizing, Direction, RequestedAxis};

use super::common::absolute::{layout_out_of_flow_item, OutOfFlowItem};
use super::common::alignment::apply_alignment_fallback;
use super::common::aspect_ratio::{
    resolve_size_constraints, ResolvedAxisConstraints, SizeConstraintInput, TransferredSizesMode,
};
#[cfg(feature = "content_size")]
use super::common::content_size::compute_content_size_contribution;
use super::common::intrinsic_size::{
    fit_content_inline_size_with_metadata, intrinsic_content_size_from_initial_geometry,
    measure_aspect_ratio_automatic_minimum, measure_child_intrinsic_contribution,
    resolve_content_based_block_size_constraints, resolve_intrinsic_axis_constraints,
    resolve_intrinsic_preferred_axis_size, resolve_minimum_size, resolve_node_size_constraints, BlockSizeProperties,
    ContentBasedBlockSize, IntrinsicAxisInput, IntrinsicAxisValue, NodeSizeConstraintInput, ResolvedNodeSizing,
};
use super::common::stretch::StretchSizeProperties;
use crate::tree::OutOfFlowContainingBlock;

/// The result of resolving `flex-basis`, including the `auto` indirection
/// through the preferred main size.
#[derive(Clone, Copy, Debug, PartialEq)]
enum UsedFlexBasis {
    /// The flex basis resolved to a border-box size. The value may itself have
    /// come from an intrinsic preferred size such as `min-content`.
    Resolved(f32),
    /// The `content` value, an automatic main size, or an unresolved numeric
    /// value that falls back to the item's content.
    Content,
    /// An explicit intrinsic sizing function that must retain its own
    /// min-/max-/fit-content constraint during measurement.
    Intrinsic(Dimension),
    /// Stretch the item's margin box into definite main-axis available space.
    Stretch,
}

/// Intrinsic inline-size algorithm requested from the flex container.
///
/// A wrapped column container has a dedicated intrinsic inline-size
/// operation: its max-content contribution follows the columns formed under
/// the block constraint, while its min-content contribution is the largest
/// item contribution. This state is deliberately independent from intrinsic
/// block-size measurement: a caller can request both physical axes without
/// making one operation erase the other.
#[derive(Clone, Copy, Debug, PartialEq)]
enum FlexIntrinsicInlineSize {
    /// Intrinsic main sizing for a row flex container under the given
    /// min-/max-content constraint.
    Row(AvailableSpace),
    /// Intrinsic inline sizing for a single-line column flex container.
    ///
    /// A column's intrinsic inline contribution is the largest item
    /// contribution. It is deliberately computed before flexible-length
    /// resolution so a flexed block size cannot feed back through an aspect
    /// ratio and manufacture an inline contribution.
    Column(AvailableSpace),
    /// Intrinsic inline sizing for a wrapped column under the given
    /// min-/max-content constraint.
    ColumnWrap(AvailableSpace),
}

impl UsedFlexBasis {
    /// Preserve the semantic source of a basis that ordinary
    /// length-percentage resolution could not reduce to a number.
    fn from_unresolved_dimension(value: Dimension) -> Self {
        if value.is_intrinsic() {
            Self::Intrinsic(value)
        } else if value.is_stretch() {
            Self::Stretch
        } else {
            Self::Content
        }
    }

    /// Whether the main-axis basis still needs intrinsic or available-space
    /// resolution.
    fn is_unresolved(self) -> bool {
        !matches!(self, Self::Resolved(_))
    }
}

/// The container's logical flex flow normalized into the physical coordinate
/// system used by [`Layout`].
///
/// CSS defines row/column and their start edges in flow-relative axes, while
/// Taffy stores fragments in x/y coordinates. Keeping that projection here
/// gives the rest of the algorithm one coherent physical main/cross space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlexFlow {
    /// Main-axis orientation and reversal in physical coordinates.
    direction: FlexDirection,
    /// Whether the authored `flex-direction` reverses flex-start and flex-end.
    ///
    /// This remains logical: writing-mode and `direction` reversal is applied
    /// only when a logical position is projected into physical coordinates.
    authored_main_reversed: bool,
    /// Direction of the physical horizontal axis, wherever it appears in the
    /// main/cross pair.
    horizontal_direction: Direction,
    /// Whether the main axis is the container's logical inline axis.
    main_axis_is_inline: bool,
    /// Whether the container's logical cross-start lies at the physical high
    /// coordinate of the cross axis.
    cross_axis_start_reversed: bool,
    /// Whether flex-start lies at the physical high coordinate of the cross
    /// axis after applying `wrap-reverse`.
    cross_axis_flex_start_reversed: bool,
    /// Whether the physical cross axis is reversed after combining its base
    /// writing-mode direction with `wrap-reverse`.
    cross_axis_reversed: bool,
}

impl FlexFlow {
    /// Resolve a logical flex flow into the physical coordinate system used by
    /// Taffy's stored layout rectangles.
    fn resolve(
        flex_direction: FlexDirection,
        flex_wrap: FlexWrap,
        writing_mode: WritingMode,
        inline_direction: Direction,
    ) -> Self {
        let main_axis_is_inline = flex_direction.is_row();
        let authored_main_reversed = flex_direction.is_reverse();
        let authored_cross_reversed = flex_wrap == FlexWrap::WrapReverse;
        let main_axis = if main_axis_is_inline { writing_mode.inline_axis() } else { writing_mode.block_axis() };
        let base_main_reversed = if main_axis_is_inline {
            writing_mode.is_inline_flow_reversed(inline_direction)
        } else {
            writing_mode.is_block_flow_reversed()
        };
        let base_cross_reversed = if main_axis_is_inline {
            writing_mode.is_block_flow_reversed()
        } else {
            writing_mode.is_inline_flow_reversed(inline_direction)
        };

        // Horizontal start-edge reversal is represented by `Direction`; a
        // vertical start-edge reversal is represented by the normalized flex
        // direction or cross-axis reversal flag.
        let main_is_horizontal = main_axis == AbsoluteAxis::Horizontal;
        let physical_main_reversed = authored_main_reversed ^ (!main_is_horizontal && base_main_reversed);
        let direction = match (main_axis, physical_main_reversed) {
            (AbsoluteAxis::Horizontal, false) => FlexDirection::Row,
            (AbsoluteAxis::Horizontal, true) => FlexDirection::RowReverse,
            (AbsoluteAxis::Vertical, false) => FlexDirection::Column,
            (AbsoluteAxis::Vertical, true) => FlexDirection::ColumnReverse,
        };
        let horizontal_axis_reversed = if main_is_horizontal { base_main_reversed } else { base_cross_reversed };
        let horizontal_direction = if horizontal_axis_reversed { Direction::Rtl } else { Direction::Ltr };
        let cross_axis_reversed = authored_cross_reversed ^ (main_is_horizontal && base_cross_reversed);

        Self {
            direction,
            authored_main_reversed,
            horizontal_direction,
            main_axis_is_inline,
            cross_axis_start_reversed: base_cross_reversed,
            cross_axis_flex_start_reversed: base_cross_reversed ^ authored_cross_reversed,
            cross_axis_reversed,
        }
    }
}

/// Inputs that Flexbox section 4.5 combines with the item's content-size
/// suggestion to obtain its content-based automatic minimum.
#[derive(Clone, Copy, Debug)]
struct FlexAutomaticMinimum {
    /// Whether content and transferred suggestions use replaced-element
    /// ordering (smaller) or non-replaced ordering (larger).
    is_replaced: bool,
    /// Definite preferred main size before aspect-ratio transfer.
    specified_size_suggestion: Option<f32>,
    /// Definite preferred cross size, clamped in the cross axis and converted
    /// into the main axis through the preferred aspect ratio.
    transferred_size_suggestion: Option<f32>,
}

impl FlexAutomaticMinimum {
    /// Combine the three Flexbox sizing suggestions and apply the remaining
    /// direct main-axis caps. Every value at this boundary is a border-box
    /// size.
    #[inline]
    fn resolve(self, content_size_suggestion: f32, maximum_main_size: Option<f32>, padding_border: f32) -> f32 {
        let content_and_transferred = if self.is_replaced {
            content_size_suggestion.maybe_min(self.transferred_size_suggestion)
        } else {
            content_size_suggestion.maybe_max(self.transferred_size_suggestion)
        };

        content_and_transferred
            .maybe_min(self.specified_size_suggestion)
            .maybe_min(maximum_main_size)
            .max(padding_border)
    }
}

/// The intermediate results of a flexbox calculation for a single item
struct FlexItem {
    /// The identifier for the associated node
    node: NodeId,

    /// The order of the node relative to it's siblings
    order: u32,

    /// The base size of this item
    size: Size<Option<f32>>,
    /// Axes in `size` synthesized from the pre-flex preferred aspect ratio.
    ///
    /// Flexible-length resolution can replace the main size after this value
    /// is computed. The corresponding cross size must then be transferred
    /// again from the flexed main size instead of being mistaken for a direct
    /// authored cross size.
    preferred_size_aspect_ratio_applied: Size<bool>,
    /// The preferred size after aspect-ratio resolution.
    ///
    /// A content-based flex basis ignores this in the flex base-size and
    /// hypothetical cross-size calculations. Flex-item intrinsic size
    /// contributions still use a non-automatic preferred main size.
    preferred_size: Size<Option<f32>>,
    /// Source-preserving inputs to the Flexbox automatic-minimum calculation.
    automatic_minimum: FlexAutomaticMinimum,
    /// The preferred size before transferring a dimension through `aspect-ratio`
    /// or applying a content-box padding/border adjustment.
    ///
    /// `flex-basis: content` must ignore a preferred main size, but it may use
    /// an independently definite cross size with the intrinsic aspect ratio.
    untransferred_size: Size<Option<f32>>,
    /// The minimum allowable size with aspect-ratio transfers ignored.
    ///
    /// Flexible-length resolution uses this constraint; transferred constraints
    /// only affect the hypothetical size.
    min_size: Size<Option<f32>>,
    /// The maximum allowable size with aspect-ratio transfers ignored.
    max_size: Size<Option<f32>>,
    /// The minimum allowable size when aspect-ratio transfers participate.
    min_size_with_transfer: Size<Option<f32>>,
    /// The maximum allowable size when aspect-ratio transfers participate.
    max_size_with_transfer: Size<Option<f32>>,
    /// Shared logical block-size resolver used once flex sizing establishes
    /// the item's final inline-axis geometry.
    content_based_block_size: ContentBasedBlockSize,
    /// Logical block-axis sources with preferred-ratio transfers disabled.
    ///
    /// Flex uses these constraints while stretching the final cross size,
    /// whereas the hypothetical cross size also consumes transferred limits.
    block_axis_constraints_without_transfer: ResolvedAxisConstraints,
    /// The used aspect ratio and the CSS sizing box that it constrains.
    aspect_ratio: ResolvedAspectRatio,
    /// Whether this item's inline axis is the container's flex main axis.
    ///
    /// Intrinsic inline sizing retains a ratio-independent contribution,
    /// while intrinsic block sizing applies transferred inline min/max
    /// constraints to the content contribution. This is a child writing-mode
    /// relation, not merely a row/column property of the container.
    main_axis_is_inline: bool,
    /// The CSS sizing box used by authored size properties.
    box_sizing: BoxSizing,
    /// Authored stretch constraints retained until the flex line's cross size
    /// is definite.
    stretch: StretchSizeProperties,
    /// Resolved flex basis state. This retains whether resolution needed a
    /// content-based fallback, rather than inferring it from CSS syntax.
    used_flex_basis: UsedFlexBasis,
    /// Whether the used flex basis was resolved without content layout.
    used_flex_basis_is_definite: bool,
    /// Whether the preferred cross size is definite before line sizing.
    preferred_cross_size_is_definite: bool,
    /// Whether flex sizing makes the final main size definite for descendants.
    main_size_is_definite: bool,
    /// Whether flex cross-size resolution makes the final cross size definite.
    cross_size_is_definite: bool,
    /// Whether this item's intrinsic contribution depends on the flex
    /// container's block-size.
    depends_on_block_constraints: bool,
    /// The cross-alignment of this item
    align_self: AlignSelf,
    /// Writing mode used to read or synthesize this item's alignment baseline.
    baseline_writing_mode: WritingMode,
    /// Baseline-sharing group used on the flex line's cross axis.
    baseline_group: BaselineGroup,

    /// The overflow style of the item
    overflow: Point<Overflow>,
    /// The width of the scrollbars (if it has any)
    scrollbar_width: f32,
    /// The flex shrink style of the item
    flex_shrink: f32,
    /// The flex grow style of the item
    flex_grow: f32,

    /// The minimum size of the item. This differs from min_size above because it also
    /// takes into account content based automatic minimum sizes
    resolved_minimum_main_size: f32,

    /// The final offset of this item
    inset: Rect<Option<f32>>,
    /// The margin of this item
    margin: Rect<f32>,
    /// Whether each margin is an auto margin or not
    margin_is_auto: Rect<bool>,
    /// The padding of this item
    padding: Rect<f32>,
    /// The border of this item
    border: Rect<f32>,

    /// The default size of this item
    flex_basis: f32,
    /// The default size of this item, minus padding and border
    inner_flex_basis: f32,
    /// The amount by which this item has deviated from its target size
    violation: f32,
    /// Is the size of this item locked
    frozen: bool,

    /// Either the max- or min- content flex fraction
    /// See https://www.w3.org/TR/css-flexbox-1/#intrinsic-main-sizes
    content_flex_fraction: f32,

    /// The proposed inner size of this item
    hypothetical_inner_size: Size<f32>,
    /// Hypothetical cross size before preliminary `stretch` min/max limits.
    /// Wrapped flex lines re-resolve those limits from the final line size.
    unclamped_hypothetical_cross_size: f32,
    /// The proposed outer size of this item
    hypothetical_outer_size: Size<f32>,
    /// The size that this item wants to be
    target_size: Size<f32>,
    /// The size that this item wants to be, plus any padding and border
    outer_target_size: Size<f32>,

    /// First-baseline ascent used while sizing and aligning the flex line.
    alignment_baseline: f32,
    /// First baseline from final child layout, measured from the flex
    /// container's logical block-start edge.
    first_block_baseline: f32,
    /// Last baseline from final child layout, measured from the flex
    /// container's logical block-start edge.
    last_block_baseline: f32,

    /// A temporary value for the main offset
    ///
    /// Offset is the relative position from the item's natural flow position based on
    /// relative position values, alignment, and justification. Does not include margin/padding/border.
    offset_main: f32,
    /// A temporary value for the cross offset
    ///
    /// Offset is the relative position from the item's natural flow position based on
    /// relative position values, alignment, and justification. Does not include margin/padding/border.
    offset_cross: f32,
}

impl FlexItem {
    /// Returns true if the item is a <https://www.w3.org/TR/css-overflow-3/#scroll-container>
    fn is_scroll_container(&self) -> bool {
        self.overflow.x.is_scroll_container() | self.overflow.y.is_scroll_container()
    }

    /// Baseline selected by this item's first/last baseline preference,
    /// measured from the flex container's logical block-start edge.
    fn aligned_block_baseline(&self) -> f32 {
        if self.align_self.is_last_baseline() {
            self.last_block_baseline
        } else {
            self.first_block_baseline
        }
    }
}

/// A line of [`FlexItem`] used for intermediate computation
struct FlexLine<'a> {
    /// The slice of items to iterate over during computation of this line
    items: &'a mut [FlexItem],
    /// The dimensions of the cross-axis
    cross_size: f32,
    /// The relative offset of the cross-axis
    offset_cross: f32,
    /// Maximum ascent in the major baseline-sharing group.
    major_baseline: Option<f32>,
    /// Maximum ascent in the minor baseline-sharing group.
    minor_baseline: Option<f32>,
}

impl<'a> FlexLine<'a> {
    /// Create an unresolved line over `items`.
    fn new(items: &'a mut [FlexItem]) -> Self {
        Self { items, cross_size: 0.0, offset_cross: 0.0, major_baseline: None, minor_baseline: None }
    }

    /// Return the final shared baseline for one sharing group. All members of
    /// a resolved group expose the same container-relative baseline, so source
    /// order is immaterial here.
    fn shared_block_baseline(&self, group: BaselineGroup) -> Option<f32> {
        self.items
            .iter()
            .find(|item| item.align_self.is_baseline() && item.baseline_group == group)
            .map(FlexItem::aligned_block_baseline)
    }
}

/// Select the flex container's first and last baselines after final item
/// layout. This mirrors Blink's `BaselineAccumulator`: major then minor for
/// the first line, minor then major for the last line, followed by the normal
/// first/last item fallback.
fn flex_container_baselines(flex_lines: &[FlexLine<'_>], constants: &AlgoConstants) -> (Option<f32>, Option<f32>) {
    let first_line = if constants.wrap_reverse { flex_lines.last() } else { flex_lines.first() };
    let last_line = if constants.wrap_reverse { flex_lines.first() } else { flex_lines.last() };

    let first = first_line.and_then(|line| {
        if constants.main_axis_is_inline {
            line.shared_block_baseline(BaselineGroup::Major)
                .or_else(|| line.shared_block_baseline(BaselineGroup::Minor))
                .or_else(|| line.items.first().map(|item| item.first_block_baseline))
        } else {
            line.items.first().map(|item| item.first_block_baseline)
        }
    });
    let last = last_line.and_then(|line| {
        if constants.main_axis_is_inline {
            line.shared_block_baseline(BaselineGroup::Minor)
                .or_else(|| line.shared_block_baseline(BaselineGroup::Major))
                .or_else(|| line.items.last().map(|item| item.last_block_baseline))
        } else {
            line.items.last().map(|item| item.last_block_baseline)
        }
    });

    (first, last)
}

/// Values that can be cached during the flexbox algorithm
struct AlgoConstants {
    /// The direction of the current segment being laid out
    dir: FlexDirection,
    /// Whether the authored flex main direction is reversed.
    authored_main_reversed: bool,
    /// The CSS inline direction inherited by the container.
    inline_direction: Direction,
    /// The direction of the physical horizontal axis.
    horizontal_direction: Direction,
    /// Is the physical main axis horizontal?
    is_row: bool,
    /// Is the physical main axis vertical?
    is_column: bool,
    /// Does the main axis use the container's logical inline axis?
    main_axis_is_inline: bool,
    /// Does the main axis use the container's logical block axis?
    main_axis_is_block: bool,
    /// Whether logical cross-start is the physical high edge.
    cross_axis_start_reversed: bool,
    /// Whether flex cross-start is the physical high edge.
    cross_axis_flex_start_reversed: bool,
    /// Is wrapping enabled (in either direction)
    is_wrap: bool,
    /// Specialized intrinsic inline-size operation, when one was requested.
    intrinsic_inline_size: Option<FlexIntrinsicInlineSize>,
    /// Whether a block-axis main size comes from the intrinsic block size
    /// produced by flex layout. This is independent from an inline-size probe
    /// so a two-axis measurement can compute both quantities correctly.
    uses_layout_intrinsic_block_size: bool,
    /// Whether `flex-wrap` reverses the logical cross axis.
    wrap_reverse: bool,
    /// Whether the normalized physical cross axis is reversed. Horizontal
    /// reversal remains represented by `horizontal_direction`.
    cross_axis_reversed: bool,

    /// Writing mode that owns the container's logical axes.
    writing_mode: WritingMode,

    /// The item's min_size style
    min_size: Size<Option<f32>>,
    /// The item's max_size style
    max_size: Size<Option<f32>>,
    /// Shared CSS block-size resolver. Flex layout supplies the real intrinsic
    /// size after line sizing; the resolver owns aspect-ratio and automatic
    /// minimum semantics.
    content_based_block_size: ContentBasedBlockSize,
    /// Whether this pass must produce and apply a content-based block size.
    /// Content-size probes deliberately leave this disabled.
    resolve_content_based_block_size: bool,
    /// The margin of this section
    margin: Rect<f32>,
    /// The border of this section
    border: Rect<f32>,
    /// The space between the content box and the border box.
    /// This consists of padding + border + scrollbar_gutter.
    content_box_inset: Rect<f32>,
    /// The size reserved for scrollbar gutters in each axis
    scrollbar_gutter: Point<f32>,
    /// Authored logical column/row gaps. Keeping the unresolved values lets
    /// the inline-axis percentage basis become final without manufacturing a
    /// definite block-axis basis.
    raw_gap: Size<LengthPercentage>,
    /// Used physical gap of this section.
    gap: Size<f32>,
    /// The align_items property of this node
    align_items: AlignItems,
    /// The align_content property of this node
    align_content: AlignContent,
    /// The justify_content property of this node
    justify_content: Option<JustifyContent>,

    /// The border-box size of the node being laid out (if known)
    node_outer_size: Size<Option<f32>>,
    /// The content-box size of the node being laid out (if known)
    node_inner_size: Size<Option<f32>>,
    /// The content-box axes that are definite percentage-resolution bases.
    node_definite_inner_size: Size<Option<f32>>,
    /// Content-box size used to resolve child percentages.
    ///
    /// The final logical inline size participates in cyclic-percentage
    /// re-resolution, while the logical block size remains definite-only.
    node_percentage_size: Size<Option<f32>>,

    /// The size of the virtual container containing the flex items.
    container_size: Size<f32>,
    /// The size of the internal container
    inner_container_size: Size<f32>,
}

impl AlgoConstants {
    /// The flow-relative coordinate system established by this container.
    #[inline(always)]
    fn writing_direction(&self) -> WritingDirection {
        WritingDirection::new(self.writing_mode, self.inline_direction)
    }

    /// Project physical margins onto the authored flex cross axis.
    #[inline(always)]
    fn cross_axis_margins<T: Copy>(&self, margins: Rect<T>) -> Line<T> {
        let logical = self.writing_direction().to_logical_box_strut(margins);
        if self.main_axis_is_inline {
            Line { start: logical.block_start, end: logical.block_end }
        } else {
            Line { start: logical.inline_start, end: logical.inline_end }
        }
    }
}

/// Resolve flex gaps against the same percentage size passed to descendants.
///
/// CSS makes an auto inline size available after intrinsic sizing, so cyclic
/// inline-axis gaps can resolve for final layout. An auto block size remains
/// an indefinite percentage basis; its percentage gap therefore stays zero.
fn resolve_flex_gap(
    tree: &impl LayoutFlexboxContainer,
    raw_gap: &Size<LengthPercentage>,
    writing_mode: WritingMode,
    percentage_size: Size<Option<f32>>,
) -> Size<f32> {
    let logical_percentage_size = writing_mode.to_logical(percentage_size);
    writing_mode.to_physical(LogicalSize {
        inline_size: raw_gap
            .width
            .maybe_resolve(logical_percentage_size.inline_size, |val, basis| tree.calc(val, basis))
            .unwrap_or(0.0),
        block_size: raw_gap
            .height
            .maybe_resolve(logical_percentage_size.block_size, |val, basis| tree.calc(val, basis))
            .unwrap_or(0.0),
    })
}

/// Select flex-imposed definite axes from a set of known child dimensions.
///
/// The child still resolves definite axes supplied by its own authored sizing;
/// this carries only the extra definiteness established by flex layout.
fn flex_item_definite_dimensions(
    item: &FlexItem,
    known_dimensions: Size<Option<f32>>,
    constants: &AlgoConstants,
) -> Size<Option<f32>> {
    let mut definite_dimensions = Size::NONE;
    if item.main_size_is_definite {
        definite_dimensions.set_main(constants.dir, known_dimensions.main(constants.dir));
    }
    if item.cross_size_is_definite {
        definite_dimensions.set_cross(constants.dir, known_dimensions.cross(constants.dir));
    }
    definite_dimensions
}

/// Select definite preferred cross-axis geometry during intrinsic flex probes.
///
/// The main axis is intentionally omitted because these probes are computing
/// the flex basis or intrinsic main contribution. A directly resolved cross
/// size, including one transferred through a preferred ratio, remains usable
/// by descendants during that measurement.
fn flex_item_intrinsic_definite_dimensions(
    item: &FlexItem,
    known_dimensions: Size<Option<f32>>,
    constants: &AlgoConstants,
) -> Size<Option<f32>> {
    Size::NONE.with_cross(
        constants.dir,
        item.preferred_cross_size_is_definite.then(|| known_dimensions.cross(constants.dir)).flatten(),
    )
}

/// Project an intrinsic flex-item constraint into the child's main axis.
///
/// Min-/max-content constraints describe intrinsic inline sizing. When the
/// flex container's main axis maps to the child's block axis, Blink instead
/// measures the child with an indefinite initial block size and reads the
/// layout result's intrinsic block size. `MaxContent` is Taffy's unbounded
/// available-space representation for that block-axis layout measurement.
#[inline]
fn flex_item_content_main_constraint(item: &FlexItem, inline_constraint: AvailableSpace) -> AvailableSpace {
    if item.main_axis_is_inline {
        inline_constraint
    } else {
        AvailableSpace::MaxContent
    }
}

/// Resolve the space available to a flex item's cross axis.
///
/// A definite container cross size remains definite while the container's main
/// axis is being measured under a min/max-content constraint. This is what
/// allows stretch alignment and an aspect ratio to contribute to an inline-flex
/// container's intrinsic main size.
fn resolve_cross_axis_available_space(
    available_space: AvailableSpace,
    container_cross_size: Option<f32>,
    min_size: Option<f32>,
    max_size: Option<f32>,
) -> AvailableSpace {
    if let Some(container_cross_size) = container_cross_size {
        return AvailableSpace::Definite(container_cross_size.maybe_clamp(min_size, max_size));
    }

    match available_space {
        AvailableSpace::Definite(value) => AvailableSpace::Definite(value.maybe_clamp(min_size, max_size)),
        AvailableSpace::MinContent => min_size.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MinContent),
        AvailableSpace::MaxContent => max_size.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MaxContent),
    }
}

/// Computes the layout of a box according to the flexbox algorithm
pub fn compute_flexbox_layout(
    tree: &mut impl LayoutFlexboxContainer,
    node: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    let writing_mode = tree.get_writing_mode(node);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { run_mode, .. } = inputs;
    let resolved_aspect_ratio = tree.get_resolved_aspect_ratio(node);
    let size_containment = tree.get_size_containment(node);
    let style = tree.get_flexbox_container_style(node);

    // Pull these out earlier to avoid borrowing issues
    let aspect_ratio = if inputs.sizing_mode == SizingMode::InherentSize {
        resolved_aspect_ratio
    } else {
        resolved_aspect_ratio.disabled()
    };
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding_border_sum = padding.sum_axes() + border.sum_axes();
    let overflow = style.overflow();
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let scrollbar_gutter = overflow.transpose().map(|overflow| match overflow {
        Overflow::Scroll => style.scrollbar_width(),
        _ => 0.0,
    });
    let content_box_inset_size = padding_border_sum + Size { width: scrollbar_gutter.x, height: scrollbar_gutter.y };
    let contained_outer_size = size_containment.resolve_outer_size(Size::ZERO, content_box_inset_size);
    let contained_outer_block_size = writing_mode.to_logical(contained_outer_size).block_size;
    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };
    let raw_size = style.size();
    let raw_min_size = style.min_size();
    let raw_max_size = style.max_size();
    let raw_logical_size = writing_mode.to_logical(raw_size);
    let raw_logical_min_size = writing_mode.to_logical(raw_min_size);
    let raw_logical_max_size = writing_mode.to_logical(raw_max_size);
    let block_size_properties = BlockSizeProperties::new(
        raw_logical_size.block_size,
        raw_logical_min_size.block_size,
        raw_logical_max_size.block_size,
    );
    let content_based_block_size = ContentBasedBlockSize::new(
        block_size_properties,
        aspect_ratio,
        padding_border_sum,
        inputs.block_auto_behavior,
        writing_mode.to_logical(inputs.available_space).block_size,
        is_scroll_container,
        contained_outer_block_size,
    );
    let needs_content_based_block_resolution = inputs.sizing_mode == SizingMode::InherentSize
        && content_based_block_size.requires_resolution()
        && inputs.axis.contains(writing_mode.block_axis());
    drop(style);

    let mut node_sizing = resolve_node_size_constraints(
        tree,
        node,
        inputs,
        NodeSizeConstraintInput {
            raw_size,
            raw_min_size,
            raw_max_size,
            box_sizing_adjustment,
            padding_border_size: padding_border_sum,
            aspect_ratio,
            contained_outer_size,
        },
    );
    let block_axis_constraints = node_sizing.constraints.block_axis_constraints(writing_mode);
    let content_based_block_size = content_based_block_size.with_resolved_constraints(block_axis_constraints);
    content_based_block_size.apply_initial_block_geometry(
        writing_mode,
        writing_mode.to_logical(inputs.known_dimensions).block_size,
        &mut node_sizing,
    );
    let applied_aspect_ratio = run_mode == RunMode::ComputeSize && node_sizing.applied_aspect_ratio;
    let node_outer_size = node_sizing.outer_size;

    // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
    // is ComputeSize (and thus the container's size is all that we're interested in)
    if run_mode == RunMode::ComputeSize && !needs_content_based_block_resolution {
        if let Size { width: Some(width), height: Some(height) } = node_outer_size {
            return LayoutOutput::from_outer_size(Size { width, height })
                .with_block_constraint_dependency(node_sizing.depends_on_block_constraints)
                .with_applied_aspect_ratio(applied_aspect_ratio);
        }

        // We can also short-circuit if the width is known and only the width has been requested.
        if inputs.axis == RequestedAxis::Horizontal {
            if let Some(width) = node_outer_size.width {
                return LayoutOutput::from_outer_size(Size { width, height: 0.0 })
                    .with_block_constraint_dependency(node_sizing.depends_on_block_constraints)
                    .with_applied_aspect_ratio(applied_aspect_ratio);
            }
        }
    }

    compute_preliminary(tree, node, inputs, node_sizing, content_based_block_size)
        .with_block_constraint_dependency(node_sizing.depends_on_block_constraints)
        .with_applied_aspect_ratio(applied_aspect_ratio)
}

/// Compute a preliminary size for an item
fn compute_preliminary(
    tree: &mut impl LayoutFlexboxContainer,
    node: NodeId,
    inputs: LayoutInput,
    node_sizing: ResolvedNodeSizing,
    content_based_block_size: ContentBasedBlockSize,
) -> LayoutOutput {
    let writing_mode = tree.get_writing_mode(node);

    // Define some general constants we will need for the remainder of the algorithm.
    let mut constants = compute_constants(
        tree,
        tree.get_flexbox_container_style(node),
        inputs,
        node_sizing,
        writing_mode,
        content_based_block_size,
    );
    let LayoutInput { available_space, run_mode, .. } = inputs;
    let node_outer_size = node_sizing.outer_size;

    // 9. Flex Layout Algorithm

    // 9.1. Initial Setup

    // 9.2. Line Length Determination

    // 2. Determine the available main and cross space for the flex items
    debug_log!("determine_available_space");
    let available_space = determine_available_space(node_sizing.definite_size, available_space, &constants);

    // 1. Generate anonymous flex items as described in §4 Flex Items. Intrinsic
    // preferred/min/max widths need the resolved container space, so item
    // materialization follows that pure step even though the spec lists it
    // first.
    debug_log!("generate_anonymous_flex_items");
    let mut flex_items = generate_anonymous_flex_items(tree, node, &constants, available_space);

    // A single-line column's intrinsic inline contribution is independent of
    // flexible-length resolution in its block axis. Resolve it directly from
    // the item contributions, matching the intrinsic sizing entry point used
    // by block layout, floats, tables, and other shrink-to-fit callers.
    if let Some(FlexIntrinsicInlineSize::Column(constraint)) = constants.intrinsic_inline_size {
        let (intrinsic_cross_size, depends_on_block_constraints) = determine_single_line_column_intrinsic_cross_size(
            tree,
            &mut flex_items,
            constraint,
            available_space,
            &constants,
        );
        debug_log!("single_line_column_intrinsic_cross_size", intrinsic_cross_size);
        let mut outer_size = constants.node_outer_size.unwrap_or(Size::ZERO);
        outer_size.set_cross(constants.dir, intrinsic_cross_size);
        return LayoutOutput::from_outer_size(outer_size)
            .with_block_constraint_dependency(depends_on_block_constraints);
    }

    // 3. Determine the flex base size and hypothetical main size of each item.
    debug_log!("determine_flex_base_size");
    determine_flex_base_size(tree, &constants, available_space, &mut flex_items);

    #[cfg(feature = "debug")]
    for item in flex_items.iter() {
        debug_log!("item.flex_basis", item.flex_basis);
        debug_log!("item.inner_flex_basis", item.inner_flex_basis);
        debug_log!("item.hypothetical_outer_size", dbg:item.hypothetical_outer_size);
        debug_log!("item.hypothetical_inner_size", dbg:item.hypothetical_inner_size);
        debug_log!("item.resolved_minimum_main_size", dbg:item.resolved_minimum_main_size);
    }

    // 4. Determine the main size of the flex container
    // This has already been done as part of compute_constants. The inner size is exposed as constants.node_inner_size.

    // 9.3. Main Size Determination

    // 5. Collect flex items into flex lines.
    debug_log!("collect_flex_lines");
    let mut flex_lines = collect_flex_lines(&constants, available_space, &mut flex_items);

    // If the container size is undefined, determine its main size.
    debug_log!("determine_container_main_size");
    let intrinsic_block_size_is_main = constants.main_axis_is_block && constants.resolve_content_based_block_size;
    match (constants.node_inner_size.main(constants.dir), intrinsic_block_size_is_main) {
        (Some(inner_main_size), false) => {
            let outer_main_size = inner_main_size + constants.content_box_inset.main_axis_sum(constants.dir);
            constants.inner_container_size.set_main(constants.dir, inner_main_size);
            constants.container_size.set_main(constants.dir, outer_main_size);
        }
        _ => {
            // Sets constants.container_size and constants.outer_container_size
            determine_container_main_size(tree, available_space, &mut flex_lines, &mut constants);
            constants.node_inner_size.set_main(constants.dir, Some(constants.inner_container_size.main(constants.dir)));
            constants.node_outer_size.set_main(constants.dir, Some(constants.container_size.main(constants.dir)));

            debug_log!("constants.node_outer_size", dbg:constants.node_outer_size);
            debug_log!("constants.node_inner_size", dbg:constants.node_inner_size);
        }
    }
    if constants.main_axis_is_inline {
        constants
            .node_percentage_size
            .set_main(constants.dir, Some(constants.inner_container_size.main(constants.dir)));
        constants.gap =
            resolve_flex_gap(tree, &constants.raw_gap, constants.writing_mode, constants.node_percentage_size);

        // Cyclic percentage gaps contribute zero while determining an auto
        // inline size, but resolve against that final inline size for used
        // layout. The first collection above belongs to the intrinsic sizing
        // pass; wrapped layout must collect again with both the final main
        // size and final gap. This second collection does not feed back into
        // the container's intrinsic size.
        if constants.is_wrap {
            drop(flex_lines);
            let final_available_space = available_space
                .with_main(constants.dir, AvailableSpace::Definite(constants.inner_container_size.main(constants.dir)));
            flex_lines = collect_flex_lines(&constants, final_available_space, &mut flex_items);
        }
    }

    // 6. Resolve the flexible lengths of all the flex items to find their used main size.
    debug_log!("resolve_flexible_lengths");
    for line in &mut flex_lines {
        resolve_flexible_lengths(line, &constants);
    }

    // 9.4. Cross Size Determination

    // 7. Determine the hypothetical cross size of each item.
    debug_log!("determine_hypothetical_cross_size");
    for line in &mut flex_lines {
        determine_hypothetical_cross_size(tree, line, &constants, available_space);
    }

    // Calculate child baselines. This function is internally smart and only computes child baselines
    // if they are necessary.
    debug_log!("calculate_children_base_lines");
    calculate_children_base_lines(tree, node_outer_size, available_space, &mut flex_lines, &constants);

    // 8. Calculate the cross size of each flex line.
    debug_log!("calculate_cross_size");
    let intrinsic_line_cross_size = calculate_cross_size(&mut flex_lines, node_outer_size, &constants);

    // Resolve the container's block-axis intrinsic preferred/min/max values
    // from the unconstrained line sizes before a definite authored cross size
    // replaces the single-line size.
    debug_log!("determine_container_cross_size");
    determine_container_cross_size(&flex_lines, node_outer_size, intrinsic_line_cross_size, &mut constants);
    if !constants.main_axis_is_inline {
        constants
            .node_percentage_size
            .set_cross(constants.dir, Some(constants.inner_container_size.cross(constants.dir)));
        constants.gap =
            resolve_flex_gap(tree, &constants.raw_gap, constants.writing_mode, constants.node_percentage_size);
    }
    if !constants.is_wrap && node_outer_size.cross(constants.dir).is_some() {
        flex_lines[0].cross_size = constants.inner_container_size.cross(constants.dir);
    }

    // 9. Handle 'align-content: stretch'.
    debug_log!("handle_align_content_stretch");
    handle_align_content_stretch(&mut flex_lines, constants.container_size.map(Some), &constants);

    // 10. Collapse visibility:collapse items. If any flex items have visibility: collapse,
    //     note the cross size of the line they’re in as the item’s strut size, and restart
    //     layout from the beginning.
    //
    //     In this second layout round, when collecting items into lines, treat the collapsed
    //     items as having zero main size. For the rest of the algorithm following that step,
    //     ignore the collapsed items entirely (as if they were display:none) except that after
    //     calculating the cross size of the lines, if any line’s cross size is less than the
    //     largest strut size among all the collapsed items in the line, set its cross size to
    //     that strut size.
    //
    //     Skip this step in the second layout round.

    // TODO implement once (if ever) we support visibility:collapse

    // 11. Determine the used cross size of each flex item.
    debug_log!("determine_used_cross_size");
    determine_used_cross_size(tree, &mut flex_lines, &constants);

    // 9.5. Main-Axis Alignment

    // 12. Distribute any remaining free space.
    debug_log!("distribute_remaining_free_space");
    distribute_remaining_free_space(&mut flex_lines, &constants);

    // 9.6. Cross-Axis Alignment

    // 13. Resolve cross-axis auto margins (also includes 14).
    debug_log!("resolve_cross_axis_auto_margins");
    resolve_cross_axis_auto_margins(&mut flex_lines, &constants);

    // The final line sum is used by align-content after stretching and item
    // cross-size resolution. The container size itself was resolved above
    // from the unconstrained line contribution.
    let total_line_cross_size = flex_lines.iter().map(|line| line.cross_size).sum();

    // We have the container size.
    // If our caller does not care about performing layout we are done now.
    if run_mode == RunMode::ComputeSize {
        // A wrapped column flex container can change its intrinsic inline size
        // when its block constraint changes even if no individual item reports
        // a dependency (the number of columns itself may change).
        let depends_on_block_constraints = (constants.main_axis_is_block && constants.is_wrap)
            || flex_lines.iter().flat_map(|line| line.items.iter()).any(|item| item.depends_on_block_constraints);
        return LayoutOutput::from_outer_size(constants.container_size)
            .with_block_constraint_dependency(depends_on_block_constraints);
    }

    // 16. Align all flex lines per align-content.
    debug_log!("align_flex_lines_per_align_content");
    align_flex_lines_per_align_content(&mut flex_lines, &constants, total_line_cross_size);

    // Do a final layout pass and gather the resulting layouts
    debug_log!("final_layout_pass");
    let inflow_content_size = final_layout_pass(tree, &mut flex_lines, &constants);

    // Before returning we perform absolute layout on all absolutely positioned children
    debug_log!("perform_absolute_layout_on_absolute_children");
    let absolute_content_size = perform_absolute_layout_on_absolute_children(tree, node, &constants);

    debug_log!("hidden_layout");
    let len = tree.child_count(node);
    for order in 0..len {
        let child = tree.get_child_id(node, order);
        if tree.get_flexbox_child_style(child).box_generation_mode() == BoxGenerationMode::None {
            tree.set_unrounded_layout(child, &Layout::with_order(order as u32));
            tree.perform_child_layout(
                child,
                ChildLayoutInput::new(
                    Size::NONE,
                    Size::NONE,
                    writing_mode,
                    Size::MAX_CONTENT,
                    SizingMode::InherentSize,
                    Line::FALSE,
                ),
            );
        }
    }

    // 8.5. Flex Container Baselines: calculate distinct first and last
    // baselines from the final child fragments.
    // See https://www.w3.org/TR/css-flexbox-1/#flex-baselines
    let (first_block_baseline, last_block_baseline) = flex_container_baselines(&flex_lines, &constants);

    let writing_direction = constants.writing_direction();

    LayoutOutput::from_sizes_and_baseline_sets(
        constants.container_size,
        inflow_content_size.f32_max(absolute_content_size),
        physical_baseline(first_block_baseline, constants.container_size, writing_direction),
        physical_baseline(last_block_baseline, constants.container_size, writing_direction),
    )
}

/// Compute constants that can be reused during the flexbox algorithm.
#[inline]
fn compute_constants(
    tree: &impl LayoutFlexboxContainer,
    style: impl FlexboxContainerStyle,
    inputs: LayoutInput,
    node_sizing: ResolvedNodeSizing,
    writing_mode: WritingMode,
    content_based_block_size: ContentBasedBlockSize,
) -> AlgoConstants {
    let LayoutInput { sizing_mode, .. } = inputs;
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let authored_direction = style.flex_direction();
    let flex_wrap = style.flex_wrap();
    let inline_direction = style.direction();
    let flow = FlexFlow::resolve(authored_direction, flex_wrap, writing_mode, inline_direction);
    let dir = flow.direction;
    let is_row = dir.is_row();
    let is_column = dir.is_column();
    let main_axis_is_inline = flow.main_axis_is_inline;
    let main_axis_is_block = !main_axis_is_inline;
    let is_wrap = matches!(flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let wrap_reverse = flex_wrap == FlexWrap::WrapReverse;
    let cross_axis_reversed = flow.cross_axis_reversed;

    let margin = style.margin().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));

    let align_items = style.align_items().unwrap_or(AlignItems::NORMAL).resolve_normal(AlignItems::STRETCH);
    let align_content = style.align_content().unwrap_or(AlignContent::STRETCH);
    let justify_content = style.justify_content();
    let horizontal_direction = flow.horizontal_direction;

    // Scrollbar gutters are reserved when the `overflow` property is set to `Overflow::Scroll`.
    // However, the axis are switched (transposed) because a node that scrolls vertically needs
    // *horizontal* space to be reserved for a scrollbar
    let overflow = style.overflow();
    let scrollbar_gutter = overflow.transpose().map(|overflow| match overflow {
        Overflow::Scroll => style.scrollbar_width(),
        _ => 0.0,
    });
    let mut content_box_inset = padding + border;
    content_box_inset.bottom += scrollbar_gutter.y;

    match horizontal_direction {
        Direction::Ltr => content_box_inset.right += scrollbar_gutter.x,
        Direction::Rtl => content_box_inset.left += scrollbar_gutter.x,
    };

    let node_outer_size = node_sizing.outer_size;
    let node_inner_size = node_outer_size.maybe_sub(content_box_inset.sum_axes());
    let node_definite_inner_size = node_sizing.definite_size.maybe_sub(content_box_inset.sum_axes());
    let logical_inner_size_for_percentages = writing_mode.to_logical(node_inner_size);
    let logical_definite_inner_size = writing_mode.to_logical(node_definite_inner_size);
    let node_percentage_size = writing_mode.to_physical(LogicalSize {
        inline_size: logical_inner_size_for_percentages.inline_size,
        block_size: logical_definite_inner_size.block_size,
    });
    let raw_gap = style.gap();
    let gap = resolve_flex_gap(tree, &raw_gap, writing_mode, node_percentage_size);

    let container_size = Size::zero();
    let inner_container_size = Size::zero();
    let resolve_content_based_block_size = sizing_mode == SizingMode::InherentSize
        && inputs.axis.contains(writing_mode.block_axis())
        && content_based_block_size.requires_resolution();
    let inline_available_space = inputs.available_space.get_abs(writing_mode.inline_axis());
    let is_intrinsic_inline_probe = sizing_mode == SizingMode::ContentSize
        && inputs.sizing_purpose == SizingPurpose::IntrinsicContribution
        && inputs.axis.contains(writing_mode.inline_axis())
        && matches!(inline_available_space, AvailableSpace::MinContent | AvailableSpace::MaxContent);
    let intrinsic_inline_size = is_intrinsic_inline_probe.then_some({
        if main_axis_is_inline {
            FlexIntrinsicInlineSize::Row(inline_available_space)
        } else if is_wrap {
            FlexIntrinsicInlineSize::ColumnWrap(inline_available_space)
        } else {
            FlexIntrinsicInlineSize::Column(inline_available_space)
        }
    });
    let uses_layout_intrinsic_block_size = main_axis_is_block
        && (inputs.sizing_purpose == SizingPurpose::Layout || inputs.axis.contains(writing_mode.block_axis()));

    AlgoConstants {
        dir,
        authored_main_reversed: flow.authored_main_reversed,
        inline_direction,
        horizontal_direction,
        is_row,
        is_column,
        main_axis_is_inline,
        main_axis_is_block,
        cross_axis_start_reversed: flow.cross_axis_start_reversed,
        cross_axis_flex_start_reversed: flow.cross_axis_flex_start_reversed,
        is_wrap,
        intrinsic_inline_size,
        uses_layout_intrinsic_block_size,
        wrap_reverse,
        cross_axis_reversed,
        writing_mode,
        min_size: node_sizing.min_size,
        max_size: node_sizing.max_size,
        content_based_block_size,
        resolve_content_based_block_size,
        margin,
        border,
        raw_gap,
        gap,
        content_box_inset,
        scrollbar_gutter,
        align_items,
        align_content,
        justify_content,
        node_outer_size,
        node_inner_size,
        node_definite_inner_size,
        node_percentage_size,
        container_size,
        inner_container_size,
    }
}

/// Generate anonymous flex items.
///
/// # [9.1. Initial Setup](https://www.w3.org/TR/css-flexbox-1/#box-manip)
///
/// - [**Generate anonymous flex items**](https://www.w3.org/TR/css-flexbox-1/#algo-anon-box) as described in [§4 Flex Items](https://www.w3.org/TR/css-flexbox-1/#flex-items).
#[inline]
fn generate_anonymous_flex_items(
    tree: &mut impl LayoutFlexboxContainer,
    node: NodeId,
    constants: &AlgoConstants,
    available_space: Size<AvailableSpace>,
) -> Vec<FlexItem> {
    let child_ids: Vec<_> = tree.child_ids(node).collect();
    child_ids
        .into_iter()
        .enumerate()
        .filter_map(|(index, child)| {
            let aspect_ratio = tree.get_resolved_aspect_ratio(child);
            let child_writing_mode = tree.get_writing_mode(child);
            let main_axis_is_inline = child_writing_mode.inline_axis() == constants.dir.main_axis();
            let child_style = tree.get_flexbox_child_style(child);
            if child_style.position() == Position::Absolute
                || child_style.box_generation_mode() == BoxGenerationMode::None
            {
                return None;
            }
            // CSS box-model percentages use the containing block's logical
            // inline-size, including when that inline axis is vertical.
            let percentage_basis = constants.writing_mode.to_logical(constants.node_percentage_size).inline_size;
            let padding = child_style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
            let border = child_style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
            let pb_sum = (padding + border).sum_axes();
            let box_sizing = child_style.box_sizing();
            let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
            let raw_size = child_style.size();
            let raw_min_size = child_style.min_size();
            let raw_max_size = child_style.max_size();
            let raw_logical_size = child_writing_mode.to_logical(raw_size);
            let raw_logical_min_size = child_writing_mode.to_logical(raw_min_size);
            let raw_logical_max_size = child_writing_mode.to_logical(raw_max_size);
            let child_block_size_depends_on_parent =
                [raw_logical_size.block_size, raw_logical_min_size.block_size, raw_logical_max_size.block_size]
                    .into_iter()
                    .any(|value| value.may_have_percentage_dependence() || value.is_stretch());
            let mut depends_on_block_constraints = child_block_size_depends_on_parent && aspect_ratio.ratio.is_some();
            let flex_basis = child_style.flex_basis();
            let raw_margin = child_style.margin();
            let margin = raw_margin.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
            let margin_is_auto = raw_margin.map(LengthPercentageAuto::is_auto);
            let align_self = FlexboxItemStyle::align_self(&child_style)
                .unwrap_or(constants.align_items)
                .resolve_normal(AlignItems::STRETCH)
                .resolve_axis_relative(
                    child_writing_mode,
                    child_style.direction(),
                    constants.writing_mode,
                    constants.inline_direction,
                    constants.dir.cross_axis(),
                );
            let stretch_properties = StretchSizeProperties::new(raw_size, raw_min_size, raw_max_size);
            let stretch_available_size = constants.node_definite_inner_size.maybe_sub(margin.sum_axes());
            let stretch = stretch_properties.resolve(stretch_available_size, pb_sum);
            // Flex stores the untransferred preferred size in the authored
            // sizing box, while the shared stretch resolver returns the used
            // border box. Convert only for that intermediate representation;
            // all min/max and final sizes remain border-box values.
            let stretch_preferred_in_sizing_box =
                stretch.preferred.maybe_sub(box_sizing_adjustment).maybe_max(Size::ZERO);
            let definite_untransferred_size = raw_size
                .maybe_resolve(constants.node_definite_inner_size, |val, basis| tree.calc(val, basis))
                .or(stretch_preferred_in_sizing_box);
            let definite_preferred_size = definite_untransferred_size.maybe_add(box_sizing_adjustment);
            let mut untransferred_size = raw_size
                .maybe_resolve(constants.node_percentage_size, |val, basis| tree.calc(val, basis))
                .or(stretch_preferred_in_sizing_box);
            let preferred_size_is_indefinite = untransferred_size.map(|size| size.is_none());
            let unresolved_flex_basis = if flex_basis.is_auto() { raw_size.main(constants.dir) } else { flex_basis };
            let resolved_flex_basis = if flex_basis.is_auto() {
                definite_untransferred_size.main(constants.dir)
            } else {
                flex_basis.maybe_resolve(constants.node_definite_inner_size.main(constants.dir), |val, basis| {
                    tree.calc(val, basis)
                })
            }
            .maybe_add(box_sizing_adjustment.main(constants.dir));
            let used_flex_basis_is_definite = resolved_flex_basis.is_some();
            let mut used_flex_basis = resolved_flex_basis
                .map(UsedFlexBasis::Resolved)
                .unwrap_or_else(|| UsedFlexBasis::from_unresolved_dimension(unresolved_flex_basis));
            let mut size = if used_flex_basis.is_unresolved() {
                untransferred_size.with_main(constants.dir, None)
            } else {
                untransferred_size
            }
            .maybe_add(box_sizing_adjustment);
            // Flexbox gives a stretched item in a definite single-line cross
            // axis an automatic preferred outer cross size and treats it as
            // definite. Retain the corresponding border-box size here so the
            // flex basis and automatic minimum consume the same geometry.
            let automatic_preferred_cross_size = if !constants.is_wrap
                && align_self == AlignSelf::STRETCH
                && raw_size.cross(constants.dir).is_auto()
                && !margin_is_auto.cross_start(constants.dir)
                && !margin_is_auto.cross_end(constants.dir)
            {
                constants
                    .node_definite_inner_size
                    .cross(constants.dir)
                    .map(|cross_size| f32_max(cross_size - margin.cross_axis_sum(constants.dir), 0.0))
            } else {
                None
            };
            let mut min_size =
                resolve_minimum_size(raw_min_size, constants.node_percentage_size, |val, basis| tree.calc(val, basis))
                    .maybe_add(box_sizing_adjustment)
                    .or(stretch.min);
            let mut max_size = raw_max_size
                .maybe_resolve(constants.node_percentage_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment)
                .or(stretch.max);
            let size_is_auto = raw_size.map(|dimension| dimension.is_auto());
            let initial_preferred_size = if used_flex_basis.is_unresolved() {
                definite_preferred_size.with_main(constants.dir, None)
            } else {
                definite_preferred_size
            };
            let initial_preferred_size = initial_preferred_size.with_cross(
                constants.dir,
                initial_preferred_size.cross(constants.dir).or(automatic_preferred_cross_size),
            );
            let initial_constraints = resolve_size_constraints(SizeConstraintInput {
                size: initial_preferred_size,
                preferred_size_is_indefinite: initial_preferred_size.map(|size| size.is_none()),
                min_size,
                max_size,
                size_is_auto,
                writing_mode: child_writing_mode,
                inline_auto_behavior: AutoSizeBehavior::FitContent,
                block_auto_behavior: AutoSizeBehavior::FitContent,
                transferred_sizes_mode: TransferredSizesMode::Normal,
                aspect_ratio,
                padding_border: pb_sum,
            });
            let direct_size_with_transfer = initial_constraints.size;
            let inset = child_style.inset().zip_size(constants.node_percentage_size, |p, s| {
                p.maybe_resolve(s, |val, basis| tree.calc(val, basis))
            });
            let preferred_cross_size_is_definite =
                direct_size_with_transfer.cross(constants.dir).is_some() || automatic_preferred_cross_size.is_some();
            let baseline_writing_mode = determine_baseline_writing_mode(
                constants.writing_direction(),
                child_writing_mode,
                constants.main_axis_is_inline,
            );
            let baseline_group = determine_baseline_group(
                constants.writing_direction(),
                baseline_writing_mode,
                constants.main_axis_is_inline,
                align_self.is_last_baseline(),
                constants.wrap_reverse,
            );
            let overflow = child_style.overflow();
            let scrollbar_width = child_style.scrollbar_width();
            let flex_grow = child_style.flex_grow();
            let flex_shrink = child_style.flex_shrink();
            let is_replaced = child_style.is_compressible_replaced();
            drop(child_style);

            let child_available_space = Size {
                width: constants.node_inner_size.width.map(AvailableSpace::Definite).unwrap_or(available_space.width),
                height: constants
                    .node_inner_size
                    .height
                    .map(AvailableSpace::Definite)
                    .unwrap_or(available_space.height),
            };
            let available_width = child_available_space.width.maybe_sub(margin.horizontal_axis_sum());
            let intrinsic_inputs = LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode: SizingMode::InherentSize,
                sizing_purpose: SizingPurpose::IntrinsicContribution,
                axis: RequestedAxis::Horizontal,
                inline_auto_behavior: AutoSizeBehavior::FitContent,
                block_auto_behavior: AutoSizeBehavior::FitContent,
                known_dimensions: Size::NONE,
                definite_dimensions: Size::NONE,
                parent_size: constants.node_percentage_size,
                parent_writing_mode: constants.writing_mode,
                available_space: child_available_space,
                block_margins_are_collapsible: Line::FALSE,
            };
            let content_size_override = if is_replaced {
                IntrinsicAxisValue::default()
            } else {
                intrinsic_content_size_from_initial_geometry(
                    AbsoluteAxis::Horizontal,
                    initial_constraints.initial_geometry(),
                    aspect_ratio,
                    pb_sum,
                )
            };
            let intrinsic = resolve_intrinsic_axis_constraints(
                tree,
                child,
                intrinsic_inputs,
                IntrinsicAxisInput {
                    preferred: raw_size.width,
                    min: raw_min_size.width,
                    max: raw_max_size.width,
                    available_space: available_width,
                    axis: AbsoluteAxis::Horizontal,
                    content_size_override,
                },
            );
            if let Some(intrinsic_width) = intrinsic.preferred {
                untransferred_size.width = Some(intrinsic_width - box_sizing_adjustment.width);
                // `flex-basis:auto` defers to the preferred main size. Once an
                // intrinsic width has been measured, that indirection has a
                // resolved value and must not fall through to the max-content
                // measurement used by `flex-basis:content`.
                if flex_basis.is_auto() && constants.dir.main_axis() == AbsoluteAxis::Horizontal {
                    used_flex_basis = UsedFlexBasis::Resolved(intrinsic_width);
                }
                if !used_flex_basis.is_unresolved() || !constants.dir.is_row() {
                    size.width = size.width.or(Some(intrinsic_width));
                }
            }
            min_size.width = min_size.width.or(intrinsic.min);
            max_size.width = max_size.width.or(intrinsic.max);
            depends_on_block_constraints |= intrinsic.depends_on_block_constraints;
            let preferred_size = untransferred_size
                .maybe_add(box_sizing_adjustment)
                .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, pb_sum);
            let constraint_input = SizeConstraintInput {
                size,
                preferred_size_is_indefinite,
                min_size,
                max_size,
                size_is_auto,
                writing_mode: child_writing_mode,
                inline_auto_behavior: AutoSizeBehavior::FitContent,
                block_auto_behavior: AutoSizeBehavior::FitContent,
                transferred_sizes_mode: TransferredSizesMode::Normal,
                aspect_ratio,
                padding_border: pb_sum,
            };
            let mut constraints_with_transfer = resolve_size_constraints(constraint_input);
            let mut constraints_without_transfer = resolve_size_constraints(SizeConstraintInput {
                transferred_sizes_mode: TransferredSizesMode::Ignore,
                ..constraint_input
            });
            let block_axis = child_writing_mode.block_axis();
            let block_known_dimensions = match block_axis {
                AbsoluteAxis::Horizontal => Size { width: None, height: constraints_with_transfer.size.height },
                AbsoluteAxis::Vertical => Size { width: constraints_with_transfer.size.width, height: None },
            };
            let block_definite_dimensions = match block_axis {
                AbsoluteAxis::Horizontal => Size { width: None, height: definite_preferred_size.height },
                AbsoluteAxis::Vertical => Size { width: definite_preferred_size.width, height: None },
            };
            let block_intrinsic = resolve_intrinsic_axis_constraints(
                tree,
                child,
                LayoutInput {
                    axis: block_axis.into(),
                    known_dimensions: block_known_dimensions,
                    definite_dimensions: block_definite_dimensions,
                    ..intrinsic_inputs
                },
                IntrinsicAxisInput {
                    preferred: raw_logical_size.block_size,
                    min: raw_logical_min_size.block_size,
                    max: raw_logical_max_size.block_size,
                    available_space: child_available_space.get_abs(block_axis),
                    axis: block_axis,
                    content_size_override: if is_replaced {
                        IntrinsicAxisValue::default()
                    } else {
                        intrinsic_content_size_from_initial_geometry(
                            block_axis,
                            constraints_with_transfer.initial_geometry(),
                            aspect_ratio,
                            pb_sum,
                        )
                    },
                },
            );
            constraints_with_transfer.apply_late_intrinsic_axis(
                block_axis,
                block_intrinsic.preferred,
                block_intrinsic.preferred_aspect_ratio_applied,
                block_intrinsic.min,
                block_intrinsic.max,
            );
            constraints_without_transfer.apply_late_intrinsic_axis(
                block_axis,
                block_intrinsic.preferred,
                block_intrinsic.preferred_aspect_ratio_applied,
                block_intrinsic.min,
                block_intrinsic.max,
            );
            depends_on_block_constraints |= block_intrinsic.depends_on_block_constraints;
            for axis in [AbsoluteAxis::Horizontal, AbsoluteAxis::Vertical] {
                // Flexbox defines the automatic minimum in its main axis from
                // the content/transferred/specified suggestions below. The
                // shared CSS Sizing automatic minimum still owns the cross
                // axis, where Flexbox does not replace it.
                if axis == constants.dir.main_axis() {
                    continue;
                }
                let automatic_minimum = measure_aspect_ratio_automatic_minimum(
                    tree,
                    child,
                    LayoutInput { axis: axis.into(), ..intrinsic_inputs },
                    axis,
                    pb_sum,
                    constraints_with_transfer,
                );
                constraints_with_transfer.apply_automatic_minimum(axis, automatic_minimum.value);
                constraints_without_transfer.apply_automatic_minimum(axis, automatic_minimum.value);
                depends_on_block_constraints |= automatic_minimum.depends_on_block_constraints;
            }
            let content_based_block_size = ContentBasedBlockSize::new(
                BlockSizeProperties::new(
                    raw_logical_size.block_size,
                    raw_logical_min_size.block_size,
                    raw_logical_max_size.block_size,
                ),
                aspect_ratio,
                pb_sum,
                AutoSizeBehavior::FitContent,
                AvailableSpace::MaxContent,
                overflow.x.is_scroll_container() || overflow.y.is_scroll_container(),
                None,
            )
            .with_resolved_constraints(constraints_with_transfer.block_axis_constraints(child_writing_mode));
            let block_axis_constraints_without_transfer =
                constraints_without_transfer.block_axis_constraints(child_writing_mode);
            size = constraints_with_transfer.size;
            let preferred_size_aspect_ratio_applied = constraints_with_transfer.aspect_ratio_applied;
            let specified_size_suggestion = definite_preferred_size.main(constants.dir);
            let transferred_size_suggestion = Size::NONE
                .with_cross(
                    constants.dir,
                    definite_preferred_size.cross(constants.dir).or(automatic_preferred_cross_size),
                )
                .maybe_clamp(constraints_without_transfer.min_size, constraints_without_transfer.max_size)
                .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, pb_sum)
                .main(constants.dir);

            Some(FlexItem {
                node: child,
                order: index as u32,
                size,
                preferred_size_aspect_ratio_applied,
                preferred_size,
                automatic_minimum: FlexAutomaticMinimum {
                    is_replaced,
                    specified_size_suggestion,
                    transferred_size_suggestion,
                },
                untransferred_size,
                min_size: constraints_without_transfer.min_size,
                max_size: constraints_without_transfer.max_size,
                min_size_with_transfer: constraints_with_transfer.min_size,
                max_size_with_transfer: constraints_with_transfer.max_size,
                content_based_block_size,
                block_axis_constraints_without_transfer,
                aspect_ratio,
                main_axis_is_inline,
                box_sizing,
                stretch: stretch_properties,
                used_flex_basis,
                used_flex_basis_is_definite,
                preferred_cross_size_is_definite,
                main_size_is_definite: false,
                cross_size_is_definite: false,
                depends_on_block_constraints,

                inset,
                margin,
                margin_is_auto,
                padding,
                border,
                align_self,
                baseline_writing_mode,
                baseline_group,
                overflow,
                scrollbar_width,
                flex_grow,
                flex_shrink,
                flex_basis: 0.0,
                inner_flex_basis: 0.0,
                violation: 0.0,
                frozen: false,

                resolved_minimum_main_size: 0.0,
                hypothetical_inner_size: Size::zero(),
                unclamped_hypothetical_cross_size: 0.0,
                hypothetical_outer_size: Size::zero(),
                target_size: Size::zero(),
                outer_target_size: Size::zero(),
                content_flex_fraction: 0.0,

                alignment_baseline: 0.0,
                first_block_baseline: 0.0,
                last_block_baseline: 0.0,

                offset_main: 0.0,
                offset_cross: 0.0,
            })
        })
        .collect()
}

/// Determine the available main and cross space for the flex items.
///
/// # [9.2. Line Length Determination](https://www.w3.org/TR/css-flexbox-1/#line-sizing)
///
/// - [**Determine the available main and cross space for the flex items**](https://www.w3.org/TR/css-flexbox-1/#algo-available).
///
/// For each dimension, if that dimension of the flex container’s content box is a definite size, use that;
/// if that dimension of the flex container is being sized under a min or max-content constraint, the available space in that dimension is that constraint;
/// otherwise, subtract the flex container’s margin, border, and padding from the space available to the flex container in that dimension and use that value.
/// **This might result in an infinite value**.
#[inline]
#[must_use]
fn determine_available_space(
    node_definite_outer_size: Size<Option<f32>>,
    outer_available_space: Size<AvailableSpace>,
    constants: &AlgoConstants,
) -> Size<AvailableSpace> {
    let width = match node_definite_outer_size.width {
        Some(node_width) => AvailableSpace::Definite(node_width - constants.content_box_inset.horizontal_axis_sum()),
        None => outer_available_space
            .width
            .maybe_sub(constants.margin.horizontal_axis_sum())
            .maybe_sub(constants.content_box_inset.horizontal_axis_sum()),
    };

    let height = match node_definite_outer_size.height {
        Some(node_height) => AvailableSpace::Definite(node_height - constants.content_box_inset.vertical_axis_sum()),
        None => outer_available_space
            .height
            .maybe_sub(constants.margin.vertical_axis_sum())
            .maybe_sub(constants.content_box_inset.vertical_axis_sum()),
    };

    Size { width, height }
}

/// Convert one definite cross-axis constraint into the main axis through the
/// item's preferred aspect ratio. Flexbox's automatic-minimum suggestions are
/// all border-box sizes, so the conversion stays at that edge.
#[inline]
fn transfer_flex_cross_size_to_main(
    cross_size: Option<f32>,
    dir: FlexDirection,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Option<f32> {
    Size::from_cross(dir, cross_size)
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
        .main(dir)
}

/// Resolve the content-based automatic minimum of a flex item from the three
/// sizing suggestions defined by Flexbox section 4.5.
///
/// Replaced items choose the smaller content/transferred suggestion;
/// non-replaced items choose the larger. Both are then capped by a definite
/// preferred main size and a definite maximum main size.
#[inline]
fn resolve_flex_content_based_automatic_minimum(
    item: &FlexItem,
    min_content_main_size: f32,
    dir: FlexDirection,
    padding_border: Size<f32>,
) -> f32 {
    let content_minimum =
        transfer_flex_cross_size_to_main(item.min_size.cross(dir), dir, item.aspect_ratio, padding_border);
    let content_maximum =
        transfer_flex_cross_size_to_main(item.max_size.cross(dir), dir, item.aspect_ratio, padding_border);
    let ratio_aware_content_size = min_content_main_size.maybe_clamp(content_minimum, content_maximum);
    // Blink's two intrinsic SizeType probes differ only in preferred-ratio
    // participation. For a non-replaced inline-axis main size, retain the
    // ratio-independent intrinsic contribution as the larger candidate. A
    // block-axis main size instead constrains its intrinsic block size by the
    // transferred inline min/max pair before it becomes the content-size
    // suggestion.
    let content_size_suggestion =
        if !item.automatic_minimum.is_replaced && item.main_axis_is_inline && item.preferred_cross_size_is_definite {
            ratio_aware_content_size.max(min_content_main_size)
        } else {
            ratio_aware_content_size
        };
    item.automatic_minimum.resolve(content_size_suggestion, item.max_size.main(dir), padding_border.main(dir))
}

/// Determine the flex base size and hypothetical main size of each item.
///
/// # [9.2. Line Length Determination](https://www.w3.org/TR/css-flexbox-1/#line-sizing)
///
/// - [**Determine the flex base size and hypothetical main size of each item:**](https://www.w3.org/TR/css-flexbox-1/#algo-main-item)
///
///     - A. If the item has a definite used flex basis, that’s the flex base size.
///
///     - B. If the flex item has ...
///
///         - an intrinsic aspect ratio,
///         - a used flex basis of content, and
///         - a definite cross size,
///
///       then the flex base size is calculated from its inner cross size and the flex item’s intrinsic aspect ratio.
///
///     - C. If the used flex basis is content or depends on its available space, and the flex container is being sized under a min-content
///       or max-content constraint (e.g. when performing automatic table layout \[CSS21\]), size the item under that constraint.
///       The flex base size is the item’s resulting main size.
///
///     - E. Otherwise, size the item into the available space using its used flex basis in place of its main size, treating a value of content as max-content.
///       If a cross size is needed to determine the main size (e.g. when the flex item’s main size is in its block axis) and the flex item’s cross size is auto and not definite,
///       in this calculation use fit-content as the flex item’s cross size. The flex base size is the item’s resulting main size.
///
///   When determining the flex base size, the item’s min and max main sizes are ignored (no clamping occurs).
///   Furthermore, the sizing calculations that floor the content box size at zero when applying box-sizing are also ignored.
///   (For example, an item with a specified size of zero, positive padding, and box-sizing: border-box will have an outer flex base size of zero—and hence a negative inner flex base size.)
#[inline]
fn determine_flex_base_size(
    tree: &mut impl LayoutFlexboxContainer,
    constants: &AlgoConstants,
    available_space: Size<AvailableSpace>,
    flex_items: &mut [FlexItem],
) {
    let dir = constants.dir;

    for child in flex_items.iter_mut() {
        let used_flex_basis = child.used_flex_basis;
        let flex_basis_is_unresolved = used_flex_basis.is_unresolved();
        let aspect_ratio = child.aspect_ratio;
        let padding_border = (child.padding + child.border).sum_axes();
        let box_sizing_adjustment = if child.box_sizing == BoxSizing::ContentBox { padding_border } else { Size::ZERO };

        // Parent size for child sizing
        let cross_axis_percentage_size = constants.node_percentage_size.cross(dir);
        let cross_axis_definite_size = constants.node_definite_inner_size.cross(dir);
        let child_parent_size = Size::from_cross(dir, cross_axis_percentage_size);

        // Available space for child sizing
        // Min/max sizes transferred through the aspect ratio are taken into account here
        // https://github.com/w3c/csswg-drafts/issues/10997
        let cross_axis_margin_sum = constants.margin.cross_axis_sum(dir);
        let min_size_with_transfer = child.min_size_with_transfer;
        let max_size_with_transfer = child.max_size_with_transfer;
        let child_min_cross = min_size_with_transfer.cross(dir).maybe_add(cross_axis_margin_sum);
        let child_max_cross = max_size_with_transfer.cross(dir).maybe_add(cross_axis_margin_sum);

        // Clamp available space by min- and max- size
        let cross_axis_available_space = resolve_cross_axis_available_space(
            available_space.cross(dir),
            cross_axis_definite_size,
            child_min_cross,
            child_max_cross,
        );

        // Known dimensions for child sizing
        let child_known_dimensions = {
            let mut ckd = if flex_basis_is_unresolved {
                child.untransferred_size.with_main(dir, None).maybe_add(box_sizing_adjustment)
            } else {
                child.size.with_main(dir, None)
            };
            // Clamp the definite cross size by the cross min/max sizes so that sizes
            // transferred through an intrinsic aspect ratio (e.g. for replaced elements)
            // are based on the used cross size.
            ckd.set_cross(
                dir,
                ckd.cross(dir).maybe_clamp(min_size_with_transfer.cross(dir), max_size_with_transfer.cross(dir)),
            );
            if child.align_self == AlignSelf::STRETCH
                && !child.margin_is_auto.cross_start(constants.dir)
                && !child.margin_is_auto.cross_end(constants.dir)
                && ckd.cross(dir).is_none()
            {
                ckd.set_cross(
                    dir,
                    cross_axis_available_space.into_option().maybe_sub(child.margin.cross_axis_sum(dir)),
                );
            }
            ckd
        };

        let content_ratio_size =
            if matches!(used_flex_basis, UsedFlexBasis::Content) && child.preferred_cross_size_is_definite {
                child_known_dimensions
                    .maybe_sub(box_sizing_adjustment)
                    .maybe_max(Size::ZERO)
                    .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, child.box_sizing, padding_border)
                    .maybe_add(box_sizing_adjustment)
                    .main(dir)
            } else {
                None
            };

        // Blink's flex BlockSizeFunc(SizeType::kContent) first establishes the
        // item's fit-content inline size, then transfers that size through the
        // preferred ratio to obtain a block-axis flex basis. This is distinct
        // from part B above: the inline size is content-derived rather than an
        // independently definite preferred cross size.
        let fit_content_ratio_size = if used_flex_basis.is_unresolved()
            && content_ratio_size.is_none()
            && !child.main_axis_is_inline
            && !child.automatic_minimum.is_replaced
            && aspect_ratio.ratio.is_some()
        {
            // Main-axis min/max constraints are ignored while resolving the
            // flex base size, so do not let constraints transferred out of the
            // main axis constrain this fit-content inline probe. Direct cross
            // constraints still participate in the initial inline geometry.
            let cross_margin_sum = child.margin.cross_axis_sum(dir);
            let direct_cross_available_space = resolve_cross_axis_available_space(
                available_space.cross(dir),
                cross_axis_definite_size,
                child.min_size.cross(dir).maybe_add(cross_margin_sum),
                child.max_size.cross(dir).maybe_add(cross_margin_sum),
            )
            .maybe_sub(cross_margin_sum);
            let fit_content_known_dimensions = child_known_dimensions.with_cross(dir, None);
            let measurement_inputs = ChildLayoutInput::new(
                fit_content_known_dimensions,
                child_parent_size,
                constants.writing_mode,
                Size::MAX_CONTENT.with_cross(dir, direct_cross_available_space),
                SizingMode::ContentSize,
                Line::FALSE,
            )
            .with_definite_dimensions(flex_item_intrinsic_definite_dimensions(
                child,
                fit_content_known_dimensions,
                constants,
            ));
            let fit_content_inline = fit_content_inline_size_with_metadata(
                tree,
                child.node,
                measurement_inputs,
                direct_cross_available_space.compute_free_space(0.0),
                dir.cross_axis(),
            );
            child.depends_on_block_constraints |= fit_content_inline.depends_on_block_constraints;
            Size::from_cross(
                dir,
                fit_content_inline.value.maybe_clamp(child.min_size.cross(dir), child.max_size.cross(dir)),
            )
            .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
            .main(dir)
        } else {
            None
        };
        child.main_size_is_definite = constants.node_definite_inner_size.main(dir).is_some()
            || child.used_flex_basis_is_definite
            || content_ratio_size.is_some()
            || fit_content_ratio_size.is_some();

        child.flex_basis = 'flex_basis: {
            // A. If the item has a definite used flex basis, that’s the flex base size.

            // B. If the flex item has an intrinsic aspect ratio,
            //    a used flex basis of content, and a definite cross size,
            //    then the flex base size is calculated from its inner
            //    cross size and the flex item’s intrinsic aspect ratio.

            // A `content` basis ignores a preferred main size. It can still use
            // a main size transferred from an independently definite cross
            // size (including an align-self stretch size) through aspect-ratio.
            match used_flex_basis {
                UsedFlexBasis::Resolved(flex_basis) => break 'flex_basis flex_basis,
                UsedFlexBasis::Content => {
                    if let Some(content_ratio_size) = content_ratio_size {
                        break 'flex_basis content_ratio_size;
                    }
                }
                UsedFlexBasis::Intrinsic(_) | UsedFlexBasis::Stretch => {}
            }

            // `content`, intrinsic sizing keywords, and an unresolved
            // `stretch` all invoke Blink's content block-size callback. Once
            // fit-content inline geometry supplied a ratio-derived block size,
            // that callback is complete and descendant block layout is not
            // consulted for the basis.
            if let Some(fit_content_ratio_size) = fit_content_ratio_size {
                break 'flex_basis fit_content_ratio_size;
            }

            // C. If the used flex basis is content or depends on its available space,
            //    and the flex container is being sized under a min-content or max-content
            //    constraint (e.g. when performing automatic table layout [CSS21]),
            //    size the item under that constraint. The flex base size is the item’s
            //    resulting main size.

            // This is covered by the implementation of E below, which passes the available_space constraint
            // through to the child size computation. It may need a separate implementation if/when D is implemented.

            // D. Otherwise, if the used flex basis is content or depends on its
            //    available space, the available main size is infinite, and the flex item’s
            //    inline axis is parallel to the main axis, lay the item out using the rules
            //    for a box in an orthogonal flow [CSS3-WRITING-MODES]. The flex base size
            //    is the item’s max-content main size.

            // TODO: implement this orthogonal-flow branch by deriving the
            // child's fit-content cross constraint from its ConstraintSpace.

            // Intrinsic flex-basis keywords are sizing functions, not aliases
            // for `content`. Resolve them through the shared intrinsic-axis
            // protocol so min-content, max-content and fit-content retain
            // their distinct constraints in parallel and orthogonal flows.
            if let UsedFlexBasis::Intrinsic(value) = used_flex_basis {
                let basis_available_space = available_space.main(dir).maybe_sub(child.margin.main_axis_sum(dir));
                let measurement_inputs = ChildLayoutInput::new(
                    child_known_dimensions,
                    child_parent_size,
                    constants.writing_mode,
                    Size::MAX_CONTENT.with_cross(dir, cross_axis_available_space),
                    SizingMode::ContentSize,
                    Line::FALSE,
                )
                .with_definite_dimensions(flex_item_intrinsic_definite_dimensions(
                    child,
                    child_known_dimensions,
                    constants,
                ))
                .into_measurement(dir.main_axis().into());
                let resolved = resolve_intrinsic_preferred_axis_size(
                    tree,
                    child.node,
                    measurement_inputs,
                    value,
                    basis_available_space,
                    dir.main_axis(),
                    constants.node_percentage_size.main(dir),
                );
                child.depends_on_block_constraints |= resolved.depends_on_block_constraints;
                if let Some(size) = resolved.value {
                    break 'flex_basis size;
                }
            }

            // `stretch` fills definite available space with the margin box.
            // Under an intrinsic constraint it falls through to the content
            // sizing rule below, matching the sizing keyword's auto fallback.
            if used_flex_basis == UsedFlexBasis::Stretch {
                let stretched_basis =
                    available_space.main(dir).maybe_sub(child.margin.main_axis_sum(dir)).into_option();
                if let Some(size) = stretched_basis {
                    child.used_flex_basis_is_definite = true;
                    child.main_size_is_definite = true;
                    break 'flex_basis f32_max(0.0, size);
                }
            }

            // E. Otherwise, size the item into the available space using its used flex basis
            //    in place of its main size, treating a value of content as max-content.
            //    If a cross size is needed to determine the main size (e.g. when the
            //    flex item’s main size is in its block axis) and the flex item’s cross size
            //    is auto and not definite, in this calculation use fit-content as the
            //    flex item’s cross size. The flex base size is the item’s resulting main size.

            let inline_constraint = match constants.intrinsic_inline_size {
                // Row intrinsic sizing constructs each content-based flex base
                // in an indefinite flex-basis space. Its content fallback is
                // therefore max-content even while the container contribution
                // itself is being queried under a min-content constraint.
                Some(FlexIntrinsicInlineSize::Row(_)) => AvailableSpace::MaxContent,
                _ if available_space.main(dir) == AvailableSpace::MinContent => AvailableSpace::MinContent,
                _ => AvailableSpace::MaxContent,
            };
            let content_constraint = flex_item_content_main_constraint(child, inline_constraint);
            let child_available_space =
                Size::MAX_CONTENT.with_main(dir, content_constraint).with_cross(dir, cross_axis_available_space);

            debug_log!("COMPUTE CHILD BASE SIZE:");
            let measured = tree.measure_child_size_with_metadata(
                child.node,
                ChildLayoutInput::new(
                    child_known_dimensions,
                    child_parent_size,
                    constants.writing_mode,
                    child_available_space,
                    SizingMode::ContentSize,
                    Line::FALSE,
                )
                .with_definite_dimensions(flex_item_intrinsic_definite_dimensions(
                    child,
                    child_known_dimensions,
                    constants,
                )),
                dir.main_axis().into(),
            );
            child.depends_on_block_constraints |= measured.depends_on_block_constraints;
            break 'flex_basis measured.size.get_abs(dir.main_axis());
        };

        // Floor flex-basis by the padding_border_sum (floors inner_flex_basis at zero)
        // This seems to be in violation of the spec which explicitly states that the content box should not be floored at zero
        // (like it usually is) when calculating the flex-basis. But including this matches both Chrome and Firefox's behaviour.
        //
        // TODO: resolve spec violation
        // Spec: https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions
        // Spec: https://www.w3.org/TR/css-flexbox-1/#change-2016-max-contribution
        let padding_border_sum = child.padding.main_axis_sum(constants.dir) + child.border.main_axis_sum(constants.dir);
        child.flex_basis = child.flex_basis.max(padding_border_sum);

        // The hypothetical main size is the item’s flex base size clamped according to its
        // used min and max main sizes (and flooring the content box size at zero).

        child.inner_flex_basis =
            child.flex_basis - child.padding.main_axis_sum(constants.dir) - child.border.main_axis_sum(constants.dir);

        let padding_border_axes_sums = (child.padding + child.border).sum_axes().map(Some);

        // Note that it is important that the `parent_size` parameter in the main axis is not set for this
        // function call as it used for resolving percentages, and percentage size in an axis should not contribute
        // to a min-content contribution in that same axis. However the `parent_size` and `available_space` *should*
        // be set to their usual values in the cross axis so that wrapping content can wrap correctly.
        //
        // See https://drafts.csswg.org/css-sizing-3/#min-percentage-contribution
        let style_min_main_size =
            child.min_size.or(child.overflow.map(Overflow::maybe_into_automatic_min_size).into()).main(dir);

        child.resolved_minimum_main_size = style_min_main_size.unwrap_or({
            let min_content_main_size = {
                let child_available_space = Size::MIN_CONTENT
                    .with_main(dir, flex_item_content_main_constraint(child, AvailableSpace::MinContent))
                    .with_cross(dir, cross_axis_available_space);

                debug_log!("COMPUTE CHILD MIN SIZE:");
                let measured = tree.measure_child_size_with_metadata(
                    child.node,
                    ChildLayoutInput::new(
                        child_known_dimensions,
                        child_parent_size,
                        constants.writing_mode,
                        child_available_space,
                        SizingMode::ContentSize,
                        Line::FALSE,
                    )
                    .with_definite_dimensions(flex_item_intrinsic_definite_dimensions(
                        child,
                        child_known_dimensions,
                        constants,
                    )),
                    dir.main_axis().into(),
                );
                child.depends_on_block_constraints |= measured.depends_on_block_constraints;
                measured.size.get_abs(dir.main_axis())
            };

            // 4.5. Automatic Minimum Size of Flex Items
            // https://www.w3.org/TR/css-flexbox-1/#min-size-auto
            resolve_flex_content_based_automatic_minimum(child, min_content_main_size, dir, padding_border)
        });

        // Sizes transferred through the aspect ratio clamp the hypothetical main size,
        // but do not participate in resolving flexible lengths or clamping the final size.
        // https://github.com/w3c/csswg-drafts/issues/10997
        let hypothetical_inner_min_main = child
            .resolved_minimum_main_size
            .maybe_max(min_size_with_transfer.main(constants.dir))
            .maybe_max(padding_border_axes_sums.main(constants.dir));
        let hypothetical_inner_size =
            child.flex_basis.maybe_clamp(Some(hypothetical_inner_min_main), max_size_with_transfer.main(constants.dir));
        let hypothetical_outer_size = hypothetical_inner_size + child.margin.main_axis_sum(constants.dir);

        child.hypothetical_inner_size.set_main(constants.dir, hypothetical_inner_size);
        child.hypothetical_outer_size.set_main(constants.dir, hypothetical_outer_size);
    }
}

/// Collect flex items into flex lines.
///
/// # [9.3. Main Size Determination](https://www.w3.org/TR/css-flexbox-1/#main-sizing)
///
/// - [**Collect flex items into flex lines**](https://www.w3.org/TR/css-flexbox-1/#algo-line-break):
///
///     - If the flex container is single-line, collect all the flex items into a single flex line.
///
///     - Otherwise, starting from the first uncollected item, collect consecutive items one by one until the first time that the next collected item would not fit into the flex container’s inner main size
///       (or until a forced break is encountered, see [§10 Fragmenting Flex Layout](https://www.w3.org/TR/css-flexbox-1/#pagination)).
///       If the very first uncollected item wouldn't fit, collect just it into the line.
///
///       For this step, the size of a flex item is its outer hypothetical main size. (**Note: This can be negative**.)
///
///       Repeat until all flex items have been collected into flex lines.
///
///       **Note that the "collect as many" line will collect zero-sized flex items onto the end of the previous line even if the last non-zero item exactly "filled up" the line**.
#[inline]
fn collect_flex_lines<'a>(
    constants: &AlgoConstants,
    available_space: Size<AvailableSpace>,
    flex_items: &'a mut Vec<FlexItem>,
) -> Vec<FlexLine<'a>> {
    if !constants.is_wrap {
        let mut lines = new_vec_with_capacity(1);
        lines.push(FlexLine::new(flex_items.as_mut_slice()));
        lines
    } else {
        let main_axis_available_space = match constants.max_size.main(constants.dir) {
            Some(max_size) => AvailableSpace::Definite(
                available_space
                    .main(constants.dir)
                    .into_option()
                    .unwrap_or(max_size)
                    .maybe_max(constants.min_size.main(constants.dir)),
            ),
            None => available_space.main(constants.dir),
        };

        match main_axis_available_space {
            // If we're sizing under a max-content constraint then the flex items will never wrap
            // (at least for now - future extensions to the CSS spec may add provisions for forced wrap points)
            AvailableSpace::MaxContent => {
                let mut lines = new_vec_with_capacity(1);
                lines.push(FlexLine::new(flex_items.as_mut_slice()));
                lines
            }
            // If flex-wrap is Wrap and we're sizing under a min-content constraint, then we take every possible wrapping opportunity
            // and place each item in it's own line
            AvailableSpace::MinContent => {
                let mut lines = new_vec_with_capacity(flex_items.len());
                let mut items = &mut flex_items[..];
                while !items.is_empty() {
                    let (line_items, rest) = items.split_at_mut(1);
                    lines.push(FlexLine::new(line_items));
                    items = rest;
                }
                lines
            }
            AvailableSpace::Definite(main_axis_available_space) => {
                let mut lines = new_vec_with_capacity(1);
                let mut flex_items = &mut flex_items[..];
                let main_axis_gap = constants.gap.main(constants.dir);

                while !flex_items.is_empty() {
                    // Find index of the first item in the next line
                    // (or the last item if all remaining items are in the current line)
                    let mut line_length = 0.0;
                    let index = flex_items
                        .iter()
                        .enumerate()
                        .find(|&(idx, child)| {
                            // Gaps only occur between items (not before the first one or after the last one)
                            // So first item in the line does not contribute a gap to the line length
                            let gap_contribution = if idx == 0 { 0.0 } else { main_axis_gap };
                            line_length += child.hypothetical_outer_size.main(constants.dir) + gap_contribution;
                            line_length > main_axis_available_space && idx != 0
                        })
                        .map(|(idx, _)| idx)
                        .unwrap_or(flex_items.len());

                    let (items, rest) = flex_items.split_at_mut(index);
                    lines.push(FlexLine::new(items));
                    flex_items = rest;
                }
                lines
            }
        }
    }
}

/// Compute the intrinsic inline contribution of a single-line column.
///
/// Flexing changes an item's used block size, but intrinsic inline sizing is a
/// contribution query, not used layout. The shared intrinsic-contribution
/// operation applies the item's preferred/min/max sources at the child sizing
/// boundary, so a later flexed block size cannot feed back through
/// `aspect-ratio`.
fn determine_single_line_column_intrinsic_cross_size(
    tree: &mut impl LayoutFlexboxContainer,
    items: &mut [FlexItem],
    constraint: AvailableSpace,
    available_space: Size<AvailableSpace>,
    constants: &AlgoConstants,
) -> (f32, bool) {
    debug_assert!(constants.main_axis_is_block);
    debug_assert!(!constants.is_wrap);

    let dir = constants.dir;
    let child_available_space = Size::MAX_CONTENT.with_main(dir, available_space.main(dir)).with_cross(dir, constraint);
    let mut largest_contribution: f32 = 0.0;
    let mut depends_on_block_constraints = false;

    for item in items {
        let measured = measure_child_intrinsic_contribution(
            tree,
            item.node,
            ChildLayoutInput::new(
                Size::NONE,
                constants.node_percentage_size,
                constants.writing_mode,
                child_available_space,
                SizingMode::InherentSize,
                Line::FALSE,
            ),
            dir.cross_axis(),
        );
        item.depends_on_block_constraints |= measured.depends_on_block_constraints;

        let contribution = measured.size.cross(dir) + item.margin.cross_axis_sum(dir);
        largest_contribution = largest_contribution.max(contribution);
        depends_on_block_constraints |= item.depends_on_block_constraints;
    }

    (largest_contribution + constants.content_box_inset.cross_axis_sum(dir), depends_on_block_constraints)
}

/// Measure one flex item's content size in the container's main axis without
/// letting the item's authored preferred main size replace that content.
fn measure_flex_item_content_main_size(
    tree: &mut impl LayoutFlexboxContainer,
    item: &mut FlexItem,
    constraint: AvailableSpace,
    available_space: Size<AvailableSpace>,
    constants: &AlgoConstants,
) -> f32 {
    let dir = constants.dir;
    let cross_parent_size = constants.node_definite_inner_size.cross(dir);
    let cross_margin_sum = item.margin.cross_axis_sum(dir);
    let cross_available_space = resolve_cross_axis_available_space(
        available_space.cross(dir),
        cross_parent_size,
        item.min_size_with_transfer.cross(dir).maybe_add(cross_margin_sum),
        item.max_size_with_transfer.cross(dir).maybe_add(cross_margin_sum),
    );
    let mut known_dimensions = item.size.with_main(dir, None);
    known_dimensions.set_cross(
        dir,
        known_dimensions
            .cross(dir)
            .maybe_clamp(item.min_size_with_transfer.cross(dir), item.max_size_with_transfer.cross(dir)),
    );
    if item.align_self == AlignSelf::STRETCH
        && !item.margin_is_auto.cross_start(dir)
        && !item.margin_is_auto.cross_end(dir)
        && known_dimensions.cross(dir).is_none()
    {
        known_dimensions.set_cross(dir, cross_available_space.into_option().maybe_sub(item.margin.cross_axis_sum(dir)));
    }

    let padding_border = (item.padding + item.border).sum_axes();
    if let Some(ratio_main_size) = known_dimensions
        .maybe_apply_aspect_ratio_with_box_sizing(item.aspect_ratio, BoxSizing::BorderBox, padding_border)
        .main(dir)
    {
        return ratio_main_size;
    }

    let measured = tree.measure_child_size_with_metadata(
        item.node,
        ChildLayoutInput::new(
            known_dimensions,
            constants.node_percentage_size,
            constants.writing_mode,
            Size::MAX_CONTENT.with_main(dir, constraint).with_cross(dir, cross_available_space),
            SizingMode::ContentSize,
            Line::FALSE,
        )
        .with_definite_dimensions(flex_item_intrinsic_definite_dimensions(item, known_dimensions, constants)),
        dir.main_axis().into(),
    );
    item.depends_on_block_constraints |= measured.depends_on_block_constraints;
    measured.size.get_abs(dir.main_axis())
}

/// Resolve a flex item's intrinsic main-size contribution.
///
/// The content size and non-automatic preferred size establish the initial
/// contribution. Flexibility then caps/floors it by the flex base size before
/// the used min/max constraints are applied. All intermediate values are
/// border-box sizes; the outer margin is added exactly once at the end.
fn flex_item_intrinsic_main_contribution(
    tree: &mut impl LayoutFlexboxContainer,
    item: &mut FlexItem,
    constraint: AvailableSpace,
    available_space: Size<AvailableSpace>,
    constants: &AlgoConstants,
) -> f32 {
    let dir = constants.dir;
    let content_size = measure_flex_item_content_main_size(tree, item, constraint, available_space, constants);
    let preferred_size_is_auto = tree.get_flexbox_child_style(item.node).size().main(dir).is_auto();
    let preferred_size = if preferred_size_is_auto { None } else { item.preferred_size.main(dir) };
    let padding_border = (item.padding + item.border).main_axis_sum(dir);
    let mut contribution = content_size.maybe_max(preferred_size).max(padding_border);

    if item.flex_grow == 0.0 {
        contribution = contribution.min(item.flex_basis);
    }
    if item.flex_shrink == 0.0 {
        contribution = contribution.max(item.flex_basis);
    }

    let used_minimum =
        item.resolved_minimum_main_size.maybe_max(item.min_size_with_transfer.main(dir)).max(padding_border);
    contribution =
        contribution.maybe_clamp(Some(used_minimum), item.max_size_with_transfer.main(dir)).max(padding_border);
    contribution + item.margin.main_axis_sum(dir)
}

/// Compute the web-compatible intrinsic main size of a row flex container.
fn determine_row_intrinsic_main_size(
    tree: &mut impl LayoutFlexboxContainer,
    constraint: AvailableSpace,
    available_space: Size<AvailableSpace>,
    lines: &mut [FlexLine<'_>],
    constants: &AlgoConstants,
) -> f32 {
    let mut contribution_sum = 0.0;
    let mut largest_contribution: f32 = 0.0;
    let mut item_count = 0;
    for line in lines {
        for item in line.items.iter_mut() {
            let contribution =
                flex_item_intrinsic_main_contribution(tree, item, constraint, available_space, constants);
            contribution_sum += contribution;
            largest_contribution = largest_contribution.max(contribution);
            item_count += 1;
        }
    }

    if constraint == AvailableSpace::MinContent && constants.is_wrap {
        largest_contribution
    } else {
        contribution_sum + sum_axis_gaps(constants.gap.main(constants.dir), item_count)
    }
}

/// Compute the intrinsic block size used by a real column-flex layout.
///
/// This is deliberately based on outer hypothetical main sizes before flexing.
/// The min/max-content contribution algorithm is a different sizing operation:
/// applying its flex fractions here would let an item's ability to grow change
/// the container's automatic block size even when a definite flex basis fixes
/// the item's hypothetical size.
fn largest_line_hypothetical_outer_main_size(lines: &[FlexLine<'_>], constants: &AlgoConstants) -> f32 {
    lines
        .iter()
        .map(|line| {
            line.items.iter().map(|item| item.hypothetical_outer_size.main(constants.dir)).sum::<f32>()
                + sum_axis_gaps(constants.gap.main(constants.dir), line.items.len())
        })
        .max_by(|a, b| a.total_cmp(b))
        .unwrap_or(0.0)
}

/// Determine the container's main size (if not already known)
fn determine_container_main_size(
    tree: &mut impl LayoutFlexboxContainer,
    available_space: Size<AvailableSpace>,
    lines: &mut [FlexLine<'_>],
    constants: &mut AlgoConstants,
) {
    let dir = constants.dir;
    let main_content_box_inset = constants.content_box_inset.main_axis_sum(constants.dir);

    let specified_outer_main_size = constants.node_outer_size.main(constants.dir);
    let needs_content_based_block_resolution =
        constants.main_axis_is_block && constants.resolve_content_based_block_size;
    let needs_intrinsic_main_size = specified_outer_main_size.is_none() || needs_content_based_block_resolution;
    let intrinsic_outer_main_size = if !needs_intrinsic_main_size {
        None
    } else if constants.uses_layout_intrinsic_block_size {
        // Blink's layout result records a column container's intrinsic block
        // size from the largest sum of line hypothetical main sizes. Both a
        // real layout and a parent's block-axis content measurement consume
        // that value; neither is an intrinsic inline-contribution query.
        Some(largest_line_hypothetical_outer_main_size(lines, constants) + main_content_box_inset)
    } else if let Some(FlexIntrinsicInlineSize::Row(constraint)) = constants.intrinsic_inline_size {
        Some(
            determine_row_intrinsic_main_size(tree, constraint, available_space, lines, constants)
                + main_content_box_inset,
        )
    } else {
        Some(match available_space.main(dir) {
            AvailableSpace::Definite(main_axis_available_space) => {
                let longest_line_length: f32 = lines
                    .iter()
                    .map(|line| {
                        let line_main_axis_gap = sum_axis_gaps(constants.gap.main(constants.dir), line.items.len());
                        let total_target_size = line
                            .items
                            .iter()
                            .map(|child| {
                                let padding_border_sum = (child.padding + child.border).main_axis_sum(constants.dir);
                                (child.flex_basis.maybe_max(child.min_size.main(constants.dir))
                                    + child.margin.main_axis_sum(constants.dir))
                                .max(padding_border_sum)
                            })
                            .sum::<f32>();
                        total_target_size + line_main_axis_gap
                    })
                    .max_by(|a, b| a.total_cmp(b))
                    .unwrap_or(0.0);
                let size = longest_line_length + main_content_box_inset;
                if lines.len() > 1 {
                    f32_max(size, main_axis_available_space)
                } else {
                    size
                }
            }
            AvailableSpace::MinContent if constants.is_wrap => {
                let longest_line_length: f32 = lines
                    .iter()
                    .map(|line| {
                        let line_main_axis_gap = sum_axis_gaps(constants.gap.main(constants.dir), line.items.len());
                        let total_target_size = line
                            .items
                            .iter()
                            .map(|child| {
                                let padding_border_sum = (child.padding + child.border).main_axis_sum(constants.dir);
                                (child.flex_basis.maybe_max(child.min_size.main(constants.dir))
                                    + child.margin.main_axis_sum(constants.dir))
                                .max(padding_border_sum)
                            })
                            .sum::<f32>();
                        total_target_size + line_main_axis_gap
                    })
                    .max_by(|a, b| a.total_cmp(b))
                    .unwrap_or(0.0);
                longest_line_length + main_content_box_inset
            }
            AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                // Define a base main_size variable. This is mutated once for iteration over the outer
                // loop over the flex lines as:
                //   "The flex container’s max-content size is the largest sum of the afore-calculated sizes of all items within a single line."
                let mut main_size = 0.0;

                for line in lines.iter_mut() {
                    for item in line.items.iter_mut() {
                        let style_min = item.min_size.main(constants.dir);
                        let style_preferred = item.size.main(constants.dir);
                        let style_max = item.max_size.main(constants.dir);

                        // The spec seems a bit unclear on this point (my initial reading was that the `.maybe_max(style_preferred)` should
                        // not be included here), however this matches both Chrome and Firefox as of 9th March 2023.
                        //
                        // Spec: https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions
                        // Spec modification: https://www.w3.org/TR/css-flexbox-1/#change-2016-max-contribution
                        // Issue: https://github.com/w3c/csswg-drafts/issues/1435
                        // Gentest: padding_border_overrides_size_flex_basis_0.html
                        let clamping_basis = Some(item.flex_basis).maybe_max(style_preferred);
                        let flex_basis_min = clamping_basis.filter(|_| item.flex_shrink == 0.0);
                        let flex_basis_max = clamping_basis.filter(|_| item.flex_grow == 0.0);

                        let min_main_size = style_min
                            .maybe_max(flex_basis_min)
                            .or(flex_basis_min)
                            .unwrap_or(item.resolved_minimum_main_size)
                            .max(item.resolved_minimum_main_size);
                        let max_main_size =
                            style_max.maybe_min(flex_basis_max).or(flex_basis_max).unwrap_or(f32::INFINITY);

                        let content_contribution = match (min_main_size, style_preferred, max_main_size) {
                            // If the clamping values are such that max <= min, then we can avoid the expensive step of computing the content size
                            // as we know that the clamping values will override it anyway
                            (min, Some(pref), max) if max <= min || max <= pref => {
                                pref.min(max).max(min) + item.margin.main_axis_sum(constants.dir)
                            }
                            (min, _, max) if max <= min => min + item.margin.main_axis_sum(constants.dir),

                            // Else compute the min- or -max content size and apply the full formula for computing the
                            // min- or max- content contribution
                            _ if item.is_scroll_container() => {
                                item.flex_basis + item.margin.main_axis_sum(constants.dir)
                            }
                            _ => {
                                // Parent size for child sizing
                                let cross_axis_parent_size = constants.node_definite_inner_size.cross(dir);

                                // Available space for child sizing
                                let cross_axis_margin_sum = constants.margin.cross_axis_sum(dir);
                                let child_min_cross = item.min_size.cross(dir).maybe_add(cross_axis_margin_sum);
                                let child_max_cross = item.max_size.cross(dir).maybe_add(cross_axis_margin_sum);
                                let cross_axis_available_space = resolve_cross_axis_available_space(
                                    available_space.cross(dir),
                                    cross_axis_parent_size,
                                    child_min_cross,
                                    child_max_cross,
                                );

                                let child_available_space = available_space.with_cross(dir, cross_axis_available_space);

                                // Known dimensions for child sizing
                                let child_known_dimensions = {
                                    let mut ckd = item.size.with_main(dir, None);
                                    if item.align_self == AlignSelf::STRETCH && ckd.cross(dir).is_none() {
                                        ckd.set_cross(
                                            dir,
                                            cross_axis_available_space
                                                .into_option()
                                                .maybe_sub(item.margin.cross_axis_sum(dir)),
                                        );
                                    }
                                    ckd
                                };

                                // Either the min- or max- content size depending on which constraint we are sizing under.
                                // TODO: Optimise by using already computed values where available
                                debug_log!("COMPUTE CHILD BASE SIZE (for intrinsic main size):");
                                let measured = tree.measure_child_size_with_metadata(
                                    item.node,
                                    ChildLayoutInput::new(
                                        child_known_dimensions,
                                        constants.node_percentage_size,
                                        constants.writing_mode,
                                        child_available_space,
                                        SizingMode::InherentSize,
                                        Line::FALSE,
                                    )
                                    .with_definite_dimensions(
                                        flex_item_intrinsic_definite_dimensions(
                                            item,
                                            child_known_dimensions,
                                            constants,
                                        ),
                                    ),
                                    dir.main_axis().into(),
                                );
                                item.depends_on_block_constraints |= measured.depends_on_block_constraints;
                                let content_main_size =
                                    measured.size.get_abs(dir.main_axis()) + item.margin.main_axis_sum(constants.dir);

                                // This is somewhat bizarre in that it's asymmetrical depending whether the flex container is a column or a row.
                                //
                                // I *think* this might relate to https://drafts.csswg.org/css-flexbox-1/#algo-main-container:
                                //
                                //    "The automatic block size of a block-level flex container is its max-content size."
                                //
                                // Which could suggest that flex-basis defining a vertical size does not shrink because it is in the block axis, and the automatic size
                                // in the block axis is a MAX content size. Whereas a flex-basis defining a horizontal size does shrink because the automatic size in
                                // inline axis is MIN content size (although I don't have a reference for that).
                                //
                                // Ultimately, this was not found by reading the spec, but by trial and error fixing tests to align with Webkit/Firefox output.
                                // (see the `flex_basis_unconstraint_row` and `flex_basis_uncontraint_column` generated tests which demonstrate this)
                                if constants.main_axis_is_inline {
                                    content_main_size.maybe_clamp(style_min, style_max)
                                } else {
                                    content_main_size.max(item.flex_basis).maybe_clamp(style_min, style_max)
                                }
                            }
                        };
                        item.content_flex_fraction = {
                            let diff = content_contribution - item.flex_basis;
                            if diff > 0.0 {
                                diff / f32_max(1.0, item.flex_grow)
                            } else if diff < 0.0 {
                                let scaled_shrink_factor = f32_max(1.0, item.flex_shrink * item.inner_flex_basis);
                                diff / scaled_shrink_factor
                            } else {
                                // We are assuming that diff is 0.0 here and that we haven't accidentally introduced a NaN
                                0.0
                            }
                        };
                    }

                    // TODO Spec says to scale everything by the line's max flex fraction. But neither Chrome nor firefox implement this
                    // so we don't either. But if we did want to, we'd need this computation here (and to use it below):
                    //
                    // Within each line, find the largest max-content flex fraction among all the flex items.
                    // let line_flex_fraction = line
                    //     .items
                    //     .iter()
                    //     .map(|item| item.content_flex_fraction)
                    //     .max_by(|a, b| a.total_cmp(b))
                    //     .unwrap_or(0.0); // Unwrap case never gets hit because there is always at least one item a line

                    // Add each item’s flex base size to the product of:
                    //   - its flex grow factor (or scaled flex shrink factor,if the chosen max-content flex fraction was negative)
                    //   - the chosen max-content flex fraction
                    // then clamp that result by the max main size floored by the min main size.
                    //
                    // The flex container’s max-content size is the largest sum of the afore-calculated sizes of all items within a single line.
                    let item_main_size_sum = line
                        .items
                        .iter_mut()
                        .map(|item| {
                            let flex_fraction = item.content_flex_fraction;
                            // let flex_fraction = line_flex_fraction;

                            let flex_contribution = if item.content_flex_fraction > 0.0 {
                                f32_max(1.0, item.flex_grow) * flex_fraction
                            } else if item.content_flex_fraction < 0.0 {
                                let scaled_shrink_factor = f32_max(1.0, item.flex_shrink) * item.inner_flex_basis;
                                scaled_shrink_factor * flex_fraction
                            } else {
                                0.0
                            };
                            let size = item.flex_basis + flex_contribution;
                            item.outer_target_size.set_main(constants.dir, size);
                            item.target_size.set_main(constants.dir, size);
                            size
                        })
                        .sum::<f32>();

                    let gap_sum = sum_axis_gaps(constants.gap.main(constants.dir), line.items.len());
                    main_size = f32_max(main_size, item_main_size_sum + gap_sum)
                }

                main_size + main_content_box_inset
            }
        })
    };

    let intrinsic_constraints = if constants.main_axis_is_block && constants.resolve_content_based_block_size {
        intrinsic_outer_main_size
            .map(|intrinsic_size| {
                constants.content_based_block_size.resolve(
                    constants.writing_mode,
                    constants.writing_mode.to_logical(constants.node_outer_size).inline_size,
                    intrinsic_size,
                )
            })
            .unwrap_or_default()
    } else {
        Default::default()
    }
    .resolve_against(specified_outer_main_size, constants.content_based_block_size.resolved_constraints());

    let outer_main_size = intrinsic_constraints
        .preferred
        .or(intrinsic_outer_main_size)
        .unwrap_or(0.0)
        .maybe_clamp(intrinsic_constraints.min, intrinsic_constraints.max)
        .max(main_content_box_inset - constants.scrollbar_gutter.main(constants.dir));

    // let outer_main_size = inner_main_size + constants.padding_border.main_axis_sum(constants.dir);
    let inner_main_size = f32_max(outer_main_size - main_content_box_inset, 0.0);
    constants.container_size.set_main(constants.dir, outer_main_size);
    constants.inner_container_size.set_main(constants.dir, inner_main_size);
    constants.node_inner_size.set_main(constants.dir, Some(inner_main_size));
}

/// Resolve the flexible lengths of the items within a flex line.
/// Sets the `main` component of each item's `target_size` and `outer_target_size`
///
/// # [9.7. Resolving Flexible Lengths](https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths)
#[inline]
fn resolve_flexible_lengths(line: &mut FlexLine, constants: &AlgoConstants) {
    let total_main_axis_gap = sum_axis_gaps(constants.gap.main(constants.dir), line.items.len());

    // 1. Determine the used flex factor. Sum the outer hypothetical main sizes of all
    //    items on the line. If the sum is less than the flex container’s inner main size,
    //    use the flex grow factor for the rest of this algorithm; otherwise, use the
    //    flex shrink factor.

    let total_hypothetical_outer_main_size =
        line.items.iter().map(|child| child.hypothetical_outer_size.main(constants.dir)).sum::<f32>();
    let used_flex_factor: f32 = total_main_axis_gap + total_hypothetical_outer_main_size;
    let growing = used_flex_factor < constants.node_inner_size.main(constants.dir).unwrap_or(0.0);
    let shrinking = used_flex_factor > constants.node_inner_size.main(constants.dir).unwrap_or(0.0);
    let exactly_sized = !growing & !shrinking;

    // 2. Size inflexible items. Freeze, setting its target main size to its hypothetical main size
    //    - Any item that has a flex factor of zero
    //    - If using the flex grow factor: any item that has a flex base size
    //      greater than its hypothetical main size
    //    - If using the flex shrink factor: any item that has a flex base size
    //      smaller than its hypothetical main size

    for child in line.items.iter_mut() {
        let inner_target_size = child.hypothetical_inner_size.main(constants.dir);
        child.target_size.set_main(constants.dir, inner_target_size);

        if exactly_sized
            || (child.flex_grow == 0.0 && child.flex_shrink == 0.0)
            || (growing && child.flex_basis > child.hypothetical_inner_size.main(constants.dir))
            || (shrinking && child.flex_basis < child.hypothetical_inner_size.main(constants.dir))
        {
            child.frozen = true;
            let outer_target_size = inner_target_size + child.margin.main_axis_sum(constants.dir);
            child.outer_target_size.set_main(constants.dir, outer_target_size);
        }
    }

    if exactly_sized {
        return;
    }

    // 3. Calculate initial free space. Sum the outer sizes of all items on the line,
    //    and subtract this from the flex container’s inner main size. For frozen items,
    //    use their outer target main size; for other items, use their outer flex base size.

    let used_space: f32 = total_main_axis_gap
        + line
            .items
            .iter()
            .map(|child| {
                if child.frozen {
                    child.outer_target_size.main(constants.dir)
                } else {
                    child.flex_basis + child.margin.main_axis_sum(constants.dir)
                }
            })
            .sum::<f32>();

    let initial_free_space = constants.node_inner_size.main(constants.dir).maybe_sub(used_space).unwrap_or(0.0);

    // 4. Loop

    loop {
        // a. Check for flexible items. If all the flex items on the line are frozen,
        //    free space has been distributed; exit this loop.

        if line.items.iter().all(|child| child.frozen) {
            break;
        }

        // b. Calculate the remaining free space as for initial free space, above.
        //    If the sum of the unfrozen flex items’ flex factors is less than one,
        //    multiply the initial free space by this sum. If the magnitude of this
        //    value is less than the magnitude of the remaining free space, use this
        //    as the remaining free space.

        let used_space: f32 = total_main_axis_gap
            + line
                .items
                .iter()
                .map(|child| {
                    if child.frozen {
                        child.outer_target_size.main(constants.dir)
                    } else {
                        child.flex_basis + child.margin.main_axis_sum(constants.dir)
                    }
                })
                .sum::<f32>();

        let mut unfrozen: Vec<&mut FlexItem> = line.items.iter_mut().filter(|child| !child.frozen).collect();

        let (sum_flex_grow, sum_flex_shrink): (f32, f32) =
            unfrozen.iter().fold((0.0, 0.0), |(flex_grow, flex_shrink), item| {
                (flex_grow + item.flex_grow, flex_shrink + item.flex_shrink)
            });

        let free_space = if growing && sum_flex_grow < 1.0 {
            (initial_free_space * sum_flex_grow - total_main_axis_gap)
                .maybe_min(constants.node_inner_size.main(constants.dir).maybe_sub(used_space))
        } else if shrinking && sum_flex_shrink < 1.0 {
            (initial_free_space * sum_flex_shrink - total_main_axis_gap)
                .maybe_max(constants.node_inner_size.main(constants.dir).maybe_sub(used_space))
        } else {
            (constants.node_inner_size.main(constants.dir).maybe_sub(used_space))
                .unwrap_or(used_flex_factor - used_space)
        };

        // c. Distribute free space proportional to the flex factors.
        //    - If the remaining free space is zero
        //        Do Nothing
        //    - If using the flex grow factor
        //        Find the ratio of the item’s flex grow factor to the sum of the
        //        flex grow factors of all unfrozen items on the line. Set the item’s
        //        target main size to its flex base size plus a fraction of the remaining
        //        free space proportional to the ratio.
        //    - If using the flex shrink factor
        //        For every unfrozen item on the line, multiply its flex shrink factor by
        //        its inner flex base size, and note this as its scaled flex shrink factor.
        //        Find the ratio of the item’s scaled flex shrink factor to the sum of the
        //        scaled flex shrink factors of all unfrozen items on the line. Set the item’s
        //        target main size to its flex base size minus a fraction of the absolute value
        //        of the remaining free space proportional to the ratio. Note this may result
        //        in a negative inner main size; it will be corrected in the next step.
        //    - Otherwise
        //        Do Nothing

        if free_space.is_normal() {
            if growing && sum_flex_grow > 0.0 {
                for child in &mut unfrozen {
                    child
                        .target_size
                        .set_main(constants.dir, child.flex_basis + free_space * (child.flex_grow / sum_flex_grow));
                }
            } else if shrinking && sum_flex_shrink > 0.0 {
                let sum_scaled_shrink_factor: f32 =
                    unfrozen.iter().map(|child| child.inner_flex_basis * child.flex_shrink).sum();

                if sum_scaled_shrink_factor > 0.0 {
                    for child in &mut unfrozen {
                        let scaled_shrink_factor = child.inner_flex_basis * child.flex_shrink;
                        child.target_size.set_main(
                            constants.dir,
                            child.flex_basis + free_space * (scaled_shrink_factor / sum_scaled_shrink_factor),
                        )
                    }
                }
            }
        }

        // d. Fix min/max violations. Clamp each non-frozen item’s target main size by its
        //    used min and max main sizes and floor its content-box size at zero. If the
        //    item’s target main size was made smaller by this, it’s a max violation.
        //    If the item’s target main size was made larger by this, it’s a min violation.

        let total_violation = unfrozen.iter_mut().fold(0.0, |acc, child| -> f32 {
            let resolved_min_main: Option<f32> = child.resolved_minimum_main_size.into();
            let max_main = child.max_size.main(constants.dir);
            let clamped = child.target_size.main(constants.dir).maybe_clamp(resolved_min_main, max_main).max(0.0);
            child.violation = clamped - child.target_size.main(constants.dir);
            child.target_size.set_main(constants.dir, clamped);
            child.outer_target_size.set_main(
                constants.dir,
                child.target_size.main(constants.dir) + child.margin.main_axis_sum(constants.dir),
            );

            acc + child.violation
        });

        // e. Freeze over-flexed items. The total violation is the sum of the adjustments
        //    from the previous step ∑(clamped size - unclamped size). If the total violation is:
        //    - Zero
        //        Freeze all items.
        //    - Positive
        //        Freeze all the items with min violations.
        //    - Negative
        //        Freeze all the items with max violations.

        for child in &mut unfrozen {
            match total_violation {
                v if v > 0.0 => child.frozen = child.violation > 0.0,
                v if v < 0.0 => child.frozen = child.violation < 0.0,
                _ => child.frozen = true,
            }
        }

        // f. Return to the start of this loop.
    }
}

/// Determine the hypothetical cross size of each item.
///
/// # [9.4. Cross Size Determination](https://www.w3.org/TR/css-flexbox-1/#cross-sizing)
///
/// - [**Determine the hypothetical cross size of each item**](https://www.w3.org/TR/css-flexbox-1/#algo-cross-item)
///   by performing layout with the used main size and the available space, treating auto as fit-content.
#[inline]
fn determine_hypothetical_cross_size(
    tree: &mut impl LayoutFlexboxContainer,
    line: &mut FlexLine,
    constants: &AlgoConstants,
    available_space: Size<AvailableSpace>,
) {
    for child in line.items.iter_mut() {
        let padding_border = (child.padding + child.border).sum_axes();
        let padding_border_sum = padding_border.cross(constants.dir);

        let child_known_main = constants.container_size.main(constants.dir).into();

        // The flexed main size is a fixed input to hypothetical cross-size
        // layout. Content-size measurement intentionally ignores authored size
        // properties, so transfer that fixed input through the preferred ratio
        // at this ownership boundary, matching Blink's child constraint space.
        let ratio_cross = Size::NONE
            .with_main(constants.dir, Some(child.target_size.main(constants.dir)))
            .maybe_apply_aspect_ratio_with_box_sizing(child.aspect_ratio, BoxSizing::BorderBox, padding_border)
            .cross(constants.dir);
        child.cross_size_is_definite =
            child.preferred_cross_size_is_definite || (child.main_size_is_definite && ratio_cross.is_some());
        let initial_preferred_cross = if child.preferred_size_aspect_ratio_applied.cross(constants.dir) {
            ratio_cross
        } else {
            child.size.cross(constants.dir).or(ratio_cross)
        }
        .maybe_max(padding_border_sum);

        let initial_available_cross = available_space
            .cross(constants.dir)
            .maybe_clamp(
                child.min_size_with_transfer.cross(constants.dir),
                child.max_size_with_transfer.cross(constants.dir),
            )
            .maybe_max(padding_border_sum);
        let mut preferred_size = Size::NONE
            .with_main(constants.dir, Some(child.target_size.main(constants.dir)))
            .with_cross(constants.dir, initial_preferred_cross);

        // The flexed main size may establish the item's logical inline size
        // only after flexible lengths have been resolved. Complete logical
        // block sizing at that boundary so preferred-ratio transfer and its
        // content-based automatic minimum follow the same source ordering as
        // block, grid and out-of-flow layout.
        let child_writing_mode = tree.get_writing_mode(child.node);
        if child_writing_mode.block_axis() == constants.dir.cross_axis()
            && child.content_based_block_size.requires_resolution()
        {
            let child_available_space = Size::MAX_CONTENT
                .with_main(constants.dir, child_known_main)
                .with_cross(constants.dir, initial_available_cross);
            let measurement_input = ChildLayoutInput::new(
                preferred_size,
                constants.node_percentage_size,
                constants.writing_mode,
                child_available_space,
                SizingMode::ContentSize,
                Line::FALSE,
            )
            .with_definite_dimensions(flex_item_definite_dimensions(child, preferred_size, constants))
            .with_block_auto_behavior(AutoSizeBehavior::FitContent);
            let content_constraints = resolve_content_based_block_size_constraints(
                tree,
                child.node,
                measurement_input,
                child.content_based_block_size,
            );

            let mut min_size_with_transfer = child.min_size_with_transfer;
            let mut max_size_with_transfer = child.max_size_with_transfer;
            content_constraints.apply_to_block_axis(
                child_writing_mode,
                child.content_based_block_size.resolved_constraints(),
                padding_border,
                &mut preferred_size,
                &mut min_size_with_transfer,
                &mut max_size_with_transfer,
            );
            child.min_size_with_transfer = min_size_with_transfer;
            child.max_size_with_transfer = max_size_with_transfer;

            let mut size_without_transfer = preferred_size;
            content_constraints.apply_to_block_axis(
                child_writing_mode,
                child.block_axis_constraints_without_transfer,
                padding_border,
                &mut size_without_transfer,
                &mut child.min_size,
                &mut child.max_size,
            );
            child.depends_on_block_constraints |= content_constraints.depends_on_block_constraints;
        }

        // Sizes transferred through the aspect ratio clamp the hypothetical cross size
        // https://github.com/w3c/csswg-drafts/issues/10997
        let transferred_min_cross = child.min_size_with_transfer.cross(constants.dir);
        let transferred_max_cross = child.max_size_with_transfer.cross(constants.dir);
        let preferred_cross = preferred_size.cross(constants.dir);
        let child_available_cross = available_space
            .cross(constants.dir)
            .maybe_clamp(transferred_min_cross, transferred_max_cross)
            .maybe_max(padding_border_sum);

        let unclamped_child_cross = preferred_cross.unwrap_or_else(|| {
            let known_dimensions = Size {
                width: if constants.is_row { child.target_size.width.into() } else { None },
                height: if constants.is_row { None } else { child.target_size.height.into() },
            };
            let measured = tree.measure_child_size_with_metadata(
                child.node,
                ChildLayoutInput::new(
                    known_dimensions,
                    constants.node_percentage_size,
                    constants.writing_mode,
                    Size {
                        width: if constants.is_row { child_known_main } else { child_available_cross },
                        height: if constants.is_row { child_available_cross } else { child_known_main },
                    },
                    SizingMode::ContentSize,
                    Line::FALSE,
                )
                .with_definite_dimensions(flex_item_definite_dimensions(
                    child,
                    known_dimensions,
                    constants,
                )),
                constants.dir.cross_axis().into(),
            );
            child.depends_on_block_constraints |= measured.depends_on_block_constraints;
            measured.size.get_abs(constants.dir.cross_axis()).max(padding_border_sum)
        });
        let child_inner_cross =
            unclamped_child_cross.maybe_clamp(transferred_min_cross, transferred_max_cross).max(padding_border_sum);
        let child_outer_cross = child_inner_cross + child.margin.cross_axis_sum(constants.dir);

        child.unclamped_hypothetical_cross_size = unclamped_child_cross;
        child.hypothetical_inner_size.set_cross(constants.dir, child_inner_cross);
        child.hypothetical_outer_size.set_cross(constants.dir, child_outer_cross);
    }
}

/// Calculate the base lines of the children.
#[inline]
fn calculate_children_base_lines(
    tree: &mut impl LayoutFlexboxContainer,
    node_size: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    flex_lines: &mut [FlexLine],
    constants: &AlgoConstants,
) {
    let font_baseline = FontBaseline::for_writing_mode(constants.writing_mode);

    for line in flex_lines {
        for child in line.items.iter_mut() {
            // Only calculate baselines for children participating in baseline alignment.
            if !child.align_self.is_baseline() {
                continue;
            }

            let known_dimensions = Size {
                width: if constants.is_row {
                    child.target_size.width.into()
                } else {
                    child.hypothetical_inner_size.width.into()
                },
                height: if constants.is_row {
                    child.hypothetical_inner_size.height.into()
                } else {
                    child.target_size.height.into()
                },
            };
            let measured_size_and_baselines = tree.perform_child_layout(
                child.node,
                ChildLayoutInput::new(
                    known_dimensions,
                    constants.node_percentage_size,
                    constants.writing_mode,
                    Size {
                        width: if constants.is_row {
                            constants.container_size.width.into()
                        } else {
                            available_space.width.maybe_set(node_size.width)
                        },
                        height: if constants.is_row {
                            available_space.height.maybe_set(node_size.height)
                        } else {
                            constants.container_size.height.into()
                        },
                    },
                    SizingMode::ContentSize,
                    Line::FALSE,
                )
                .with_definite_dimensions(flex_item_definite_dimensions(
                    child,
                    known_dimensions,
                    constants,
                )),
            );

            let child_size = measured_size_and_baselines.size;
            let child_writing_mode = tree.get_writing_mode(child.node);
            let baseline_writing_direction = WritingDirection::new(child.baseline_writing_mode, Direction::Ltr);
            let baseline_block_size = child.baseline_writing_mode.to_logical(child_size).block_size;
            let baseline_set = if child.align_self.is_last_baseline() {
                measured_size_and_baselines.last_baselines
            } else {
                measured_size_and_baselines.first_baselines
            };
            let baseline = if child.baseline_writing_mode == child_writing_mode {
                logical_block_baseline(baseline_set, child_size, baseline_writing_direction).unwrap_or_else(|| {
                    synthesized_logical_baseline(baseline_block_size, baseline_writing_direction, font_baseline)
                })
            } else {
                synthesized_logical_baseline(baseline_block_size, baseline_writing_direction, font_baseline)
            };

            // Scroll containers' baselines are determined from their content as if scrolled to the
            // initial position, but are additionally clamped to their border box.
            // See https://github.com/w3c/csswg-drafts/issues/7660
            let baseline =
                if child.is_scroll_container() { baseline.min(baseline_block_size).max(0.0) } else { baseline };
            // Baseline metrics are measured from the sharing-group edge. First
            // baselines use cross-start (unless wrap-reverse), while last
            // baselines use cross-end.
            let baseline = if constants.wrap_reverse != child.align_self.is_last_baseline() {
                baseline_block_size - baseline
            } else {
                baseline
            };

            let cross_margins = constants.cross_axis_margins(child.margin);
            child.alignment_baseline = match child.baseline_group {
                BaselineGroup::Major => cross_margins.start + baseline,
                BaselineGroup::Minor => cross_margins.end + baseline,
            };
        }
    }
}

#[derive(Clone, Copy, Debug)]
/// Maximum ascent and descent collected for one baseline-sharing group.
struct BaselineMetrics {
    /// Largest distance from cross-start to the shared baseline.
    max_ascent: f32,
    /// Largest distance from the shared baseline to cross-end.
    max_descent: f32,
}

impl BaselineMetrics {
    /// Cross size required to contain the sharing group.
    fn cross_size(self) -> f32 {
        self.max_ascent + self.max_descent
    }
}

/// Collect line-sizing metrics for one baseline-sharing group.
fn collect_baseline_metrics(
    items: &[FlexItem],
    group: BaselineGroup,
    direction: FlexDirection,
) -> Option<BaselineMetrics> {
    items
        .iter()
        .filter(|child| {
            child.align_self.is_baseline()
                && child.baseline_group == group
                && !child.margin_is_auto.cross_start(direction)
                && !child.margin_is_auto.cross_end(direction)
        })
        .fold(None, |metrics, child| {
            let ascent = child.alignment_baseline;
            let descent = child.hypothetical_outer_size.cross(direction) - ascent;
            Some(match metrics {
                Some(metrics) => BaselineMetrics {
                    max_ascent: metrics.max_ascent.max(ascent),
                    max_descent: metrics.max_descent.max(descent),
                },
                None => BaselineMetrics { max_ascent: ascent, max_descent: descent },
            })
        })
}

/// Calculate the cross size of each flex line.
///
/// # [9.4. Cross Size Determination](https://www.w3.org/TR/css-flexbox-1/#cross-sizing)
///
/// - [**Calculate the cross size of each flex line**](https://www.w3.org/TR/css-flexbox-1/#algo-cross-line).
#[inline]
fn calculate_cross_size(flex_lines: &mut [FlexLine], node_size: Size<Option<f32>>, constants: &AlgoConstants) -> f32 {
    for line in flex_lines.iter_mut() {
        let major_metrics = collect_baseline_metrics(line.items, BaselineGroup::Major, constants.dir);
        let minor_metrics = collect_baseline_metrics(line.items, BaselineGroup::Minor, constants.dir);
        line.major_baseline = major_metrics.map(|metrics| metrics.max_ascent);
        line.minor_baseline = minor_metrics.map(|metrics| metrics.max_ascent);

        let max_outer_cross_size =
            line.items.iter().map(|child| child.hypothetical_outer_size.cross(constants.dir)).fold(0.0, f32::max);
        line.cross_size = major_metrics
            .map(BaselineMetrics::cross_size)
            .into_iter()
            .chain(minor_metrics.map(BaselineMetrics::cross_size))
            .fold(max_outer_cross_size, f32::max);
    }
    let intrinsic_line_cross_size = match constants.intrinsic_inline_size {
        Some(FlexIntrinsicInlineSize::ColumnWrap(AvailableSpace::MinContent)) => flex_lines
            .iter()
            .flat_map(|line| line.items.iter())
            .map(|item| item.hypothetical_outer_size.cross(constants.dir))
            .fold(0.0, f32::max),
        _ => flex_lines.iter().map(|line| line.cross_size).sum(),
    };

    // If the flex container is single-line and has a definite cross size,
    // the cross size of the flex line is the flex container’s inner cross size.
    if !constants.is_wrap && node_size.cross(constants.dir).is_some() {
        let cross_axis_padding_border = constants.content_box_inset.cross_axis_sum(constants.dir);
        let cross_min_size = constants.min_size.cross(constants.dir);
        let cross_max_size = constants.max_size.cross(constants.dir);
        flex_lines[0].cross_size = node_size
            .cross(constants.dir)
            .maybe_clamp(cross_min_size, cross_max_size)
            .maybe_sub(cross_axis_padding_border)
            .maybe_max(0.0)
            .unwrap_or(0.0);
    } else {
        // Otherwise, for each flex line:
        //
        //    1. Collect all the flex items whose inline-axis is parallel to the main-axis, whose
        //       align-self is baseline, and whose cross-axis margins are both non-auto. Find the
        //       largest of the distances between each item’s baseline and its hypothetical outer
        //       cross-start edge, and the largest of the distances between each item’s baseline
        //       and its hypothetical outer cross-end edge, and sum these two values.

        //    2. Among all the items not collected by the previous step, find the largest
        //       outer hypothetical cross size.

        //    3. The used cross-size of the flex line is the largest of the numbers found in the
        //       previous two steps and zero.
        // If the flex container is single-line, then clamp the line’s cross-size to be within the container’s computed min and max cross sizes.
        // Note that if CSS 2.1’s definition of min/max-width/height applied more generally, this behavior would fall out automatically.
        if !constants.is_wrap {
            let cross_axis_padding_border = constants.content_box_inset.cross_axis_sum(constants.dir);
            let cross_min_size = constants.min_size.cross(constants.dir);
            let cross_max_size = constants.max_size.cross(constants.dir);
            flex_lines[0].cross_size = flex_lines[0].cross_size.maybe_clamp(
                cross_min_size.maybe_sub(cross_axis_padding_border),
                cross_max_size.maybe_sub(cross_axis_padding_border),
            );
        }
    }
    intrinsic_line_cross_size
}

/// Handle 'align-content: stretch'.
///
/// # [9.4. Cross Size Determination](https://www.w3.org/TR/css-flexbox-1/#cross-sizing)
///
/// - [**Handle 'align-content: stretch'**](https://www.w3.org/TR/css-flexbox-1/#algo-line-stretch). If the flex container has a definite cross size, align-content is stretch,
///   and the sum of the flex lines' cross sizes is less than the flex container’s inner cross size,
///   increase the cross size of each flex line by equal amounts such that the sum of their cross sizes exactly equals the flex container’s inner cross size.
#[inline]
fn handle_align_content_stretch(flex_lines: &mut [FlexLine], node_size: Size<Option<f32>>, constants: &AlgoConstants) {
    if constants.align_content == AlignContent::STRETCH {
        let cross_axis_padding_border = constants.content_box_inset.cross_axis_sum(constants.dir);
        let cross_min_size = constants.min_size.cross(constants.dir);
        let cross_max_size = constants.max_size.cross(constants.dir);
        let container_min_inner_cross = node_size
            .cross(constants.dir)
            .or(cross_min_size)
            .maybe_clamp(cross_min_size, cross_max_size)
            .maybe_sub(cross_axis_padding_border)
            .maybe_max(0.0)
            .unwrap_or(0.0);

        let total_cross_axis_gap = sum_axis_gaps(constants.gap.cross(constants.dir), flex_lines.len());
        let lines_total_cross: f32 = flex_lines.iter().map(|line| line.cross_size).sum::<f32>() + total_cross_axis_gap;

        if lines_total_cross < container_min_inner_cross {
            let remaining = container_min_inner_cross - lines_total_cross;
            let addition = remaining / flex_lines.len() as f32;
            flex_lines.iter_mut().for_each(|line| line.cross_size += addition);
        }
    }
}

/// Determine the used cross size of each flex item.
///
/// # [9.4. Cross Size Determination](https://www.w3.org/TR/css-flexbox-1/#cross-sizing)
///
/// - [**Determine the used cross size of each flex item**](https://www.w3.org/TR/css-flexbox-1/#algo-stretch). If a flex item has align-self: stretch, its computed cross size property is auto,
///   and neither of its cross-axis margins are auto, the used outer cross size is the used cross size of its flex line, clamped according to the item’s used min and max cross sizes.
///   Otherwise, the used cross size is the item’s hypothetical cross size.
///
///   If the flex item has align-self: stretch, redo layout for its contents, treating this used size as its definite cross size so that percentage-sized children can be resolved.
///
///   **Note that this step does not affect the main size of the flex item, even if it has an intrinsic aspect ratio**.
#[inline]
fn determine_used_cross_size(
    tree: &impl LayoutFlexboxContainer,
    flex_lines: &mut [FlexLine],
    constants: &AlgoConstants,
) {
    for line in flex_lines {
        let line_cross_size = line.cross_size;

        for child in line.items.iter_mut() {
            let child_style = tree.get_flexbox_child_style(child.node);
            let percentage_basis = constants.writing_mode.to_logical(constants.node_percentage_size).inline_size;
            let padding = child_style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
            let border = child_style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
            let padding_border = (padding + border).sum_axes();
            let available_cross_size = f32_max(line_cross_size - child.margin.cross_axis_sum(constants.dir), 0.0);
            let stretch =
                child.stretch.resolve(Size::NONE.with_cross(constants.dir, Some(available_cross_size)), padding_border);
            let min_cross_size = stretch.min.cross(constants.dir).or(child.min_size.cross(constants.dir));
            let max_cross_size = stretch.max.cross(constants.dir).or(child.max_size.cross(constants.dir));

            let re_resolves_stretch_limit =
                stretch.min.cross(constants.dir).is_some() || stretch.max.cross(constants.dir).is_some();
            let stretches_auto_cross_size = child.align_self == AlignSelf::STRETCH
                && !child.margin_is_auto.cross_start(constants.dir)
                && !child.margin_is_auto.cross_end(constants.dir)
                && child_style.size().cross(constants.dir).is_auto();
            // A stretch min/max constraint may clamp the used size, but does
            // not turn an otherwise content-sized cross axis into a definite
            // percentage basis. Only a preferred stretch value or the flex
            // align-self stretch rule establishes that provenance.
            child.cross_size_is_definite = child.cross_size_is_definite
                || stretch.preferred.cross(constants.dir).is_some()
                || stretches_auto_cross_size;
            let used_cross_size = stretch.preferred.cross(constants.dir).unwrap_or_else(|| {
                if re_resolves_stretch_limit {
                    child.unclamped_hypothetical_cross_size
                } else if stretches_auto_cross_size {
                    available_cross_size
                } else {
                    child.hypothetical_inner_size.cross(constants.dir)
                }
            });
            child.target_size.set_cross(constants.dir, used_cross_size.maybe_clamp(min_cross_size, max_cross_size));

            child.outer_target_size.set_cross(
                constants.dir,
                child.target_size.cross(constants.dir) + child.margin.cross_axis_sum(constants.dir),
            );
        }
    }
}

/// Distribute any remaining free space.
///
/// # [9.5. Main-Axis Alignment](https://www.w3.org/TR/css-flexbox-1/#main-alignment)
///
/// - [**Distribute any remaining free space**](https://www.w3.org/TR/css-flexbox-1/#algo-main-align). For each flex line:
///
///   1. If the remaining free space is positive and at least one main-axis margin on this line is `auto`, distribute the free space equally among these margins.
///      Otherwise, set all `auto` margins to zero.
///
///   2. Align the items along the main-axis per `justify-content`.
#[inline]
fn distribute_remaining_free_space(flex_lines: &mut [FlexLine], constants: &AlgoConstants) {
    for line in flex_lines {
        let total_main_axis_gap = sum_axis_gaps(constants.gap.main(constants.dir), line.items.len());
        let used_space: f32 = total_main_axis_gap
            + line.items.iter().map(|child| child.outer_target_size.main(constants.dir)).sum::<f32>();
        let free_space = constants.inner_container_size.main(constants.dir) - used_space;
        let mut num_auto_margins = 0;

        for child in line.items.iter_mut() {
            if child.margin_is_auto.main_start(constants.dir) {
                num_auto_margins += 1;
            }
            if child.margin_is_auto.main_end(constants.dir) {
                num_auto_margins += 1;
            }
        }

        let justification_free_space = if free_space > 0.0 && num_auto_margins > 0 {
            let margin = free_space / num_auto_margins as f32;

            for child in line.items.iter_mut() {
                if child.margin_is_auto.main_start(constants.dir) {
                    if constants.is_row {
                        child.margin.left = margin;
                    } else {
                        child.margin.top = margin;
                    }
                }
                if child.margin_is_auto.main_end(constants.dir) {
                    if constants.is_row {
                        child.margin.right = margin;
                    } else {
                        child.margin.bottom = margin;
                    }
                }
            }
            // Positive free space has been consumed in full by the auto
            // margins, so justify-content has no remaining space to align.
            0.0
        } else {
            free_space
        };

        let num_items = line.items.len();
        let layout_reverse = constants.dir.is_reverse();
        let gap = constants.gap.main(constants.dir);
        let raw_justify_content_mode = constants.justify_content.unwrap_or(JustifyContent::FLEX_START);
        let justify_content_mode =
            apply_alignment_fallback(justification_free_space, num_items, raw_justify_content_mode);

        let justify_item = |(i, child): (usize, &mut FlexItem)| {
            child.offset_main = compute_alignment_offset(
                justification_free_space,
                num_items,
                gap,
                justify_content_mode,
                layout_reverse,
                i == 0,
            );
        };

        if layout_reverse {
            line.items.iter_mut().rev().enumerate().for_each(justify_item);
        } else {
            line.items.iter_mut().enumerate().for_each(justify_item);
        }
    }
}

/// Resolve cross-axis `auto` margins.
///
/// # [9.6. Cross-Axis Alignment](https://www.w3.org/TR/css-flexbox-1/#cross-alignment)
///
/// - [**Resolve cross-axis `auto` margins**](https://www.w3.org/TR/css-flexbox-1/#algo-cross-margins).
///   If a flex item has auto cross-axis margins:
///
///   - If its outer cross size (treating those auto margins as zero) is less than the cross size of its flex line,
///     distribute the difference in those sizes equally to the auto margins.
///
///   - Otherwise, if the block-start or inline-start margin (whichever is in the cross axis) is auto, set it to zero.
///     Set the opposite margin so that the outer cross size of the item equals the cross size of its flex line.
#[inline]
fn resolve_cross_axis_auto_margins(flex_lines: &mut [FlexLine], constants: &AlgoConstants) {
    for line in flex_lines {
        let line_cross_size = line.cross_size;
        let major_baseline = line.major_baseline;
        let minor_baseline = line.minor_baseline;

        for child in line.items.iter_mut() {
            let free_space = line_cross_size - child.outer_target_size.cross(constants.dir);

            if child.margin_is_auto.cross_start(constants.dir) && child.margin_is_auto.cross_end(constants.dir) {
                if constants.is_row {
                    child.margin.top = free_space / 2.0;
                    child.margin.bottom = free_space / 2.0;
                } else {
                    child.margin.left = free_space / 2.0;
                    child.margin.right = free_space / 2.0;
                }
            } else if child.margin_is_auto.cross_start(constants.dir) {
                if constants.is_row {
                    child.margin.top = free_space;
                } else {
                    child.margin.left = free_space;
                }
            } else if child.margin_is_auto.cross_end(constants.dir) {
                if constants.is_row {
                    child.margin.bottom = free_space;
                } else {
                    child.margin.right = free_space;
                }
            } else {
                // 14. Align all flex items along the cross-axis.
                let shared_baseline = match child.baseline_group {
                    BaselineGroup::Major => major_baseline,
                    BaselineGroup::Minor => minor_baseline,
                };
                child.offset_cross = align_flex_items_along_cross_axis(child, free_space, shared_baseline, constants);
            }
        }
    }
}

/// Align all flex items along the cross-axis.
///
/// # [9.6. Cross-Axis Alignment](https://www.w3.org/TR/css-flexbox-1/#cross-alignment)
///
/// - [**Align all flex items along the cross-axis**](https://www.w3.org/TR/css-flexbox-1/#algo-cross-align) per `align-self`,
///   if neither of the item's cross-axis margins are `auto`.
#[inline]
fn align_flex_items_along_cross_axis(
    child: &FlexItem,
    free_space: f32,
    shared_baseline: Option<f32>,
    constants: &AlgoConstants,
) -> f32 {
    // If align-self uses a "safe" overflow-position keyword and the item would overflow its
    // line cross size, fall back to logical Start to avoid data loss. See CSS Box Alignment 3
    // §4.3 <https://www.w3.org/TR/css-align-3/#overflow-values>. Otherwise, drop the safety
    // field so the match below operates on a bare keyword and stays exhaustive.
    let align_keyword = if child.align_self.is_safe() && free_space < 0.0 {
        AlignItemsKeyword::Start
    } else {
        child.align_self.keyword
    };

    match align_keyword {
        AlignItemsKeyword::Start => {
            if constants.cross_axis_start_reversed {
                free_space
            } else {
                0.0
            }
        }
        AlignItemsKeyword::FlexStart => {
            if constants.cross_axis_flex_start_reversed {
                free_space
            } else {
                0.0
            }
        }
        AlignItemsKeyword::End => {
            if constants.cross_axis_start_reversed {
                0.0
            } else {
                free_space
            }
        }
        AlignItemsKeyword::FlexEnd => {
            if constants.cross_axis_flex_start_reversed {
                0.0
            } else {
                free_space
            }
        }
        AlignItemsKeyword::Center => free_space / 2.0,
        AlignItemsKeyword::Baseline | AlignItemsKeyword::LastBaseline => {
            let baseline_delta = shared_baseline.unwrap_or(child.alignment_baseline) - child.alignment_baseline;
            let logical_offset = match child.baseline_group {
                BaselineGroup::Major => baseline_delta,
                BaselineGroup::Minor => free_space - baseline_delta,
            };
            if constants.cross_axis_start_reversed {
                free_space - logical_offset
            } else {
                logical_offset
            }
        }
        AlignItemsKeyword::Stretch => {
            if constants.cross_axis_flex_start_reversed {
                free_space
            } else {
                0.0
            }
        }
        // SelfStart/SelfEnd are resolved to Start/End against the item's own direction when
        // flex items are generated.
        AlignItemsKeyword::Normal
        | AlignItemsKeyword::SelfStart
        | AlignItemsKeyword::SelfEnd
        | AlignItemsKeyword::Left
        | AlignItemsKeyword::Right => unreachable!(),
    }
}

/// Determine the flex container’s used cross size.
///
/// # [9.6. Cross-Axis Alignment](https://www.w3.org/TR/css-flexbox-1/#cross-alignment)
///
/// - [**Determine the flex container’s used cross size**](https://www.w3.org/TR/css-flexbox-1/#algo-cross-container):
///
///     - If the cross size property is a definite size, use that, clamped by the used min and max cross sizes of the flex container.
///
///     - Otherwise, use the sum of the flex lines' cross sizes, clamped by the used min and max cross sizes of the flex container.
#[inline]
fn determine_container_cross_size(
    flex_lines: &[FlexLine],
    node_size: Size<Option<f32>>,
    intrinsic_line_cross_size: f32,
    constants: &mut AlgoConstants,
) {
    let total_cross_axis_gap = sum_axis_gaps(constants.gap.cross(constants.dir), flex_lines.len());

    let padding_border_sum = constants.content_box_inset.cross_axis_sum(constants.dir);
    let cross_scrollbar_gutter = constants.scrollbar_gutter.cross(constants.dir);
    let intrinsic_outer_cross_size = intrinsic_line_cross_size + total_cross_axis_gap + padding_border_sum;
    let intrinsic_constraints = if constants.main_axis_is_inline && constants.resolve_content_based_block_size {
        constants.content_based_block_size.resolve(
            constants.writing_mode,
            constants.writing_mode.to_logical(constants.node_outer_size).inline_size,
            intrinsic_outer_cross_size,
        )
    } else {
        Default::default()
    }
    .resolve_against(node_size.cross(constants.dir), constants.content_based_block_size.resolved_constraints());
    let outer_container_size = intrinsic_constraints
        .preferred
        .unwrap_or(intrinsic_outer_cross_size)
        .maybe_clamp(intrinsic_constraints.min, intrinsic_constraints.max)
        .max(padding_border_sum - cross_scrollbar_gutter);
    let inner_container_size = f32_max(outer_container_size - padding_border_sum, 0.0);

    constants.container_size.set_cross(constants.dir, outer_container_size);
    constants.inner_container_size.set_cross(constants.dir, inner_container_size);
}

/// Align all flex lines per `align-content`.
///
/// # [9.6. Cross-Axis Alignment](https://www.w3.org/TR/css-flexbox-1/#cross-alignment)
///
/// - [**Align all flex lines**](https://www.w3.org/TR/css-flexbox-1/#algo-line-align) per `align-content`.
#[inline]
fn align_flex_lines_per_align_content(flex_lines: &mut [FlexLine], constants: &AlgoConstants, total_cross_size: f32) {
    let num_lines = flex_lines.len();
    let gap = constants.gap.cross(constants.dir);
    let total_cross_axis_gap = sum_axis_gaps(gap, num_lines);
    let free_space = constants.inner_container_size.cross(constants.dir) - total_cross_size - total_cross_axis_gap;

    let align_content_mode = apply_alignment_fallback(free_space, num_lines, constants.align_content);

    let align_line = |(i, line): (usize, &mut FlexLine)| {
        line.offset_cross = compute_alignment_offset(
            free_space,
            num_lines,
            gap,
            align_content_mode,
            constants.cross_axis_reversed,
            i == 0,
        );
    };

    if constants.cross_axis_reversed {
        flex_lines.iter_mut().rev().enumerate().for_each(align_line);
    } else {
        flex_lines.iter_mut().enumerate().for_each(align_line);
    }
}

/// Calculates the layout for a flex-item.
fn calculate_flex_item(
    tree: &mut impl LayoutFlexboxContainer,
    item: &mut FlexItem,
    total_offset_main: &mut f32,
    total_offset_cross: f32,
    line_offset_cross: f32,
    #[cfg(feature = "content_size")] total_content_size: &mut Size<f32>,
    constants: &AlgoConstants,
) {
    let direction = constants.dir;
    let horizontal_direction = constants.horizontal_direction;
    let known_dimensions = item.target_size.map(Some);
    let layout_output = tree.perform_child_layout(
        item.node,
        ChildLayoutInput::new(
            known_dimensions,
            constants.node_percentage_size,
            constants.writing_mode,
            constants.container_size.map(|s| s.into()),
            SizingMode::ContentSize,
            Line::FALSE,
        )
        .with_definite_dimensions(flex_item_definite_dimensions(item, known_dimensions, constants)),
    );
    let LayoutOutput {
        size,
        #[cfg(feature = "content_size")]
        content_size,
        ..
    } = layout_output;

    let is_rtl_row = direction.is_row() && horizontal_direction.is_rtl();
    let is_rtl_column = direction.is_column() && horizontal_direction.is_rtl();
    let main_relative_inset = if is_rtl_row {
        item.inset.main_end(direction).or(item.inset.main_start(direction).map(|pos| -pos)).unwrap_or(0.0)
    } else {
        item.inset.main_start(direction).or(item.inset.main_end(direction).map(|pos| -pos)).unwrap_or(0.0)
    };
    let cross_relative_inset = if is_rtl_column {
        item.inset.cross_end(direction).map(|pos| -pos).or(item.inset.cross_start(direction)).unwrap_or(0.0)
    } else {
        item.inset.cross_start(direction).or(item.inset.cross_end(direction).map(|pos| -pos)).unwrap_or(0.0)
    };
    let effective_line_offset_cross = if is_rtl_column { 0.0 } else { line_offset_cross };

    let static_offset_main = if is_rtl_row {
        *total_offset_main - item.offset_main - item.margin.main_end(direction) - size.width
    } else {
        *total_offset_main + item.offset_main + item.margin.main_start(direction)
    };
    let offset_main =
        if is_rtl_row { static_offset_main - main_relative_inset } else { static_offset_main + main_relative_inset };

    let static_offset_cross =
        total_offset_cross + item.offset_cross + effective_line_offset_cross + item.margin.cross_start(direction);
    let offset_cross = static_offset_cross + cross_relative_inset;

    let static_location = if direction.is_row() {
        Point { x: static_offset_main, y: static_offset_cross }
    } else {
        Point { x: static_offset_cross, y: static_offset_main }
    };
    let location = if direction.is_row() {
        Point { x: offset_main, y: offset_cross }
    } else {
        Point { x: offset_cross, y: offset_main }
    };

    // Fragment baselines are stored in physical x/y by child layout. Project
    // them into the parent's logical block axis and keep them there while the
    // flex container selects its own first and last baseline. Relative
    // positioning intentionally does not alter the in-flow baseline position.
    let writing_direction = constants.writing_direction();
    let font_baseline = FontBaseline::for_writing_mode(constants.writing_mode);
    let child_block_size = constants.writing_mode.to_logical(size).block_size;
    let logical_block_offset =
        writing_direction.converter(constants.container_size).to_logical_point(static_location, size).block_offset;
    let clamp_baseline = |baseline: f32| {
        if item.is_scroll_container() {
            baseline.min(child_block_size).max(0.0)
        } else {
            baseline
        }
    };
    let inner_first_baseline = clamp_baseline(logical_block_baseline_or_synthesize(
        layout_output.first_baselines,
        size,
        writing_direction,
        font_baseline,
    ));
    let inner_last_baseline = clamp_baseline(logical_block_baseline_or_synthesize(
        layout_output.last_baselines,
        size,
        writing_direction,
        font_baseline,
    ));
    item.first_block_baseline = logical_block_offset + inner_first_baseline;
    item.last_block_baseline = logical_block_offset + inner_last_baseline;

    let scrollbar_size = Size {
        width: if item.overflow.y == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
        height: if item.overflow.x == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
    };

    tree.set_unrounded_layout(
        item.node,
        &Layout {
            order: item.order,
            size,
            #[cfg(feature = "content_size")]
            content_size,
            scrollbar_size,
            location,
            padding: item.padding,
            border: item.border,
            margin: item.margin,
        },
    );

    if is_rtl_row {
        *total_offset_main -= item.offset_main + item.margin.main_axis_sum(direction) + size.main(direction);
    } else {
        *total_offset_main += item.offset_main + item.margin.main_axis_sum(direction) + size.main(direction);
    }

    #[cfg(feature = "content_size")]
    {
        let contribution_location = if horizontal_direction.is_rtl() {
            Point {
                x: constants.container_size.width - (location.x + size.width) - constants.border.right,
                y: location.y - constants.border.top,
            }
        } else {
            Point { x: location.x - constants.border.left, y: location.y - constants.border.top }
        };
        *total_content_size = total_content_size.f32_max(compute_content_size_contribution(
            contribution_location,
            size,
            content_size,
            item.overflow,
        ));
    }
}

/// Calculates the layout line.
fn calculate_layout_line(
    tree: &mut impl LayoutFlexboxContainer,
    line: &mut FlexLine,
    total_offset_cross: &mut f32,
    #[cfg(feature = "content_size")] content_size: &mut Size<f32>,
    constants: &AlgoConstants,
) {
    let direction = constants.dir;
    let horizontal_direction = constants.horizontal_direction;
    let mut total_offset_main = if horizontal_direction.is_rtl() && direction.is_row() {
        constants.container_size.width - constants.content_box_inset.main_end(direction)
    } else {
        constants.content_box_inset.main_start(direction)
    };
    let line_offset_cross = line.offset_cross;

    let is_rtl_column = horizontal_direction.is_rtl() && direction.is_column();
    if is_rtl_column {
        *total_offset_cross -= line_offset_cross + line.cross_size;
    }

    if direction.is_reverse() {
        for item in line.items.iter_mut().rev() {
            calculate_flex_item(
                tree,
                item,
                &mut total_offset_main,
                *total_offset_cross,
                line_offset_cross,
                #[cfg(feature = "content_size")]
                content_size,
                constants,
            );
        }
    } else {
        for item in line.items.iter_mut() {
            calculate_flex_item(
                tree,
                item,
                &mut total_offset_main,
                *total_offset_cross,
                line_offset_cross,
                #[cfg(feature = "content_size")]
                content_size,
                constants,
            );
        }
    }

    if !is_rtl_column {
        *total_offset_cross += line_offset_cross + line.cross_size;
    }
}

/// Do a final layout pass and collect the resulting layouts.
#[inline]
fn final_layout_pass(
    tree: &mut impl LayoutFlexboxContainer,
    flex_lines: &mut [FlexLine],
    constants: &AlgoConstants,
) -> Size<f32> {
    let mut total_offset_cross = if constants.is_column && constants.horizontal_direction.is_rtl() {
        constants.container_size.width - constants.content_box_inset.cross_end(constants.dir)
    } else {
        constants.content_box_inset.cross_start(constants.dir)
    };

    #[cfg_attr(not(feature = "content_size"), allow(unused_mut))]
    let mut content_size = Size::ZERO;

    if constants.cross_axis_reversed {
        for line in flex_lines.iter_mut().rev() {
            calculate_layout_line(
                tree,
                line,
                &mut total_offset_cross,
                #[cfg(feature = "content_size")]
                &mut content_size,
                constants,
            );
        }
    } else {
        for line in flex_lines.iter_mut() {
            calculate_layout_line(
                tree,
                line,
                &mut total_offset_cross,
                #[cfg(feature = "content_size")]
                &mut content_size,
                constants,
            );
        }
    }

    content_size.width += if constants.horizontal_direction.is_rtl() {
        constants.content_box_inset.left - constants.border.left - constants.scrollbar_gutter.x
    } else {
        constants.content_box_inset.right - constants.border.right - constants.scrollbar_gutter.x
    };
    content_size.height += constants.content_box_inset.bottom - constants.border.bottom - constants.scrollbar_gutter.y;

    content_size
}

#[inline]
/// Map main-axis content alignment to a size-independent static edge.
fn flex_main_static_position_edge(justify_content: JustifyContent, reverse: bool) -> StaticPositionEdge {
    match justify_content.keyword() {
        AlignContentKeyword::Center | AlignContentKeyword::SpaceAround | AlignContentKeyword::SpaceEvenly => {
            StaticPositionEdge::Center
        }
        AlignContentKeyword::Start | AlignContentKeyword::Baseline => StaticPositionEdge::Start,
        AlignContentKeyword::End | AlignContentKeyword::LastBaseline => StaticPositionEdge::End,
        AlignContentKeyword::FlexEnd => {
            if reverse {
                StaticPositionEdge::Start
            } else {
                StaticPositionEdge::End
            }
        }
        AlignContentKeyword::FlexStart | AlignContentKeyword::Stretch | AlignContentKeyword::SpaceBetween => {
            if reverse {
                StaticPositionEdge::End
            } else {
                StaticPositionEdge::Start
            }
        }
    }
}

#[inline]
/// Map cross-axis self alignment to a size-independent static edge.
fn flex_cross_static_position_edge(align_self: AlignSelf, wrap_reverse: bool) -> StaticPositionEdge {
    match align_self.keyword() {
        AlignItemsKeyword::Center => StaticPositionEdge::Center,
        AlignItemsKeyword::End | AlignItemsKeyword::LastBaseline => StaticPositionEdge::End,
        AlignItemsKeyword::FlexEnd => {
            if wrap_reverse {
                StaticPositionEdge::Start
            } else {
                StaticPositionEdge::End
            }
        }
        AlignItemsKeyword::FlexStart | AlignItemsKeyword::Stretch => {
            if wrap_reverse {
                StaticPositionEdge::End
            } else {
                StaticPositionEdge::Start
            }
        }
        AlignItemsKeyword::Start | AlignItemsKeyword::Baseline => StaticPositionEdge::Start,
        AlignItemsKeyword::Normal
        | AlignItemsKeyword::SelfStart
        | AlignItemsKeyword::SelfEnd
        | AlignItemsKeyword::Left
        | AlignItemsKeyword::Right => {
            unreachable!("axis-relative alignment is resolved before static-position generation")
        }
    }
}

#[inline]
/// Build the static-position candidate contributed by a flex formatting context.
fn flex_static_position(constants: &AlgoConstants, align_self: AlignSelf) -> LogicalStaticPosition {
    let writing_direction = constants.writing_direction();
    let logical_outer_size = constants.writing_mode.to_logical(constants.container_size);
    let logical_content_inset = writing_direction.to_logical_box_strut(constants.content_box_inset);
    let logical_content_size = LogicalSize {
        inline_size: f32_max(
            logical_outer_size.inline_size - logical_content_inset.inline_start - logical_content_inset.inline_end,
            0.0,
        ),
        block_size: f32_max(
            logical_outer_size.block_size - logical_content_inset.block_start - logical_content_inset.block_end,
            0.0,
        ),
    };
    let main_edge = flex_main_static_position_edge(
        constants.justify_content.unwrap_or(JustifyContent::FLEX_START),
        constants.authored_main_reversed,
    );
    let cross_edge = flex_cross_static_position_edge(align_self, constants.wrap_reverse);
    let (inline_edge, block_edge, align_self_axis) = if constants.main_axis_is_inline {
        (main_edge, cross_edge, AbstractAxis::Block)
    } else {
        (cross_edge, main_edge, AbstractAxis::Inline)
    };

    let anchor = |start: f32, size: f32, edge: StaticPositionEdge| match edge {
        StaticPositionEdge::Start => start,
        StaticPositionEdge::Center => start + size / 2.0,
        StaticPositionEdge::End => start + size,
    };
    LogicalStaticPosition {
        offset: LogicalOffset {
            inline_offset: anchor(logical_content_inset.inline_start, logical_content_size.inline_size, inline_edge),
            block_offset: anchor(logical_content_inset.block_start, logical_content_size.block_size, block_edge),
        },
        inline_edge,
        block_edge,
        align_self_axis,
    }
}

/// Emit flex static-position candidates and lay out the children for which this
/// flex box supplies the actual containing block.
fn perform_absolute_layout_on_absolute_children(
    tree: &mut impl LayoutFlexboxContainer,
    node: NodeId,
    constants: &AlgoConstants,
) -> Size<f32> {
    let area_size = constants.container_size - constants.border.sum_axes() - constants.scrollbar_gutter.into();
    let area_offset = Point {
        x: constants.border.left
            + if constants.horizontal_direction.is_rtl() { constants.scrollbar_gutter.x } else { 0.0 },
        y: constants.border.top,
    };
    let containing_block = OutOfFlowContainingBlock {
        outer_size: constants.container_size,
        area_offset,
        area_size,
        writing_direction: constants.writing_direction(),
    };
    let mut content_size = Size::ZERO;

    let numeric_children: Vec<_> = tree.child_ids(node).collect();
    let candidate_count = tree.out_of_flow_candidate_count(node);
    let candidates: Vec<_> = (0..candidate_count).map(|index| tree.get_out_of_flow_candidate(node, index)).collect();
    let mut children = Vec::with_capacity(numeric_children.len() + candidates.len());
    for insertion_index in 0..=numeric_children.len() {
        children.extend(
            candidates
                .iter()
                .filter(|candidate| candidate.insertion_index.min(numeric_children.len()) == insertion_index)
                .map(|candidate| candidate.node),
        );
        if let Some(child) = numeric_children.get(insertion_index) {
            children.push(*child);
        }
    }

    for (order, child) in children.into_iter().enumerate() {
        let child_writing_mode = tree.get_writing_mode(child);
        let child_style = tree.get_flexbox_child_style(child);
        if child_style.box_generation_mode() == BoxGenerationMode::None || child_style.position() != Position::Absolute
        {
            continue;
        }
        let align_self = FlexboxItemStyle::align_self(&child_style)
            .unwrap_or(constants.align_items)
            .resolve_normal(AlignItems::STRETCH)
            .resolve_axis_relative(
                child_writing_mode,
                child_style.direction(),
                constants.writing_mode,
                constants.inline_direction,
                constants.dir.cross_axis(),
            );
        drop(child_style);

        let local_static_position = flex_static_position(constants, align_self);
        tree.set_out_of_flow_static_position(node, child, local_static_position);
        if !tree.is_out_of_flow_containing_block(node, child) {
            continue;
        }
        let containing_block = tree.get_out_of_flow_containing_block(node, child, containing_block);
        let static_position = tree
            .get_out_of_flow_static_position(
                node,
                child,
                containing_block.outer_size,
                containing_block.writing_direction,
            )
            .unwrap_or(local_static_position);
        if let Some(output) = layout_out_of_flow_item(
            tree,
            OutOfFlowItem { node: child, order: order as u32, static_position },
            containing_block,
        ) {
            content_size = content_size.f32_max(output.content_size);
        }
    }

    content_size
}

/// Computes the total space taken up by gaps in an axis given:
///   - The size of each gap
///   - The number of items (children or flex-lines) between which there are gaps
#[inline(always)]
fn sum_axis_gaps(gap: f32, num_items: usize) -> f32 {
    // Gaps only exist between items, so...
    if num_items <= 1 {
        // ...if there are less than 2 items then there are no gaps
        0.0
    } else {
        // ...otherwise there are (num_items - 1) gaps
        gap * (num_items - 1) as f32
    }
}
