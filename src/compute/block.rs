//! Computes the CSS block layout algorithm in the case that the block container being laid out contains only block-level boxes
use crate::geometry::{
    AbsoluteAxis, Line, LogicalBoxStrut, LogicalOffset, LogicalSize, Point, Rect, Size, WritingDirection,
};
use crate::style::{AvailableSpace, CoreStyle, LengthPercentageAuto, Overflow, Position};
use crate::style_helpers::TaffyMaxContent;
use crate::tree::{
    ChildLayoutInput, CollapsibleMarginSet, Layout, LayoutInput, LayoutOutput, RunMode, SizingMode, SizingPurpose,
};
use crate::tree::{LayoutPartialTree, LayoutPartialTreeExt, NodeId};
use crate::util::debug::debug_log;
use crate::util::sys::f32_max;
use crate::util::sys::Vec;
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{
    AutoSizeBehavior, BlockContainerStyle, BlockItemStyle, BoxGenerationMode, BoxSizing, Direction,
    LayoutBlockContainer, OrthogonalFallback, RequestedAxis, TextAlign, WritingMode,
};

use super::common::absolute::{
    fit_content_width, AbsoluteBlockSizeInput, AbsoluteBlockSizeResolver, AbsoluteBoxSizing,
    InsetModifiedContainingBlock,
};
use super::common::aspect_ratio::{
    resolve_formatting_context_size, resolve_size_constraints, FormattingContextSizeInput, ResolvedAxisConstraints,
    SizeConstraintInput, TransferredSizesMode,
};
use super::common::baseline::physical_baseline;
use super::common::intrinsic_size::{
    measure_intrinsic_axis, resolve_content_based_block_size_constraints, resolve_intrinsic_axis_constraints,
    resolve_intrinsic_width_constraints, resolve_node_size_constraints, resolve_ratio_dependent_intrinsic_sizing,
    AutomaticInlineSizeResolution, BlockSizeProperties, ContentBasedBlockSize, IntrinsicAxisInput,
    IntrinsicPreferredSize, IntrinsicWidthInput, NodeSizeConstraintInput, RatioDependentAutomaticMinimum,
    ResolvedNodeSizing,
};
use super::common::used_size::{stretch_border_box_available_space, StretchSizeProperties};

#[cfg(feature = "float_layout")]
use super::float::{BfcSlot, ContentSlot, FloatContext, FloatIntrinsicWidthCalculator};
#[cfg(feature = "float_layout")]
use crate::{Clear, Float, FloatDirection};

/// Context for positioning Block and Float boxes within a Block Formatting Context
pub struct BlockFormattingContext {
    /// The float positioning context that handles positioning floats within this Block Formatting Context
    #[cfg(feature = "float_layout")]
    float_context: FloatContext,
}

impl Default for BlockFormattingContext {
    fn default() -> Self {
        Self {
            #[cfg(feature = "float_layout")]
            float_context: FloatContext::new(),
        }
    }
}

impl BlockFormattingContext {
    /// Create an empty block formatting context.
    pub fn new() -> Self {
        Default::default()
    }

    /// Create an initial `BlockContext` for this `BlockFormattingContext`
    pub fn root_block_context(&mut self) -> BlockContext<'_> {
        BlockContext {
            bfc: self,
            block_offset: 0.0,
            line_insets: [0.0, 0.0],
            content_box_line_insets: [0.0, 0.0],
            float_block_size_contribution: 0.0,
            is_root: true,
            #[cfg(feature = "float_layout")]
            adjoining_floats: [false, false],
            #[cfg(feature = "float_layout")]
            block_start_adjoining_floats: None,
        }
    }
}

/// A direction-agnostic offset in a block formatting context.
///
/// Like Chromium's `BfcOffset`, the inline coordinate is measured from
/// line-left rather than inline-start. Keeping this distinct from
/// [`LogicalOffset`] prevents RTL conversion from leaking into float fitting.
#[cfg(feature = "float_layout")]
#[derive(Clone, Copy, Debug, Default)]
pub struct BfcOffset {
    /// Offset from the BFC's line-left edge.
    pub line_offset: f32,
    /// Offset from the BFC's block-start edge.
    pub block_offset: f32,
}

/// Convert logical inline-start/end values into BFC line-left/right values.
#[inline]
fn logical_line_to_bfc_sides<T: Copy>(line: Line<T>, direction: Direction) -> [T; 2] {
    if direction.is_rtl() {
        [line.end, line.start]
    } else {
        [line.start, line.end]
    }
}

/// Convert a BFC-relative line/block offset to a logical fragment offset.
#[cfg(feature = "float_layout")]
#[inline]
fn logical_from_bfc_offset(
    offset: BfcOffset,
    child_inline_size: f32,
    parent_inline_size: f32,
    direction: Direction,
) -> LogicalOffset<f32> {
    let inline_offset = if direction.is_rtl() {
        parent_inline_size - offset.line_offset - child_inline_size
    } else {
        offset.line_offset
    };
    LogicalOffset { inline_offset, block_offset: offset.block_offset }
}

/// Context for each individual Block within a Block Formatting Context
///
/// Contains a mutable reference to the BlockFormattingContext + block-specific data
pub struct BlockContext<'bfc> {
    /// Mutable access to the root block formatting context.
    bfc: &'bfc mut BlockFormattingContext,
    /// Block offset from this box's block-start border edge to the BFC root.
    block_offset: f32,
    /// Line-left/right border-box insets from the BFC root.
    ///
    /// A BFC is deliberately agnostic to text direction. Its inline coordinate
    /// is measured from line-left; callers convert to logical inline offsets at
    /// the formatting-context boundary.
    line_insets: [f32; 2],
    /// Line-left/right content-box insets from the BFC root.
    content_box_line_insets: [f32; 2],
    /// Block size occupied by descendant floats.
    float_block_size_contribution: f32,
    /// Whether the node is the root of the Block Formatting Context is belongs to.
    is_root: bool,
    /// Whether a float has been placed (on each side) whose position adjoins the current
    /// margin-collapse strut of this block (i.e. whose final position can still be moved by
    /// margins that collapse into that strut). Such floats force clearance on cleared elements
    /// whose margins adjoin the same strut.
    #[cfg(feature = "float_layout")]
    adjoining_floats: [bool; 2],
    /// The value of `adjoining_floats` frozen at the first point at which in-flow content was
    /// committed within this block (resolving its block-start margin strut).
    /// `None` if no in-flow content has been committed yet.
    #[cfg(feature = "float_layout")]
    block_start_adjoining_floats: Option<[bool; 2]>,
}

impl BlockContext<'_> {
    /// Create a sub-`BlockContext` for a child block node
    pub fn sub_context(&mut self, additional_block_offset: f32, line_insets: [f32; 2]) -> BlockContext<'_> {
        let line_insets = [self.line_insets[0] + line_insets[0], self.line_insets[1] + line_insets[1]];
        BlockContext {
            bfc: self.bfc,
            block_offset: self.block_offset + additional_block_offset,
            line_insets,
            content_box_line_insets: line_insets,
            float_block_size_contribution: 0.0,
            is_root: false,
            // Floats adjoining the parent's current strut also adjoin this
            // block's block-start strut while it collapses with its first child.
            #[cfg(feature = "float_layout")]
            adjoining_floats: self.adjoining_floats,
            #[cfg(feature = "float_layout")]
            block_start_adjoining_floats: None,
        }
    }

    /// Returns whether this block is the root block of it's Block Formatting Context
    pub fn is_bfc_root(&self) -> bool {
        self.is_root
    }
}

#[cfg(feature = "float_layout")]
impl BlockContext<'_> {
    /// Set the inline size of the overall block formatting context. Float
    /// placement uses this to resolve offsets from inline-end.
    ///
    /// Sub-blocks within a Block Formatting Context should use the `Self::sub_context` method to create
    /// a sub-`BlockContext` with line-left/right insets instead.
    pub fn set_inline_size(&mut self, available_inline_size: f32) {
        self.bfc.float_context.set_width(available_inline_size);
    }

    /// Set the line-left/right content-box insets (padding, border and scrollbar gutter).
    pub fn apply_content_box_line_inset(&mut self, content_box_line_insets: [f32; 2]) {
        self.content_box_line_insets[0] = self.line_insets[0] + content_box_line_insets[0];
        self.content_box_line_insets[1] = self.line_insets[1] + content_box_line_insets[1];
    }

    /// Whether the float context contains any floats
    #[inline(always)]
    pub fn has_floats(&self) -> bool {
        self.bfc.float_context.has_floats()
    }

    /// Whether the float context contains any floats that extend beyond `min_block_offset`.
    #[inline(always)]
    pub fn has_active_floats(&self, min_block_offset: f32) -> bool {
        self.bfc.float_context.has_active_floats(min_block_offset + self.block_offset)
    }

    /// Position a floated box, returning its direction-agnostic BFC offset.
    pub fn place_floated_box(
        &mut self,
        floated_box: LogicalSize<f32>,
        min_block_offset: f32,
        direction: FloatDirection,
        clear: Clear,
        adjoins_unresolved_strut: bool,
    ) -> BfcOffset {
        if adjoins_unresolved_strut {
            self.adjoining_floats[direction as usize] = true;
        }
        let mut pos = self.bfc.float_context.place_floated_box(
            Size { width: floated_box.inline_size, height: floated_box.block_size },
            min_block_offset + self.block_offset,
            self.content_box_line_insets,
            direction,
            clear,
        );
        pos.y -= self.block_offset;
        pos.x -= self.line_insets[0];

        self.float_block_size_contribution = self.float_block_size_contribution.max(pos.y + floated_box.block_size);

        BfcOffset { line_offset: pos.x, block_offset: pos.y }
    }

    /// Search for a BFC line/block-space suitable for non-floated content.
    pub fn find_content_slot(&self, min_block_offset: f32, clear: Clear, after: Option<usize>) -> ContentSlot {
        let mut slot = self.bfc.float_context.find_content_slot(
            min_block_offset + self.block_offset,
            self.content_box_line_insets,
            clear,
            after,
        );
        slot.y -= self.block_offset;
        slot.x -= self.line_insets[0];
        slot
    }

    /// Search for a BFC line/block-space suitable for a box that establishes
    /// an independent formatting context (whose border box must not overlap floats).
    pub fn find_bfc_slot(
        &self,
        min_block_offset: f32,
        margins: [f32; 2],
        direction: Direction,
        clear: Clear,
        after: Option<usize>,
    ) -> BfcSlot {
        let mut slot = self.bfc.float_context.find_bfc_slot(
            min_block_offset + self.block_offset,
            self.content_box_line_insets,
            margins,
            direction,
            clear,
            after,
        );
        slot.y -= self.block_offset;
        slot.x -= self.line_insets[0];
        slot
    }

    /// Get the bottom of lowest relevant float for the specific clear property
    pub fn cleared_threshold(&self, clear: Clear) -> Option<f32> {
        self.bfc.float_context.cleared_threshold(clear).map(|threshold| threshold - self.block_offset)
    }

    /// Whether a float that is adjoining the current margin-collapse strut has been placed
    /// on the side(s) relevant to the passed clear property
    pub fn has_adjoining_float(&self, clear: Clear) -> bool {
        match clear {
            Clear::Left => self.adjoining_floats[0],
            Clear::Right => self.adjoining_floats[1],
            Clear::Both => self.adjoining_floats[0] || self.adjoining_floats[1],
            Clear::None => false,
        }
    }

    /// Merge adjoining float flags propagated from a child block into this block's flags
    fn merge_adjoining_floats(&mut self, flags: [bool; 2]) {
        self.adjoining_floats[0] |= flags[0];
        self.adjoining_floats[1] |= flags[1];
    }

    /// Record that in-flow content has been committed within this block, resolving the position of
    /// the current margin-collapse strut. Floats placed before this point no longer adjoin the
    /// current strut. The flags for the block-start strut are frozen at the first commit.
    fn commit_strut(&mut self) {
        if self.block_start_adjoining_floats.is_none() {
            self.block_start_adjoining_floats = Some(self.adjoining_floats);
        }
        self.adjoining_floats = [false, false];
    }

    /// Floats placed while this block's block-start strut was unresolved.
    fn block_start_adjoining_floats(&self) -> [bool; 2] {
        self.block_start_adjoining_floats.unwrap_or(self.adjoining_floats)
    }

    /// Include the block-size contribution made by descendant floats.
    fn add_child_floated_block_size_contribution(&mut self, child_contribution: f32) {
        self.float_block_size_contribution = self.float_block_size_contribution.max(child_contribution);
    }

    /// Return the block size consumed by descendant floats.
    pub fn floated_block_size_contribution(&self) -> f32 {
        self.float_block_size_contribution
    }
}

#[cfg(not(feature = "float_layout"))]
impl BlockContext<'_> {
    #[inline(always)]
    /// Return the block size consumed by descendant floats (always zero when float layout is disabled).
    fn floated_block_size_contribution(&self) -> f32 {
        0.0
    }
}

use super::common::alignment::{apply_alignment_fallback, compute_alignment_offset};
#[cfg(feature = "content_size")]
use super::common::content_size::compute_content_size_contribution;

/// Per-child data that is accumulated and modified over the course of the layout algorithm
struct BlockItem {
    /// The identifier for the associated node
    node_id: NodeId,

    /// The "source order" of the item. This is the index of the item within the children iterator,
    /// and controls the order in which the nodes are placed
    order: u32,

    /// Items that are tables don't have stretch sizing applied to them
    is_table: bool,

    /// Replaced items resolve an automatic inline size to their intrinsic size
    /// rather than being stretch-sized.
    /// <https://www.w3.org/TR/CSS22/visudet.html#block-replaced-width>
    is_replaced: bool,

    /// Whether this item is laid out by the block formatting algorithm.
    ///
    /// Inline-block baseline propagation uses a block child's last baseline,
    /// while other formatting contexts contribute their first baseline.
    uses_block_layout: bool,

    /// Whether the child is a non-independent block or inline node
    is_in_same_bfc: bool,

    /// How this containing block resolves the child's authored automatic
    /// logical inline size.
    inline_auto_behavior: AutoSizeBehavior,

    #[cfg(feature = "float_layout")]
    /// The `float` style of the node
    float: Float,
    #[cfg(feature = "float_layout")]
    /// The `clear` style of the node
    clear: Clear,

    /// The base size of this item
    size: Size<Option<f32>>,
    /// The minimum allowable size of this item
    min_size: Size<Option<f32>>,
    /// The maximum allowable size of this item
    max_size: Size<Option<f32>>,
    /// Late min-intrinsic minimum for a ratio-derived logical inline size.
    inline_automatic_minimum: Option<RatioDependentAutomaticMinimum>,
    /// Authored and ratio-transferred constraints for the logical block axis.
    ///
    /// The content-based automatic minimum is capped only by an authored
    /// maximum, so the used `min_size`/`max_size` pair is not sufficient here.
    block_axis_constraints: ResolvedAxisConstraints,
    /// The overflow style of the item
    overflow: Point<Overflow>,
    /// The total physical space occupied by the item's scrollbar gutters
    scrollbar_size: Size<f32>,

    /// The position style of the item
    position: Position,
    /// The final offset of this item
    inset: Rect<LengthPercentageAuto>,
    /// The margin of this item
    margin: Rect<LengthPercentageAuto>,
    /// Resolved physical padding of this item.
    padding: Rect<f32>,
    /// Resolved physical border widths of this item.
    border: Rect<f32>,
    /// The sum of padding and border for this item
    padding_border_sum: Size<f32>,

    /// The computed static position in the parent's flow-relative coordinate
    /// space, before relative or absolute insets are applied.
    static_position: LogicalOffset<f32>,
    /// Whether margins can be collapsed through this item
    can_be_collapsed_through: bool,

    /// Whether this item's intrinsic inline contribution depends on the
    /// containing block's block-size.
    depends_on_block_constraints: bool,

    /// Pending layout for an in-flow item. Its physical size and box edges are
    /// known, but its physical top-left cannot be materialized until the
    /// containing block's final size is known.
    pending_layout: Option<PendingBlockLayout>,
}

/// A child fragment waiting at the logical/physical layout boundary.
struct PendingBlockLayout {
    /// Physical fragment data other than its final top-left point.
    layout: Layout,
    /// Flow-relative border-box offset in the parent formatting context.
    logical_offset: LogicalOffset<f32>,
    /// Whether this normal-flow fragment belongs to the align-content subject.
    participates_in_align_content: bool,
}

/// Computes the layout of [`LayoutPartialTree`] according to the block layout algorithm
pub fn compute_block_layout(
    tree: &mut impl LayoutBlockContainer,
    node_id: NodeId,
    inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    let writing_mode = tree.get_writing_mode(node_id);
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { run_mode, .. } = inputs;
    let resolved_aspect_ratio = tree.get_resolved_aspect_ratio(node_id);
    let size_containment = tree.get_size_containment(node_id);
    let scrollbar_insets = tree.get_scrollbar_insets(node_id);
    let is_scroll_container_for_automatic_minimum = tree.is_scroll_container_for_automatic_minimum(node_id);
    let style = tree.get_block_container_style(node_id);

    // Pull these out earlier to avoid borrowing issues
    let overflow = style.overflow();
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let aspect_ratio = if inputs.sizing_mode == SizingMode::InherentSize { resolved_aspect_ratio } else { None };
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding_border_size = (padding + border).sum_axes();
    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    let raw_size = style.size();
    let raw_min_size = style.min_size();
    let raw_max_size = style.max_size();
    let margin = style.margin().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let logical_raw_size = writing_mode.to_logical(raw_size);
    let logical_raw_min_size = writing_mode.to_logical(raw_min_size);
    let logical_raw_max_size = writing_mode.to_logical(raw_max_size);
    let available_block_space = writing_mode.to_logical(inputs.available_space.maybe_sub(margin.sum_axes())).block_size;
    let contained_outer_size =
        size_containment.resolve_outer_size(Size::ZERO, padding_border_size + scrollbar_insets.sum_axes());
    let content_based_block_size = ContentBasedBlockSize::new(
        BlockSizeProperties::new(
            logical_raw_size.block_size,
            logical_raw_min_size.block_size,
            logical_raw_max_size.block_size,
        ),
        aspect_ratio,
        padding_border_size,
        inputs.block_auto_behavior,
        available_block_space,
        is_scroll_container_for_automatic_minimum,
        false,
    )
    .with_intrinsic_border_box_override(writing_mode.to_logical(contained_outer_size).block_size);
    let apply_available_intrinsic_floor =
        content_based_block_size.depends_on_available_block_space() && inputs.axis.contains(writing_mode.block_axis());

    drop(style);

    let mut node_sizing = resolve_node_size_constraints(
        tree,
        node_id,
        inputs,
        NodeSizeConstraintInput {
            raw_size,
            raw_min_size,
            raw_max_size,
            box_sizing_adjustment,
            padding_border_size,
            aspect_ratio,
            contained_outer_size,
            automatic_inline_size_resolution: AutomaticInlineSizeResolution::FitContent,
        },
    );
    let block_axis_constraints = node_sizing.constraints.block_axis_constraints(writing_mode);
    content_based_block_size.apply_initial_block_geometry(
        writing_mode,
        writing_mode.to_logical(inputs.known_dimensions).block_size,
        block_axis_constraints,
        &mut node_sizing,
    );
    let applied_aspect_ratio = run_mode == RunMode::ComputeSize && node_sizing.applied_aspect_ratio;
    let node_outer_size = node_sizing.outer_size;

    // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
    // is ComputeSize (and thus the container's size is all that we're interested in)
    if run_mode == RunMode::ComputeSize && !apply_available_intrinsic_floor {
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

    // Unwrap the block formatting context if one was passed, or else create a new one
    debug_log!("BLOCK");
    let output = match block_ctx {
        Some(inherited_bfc) if !is_scroll_container => {
            compute_inner(tree, node_id, inputs, node_sizing, inherited_bfc, content_based_block_size)
        }
        _ => {
            let mut root_bfc = BlockFormattingContext::new();
            let mut root_ctx = root_bfc.root_block_context();
            compute_inner(tree, node_id, inputs, node_sizing, &mut root_ctx, content_based_block_size)
        }
    };
    output
        .with_block_constraint_dependency(
            node_sizing.depends_on_block_constraints || content_based_block_size.depends_on_available_block_space(),
        )
        .with_applied_aspect_ratio(applied_aspect_ratio)
}

/// Computes the layout of [`LayoutBlockContainer`] according to the block layout algorithm
fn compute_inner(
    tree: &mut impl LayoutBlockContainer,
    node_id: NodeId,
    inputs: LayoutInput,
    node_sizing: ResolvedNodeSizing,
    #[allow(unused_mut)] mut block_ctx: &mut BlockContext<'_>,
    content_based_block_size: ContentBasedBlockSize,
) -> LayoutOutput {
    let writing_mode = tree.get_writing_mode(node_id);
    let apply_available_intrinsic_floor =
        content_based_block_size.depends_on_available_block_space() && inputs.axis.contains(writing_mode.block_axis());
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { available_space, run_mode, vertical_margins_are_collapsible, .. } = inputs;

    let scrollbar_gutter = tree.get_scrollbar_insets(node_id);
    let style = tree.get_block_container_style(node_id);
    let raw_margin = style.margin();
    let margin = raw_margin.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let direction = style.direction();
    let writing_direction = WritingDirection::new(writing_mode, direction);

    let padding_border = padding + border;
    let content_box_inset = padding_border + scrollbar_gutter;
    let logical_padding = writing_direction.to_logical_box_strut(padding);
    let logical_border = writing_direction.to_logical_box_strut(border);
    let logical_scrollbar_gutter = writing_direction.to_logical_box_strut(scrollbar_gutter);
    let logical_padding_border = logical_padding + logical_border;
    let logical_padding_border_size = logical_padding_border.sum_axes();
    let logical_content_box_inset = writing_direction.to_logical_box_strut(content_box_inset);

    // Apply content box inset
    #[cfg(feature = "float_layout")]
    block_ctx.apply_content_box_line_inset(logical_line_to_bfc_sides(
        Line { start: logical_content_box_inset.inline_start, end: logical_content_box_inset.inline_end },
        direction,
    ));

    let outer_logical_size = writing_mode.to_logical(node_sizing.outer_size);
    let definite_logical_size = writing_mode.to_logical(node_sizing.definite_size);
    let size_logical = writing_mode.to_logical(node_sizing.preferred_size);
    let min_size_logical = writing_mode.to_logical(node_sizing.min_size);
    let max_size_logical = writing_mode.to_logical(node_sizing.max_size);
    let container_content_box_size = LogicalSize {
        inline_size: outer_logical_size.inline_size.maybe_sub(logical_content_box_inset.inline_axis_sum()),
        block_size: outer_logical_size.block_size.maybe_sub(logical_content_box_inset.block_axis_sum()),
    };

    let overflow = style.overflow();
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();

    // Determine margin collapsing behaviour
    let own_margins_collapse_with_children = Line {
        start: vertical_margins_are_collapsible.start
            && !is_scroll_container
            && style.position() == Position::Relative
            && logical_padding.block_start == 0.0
            && logical_border.block_start == 0.0,
        end: vertical_margins_are_collapsible.end
            && !is_scroll_container
            && style.position() == Position::Relative
            && logical_padding.block_end == 0.0
            && logical_border.block_end == 0.0
            && size_logical.block_size.is_none(),
    };
    let has_styles_preventing_being_collapsed_through = !style.is_block()
        || block_ctx.is_bfc_root()
        || is_scroll_container
        || style.position() == Position::Absolute
        || logical_padding.block_start > 0.0
        || logical_padding.block_end > 0.0
        || logical_border.block_start > 0.0
        || logical_border.block_end > 0.0
        || matches!(size_logical.block_size, Some(size) if size > 0.0)
        || matches!(min_size_logical.block_size, Some(size) if size > 0.0);

    let text_align = style.text_align();
    let align_content = style.align_content();
    drop(style);

    // A child constraint space describes the margin-box opportunity. This
    // formatting context owns its margins, so its content sizing operates on
    // the remaining border-box opportunity.
    let available_logical_space = writing_mode.to_logical(available_space.maybe_sub(margin.sum_axes()));

    // Explicit block-axis stretch uses the containing block's margin-box
    // opportunity. Margins at an unseparated block edge are omitted only when
    // the child participates in this BFC's margin-collapsing flow. Carry the
    // mask through the child constraint space instead of changing normal
    // margin placement.
    let ignored_margins_for_stretch = if block_ctx.is_bfc_root() {
        Rect::default()
    } else {
        writing_direction.to_physical_box_strut(LogicalBoxStrut {
            inline_start: false,
            inline_end: false,
            block_start: logical_padding_border.block_start == 0.0,
            block_end: logical_padding_border.block_end == 0.0,
        })
    };

    // 1. Generate items
    let mut items = generate_item_list(
        tree,
        node_id,
        writing_direction,
        container_content_box_size,
        available_logical_space,
        ignored_margins_for_stretch,
    );

    // 2. Compute the container inline size. Block layout stretches and stacks
    // in flow-relative axes; width is only the inline axis in horizontal-tb.
    let (container_outer_inline_size, content_inline_size_depends_on_block_constraints) =
        match outer_logical_size.inline_size {
            Some(inline_size) => (inline_size, false),
            None => {
                let available_inline_size =
                    available_logical_space.inline_size.maybe_sub(logical_content_box_inset.inline_axis_sum());
                let (intrinsic_inline_size, depends) = determine_content_based_container_inline_size(
                    tree,
                    &mut items,
                    available_inline_size,
                    writing_direction,
                    container_content_box_size,
                );
                (
                    (intrinsic_inline_size + logical_content_box_inset.inline_axis_sum())
                        .maybe_clamp(min_size_logical.inline_size, max_size_logical.inline_size)
                        .maybe_max(Some(logical_padding_border_size.inline_size)),
                    depends,
                )
            }
        };

    // Short-circuit if computing size and both logical dimensions are known.
    if !apply_available_intrinsic_floor {
        if let (RunMode::ComputeSize, Some(container_outer_block_size)) = (run_mode, outer_logical_size.block_size) {
            let outer_size = writing_mode.to_physical(LogicalSize {
                inline_size: container_outer_inline_size,
                block_size: container_outer_block_size,
            });
            return LayoutOutput::from_outer_size(outer_size)
                .with_block_constraint_dependency(content_inline_size_depends_on_block_constraints);
        }
    }

    // We can also short-circuit when only the physical axis corresponding to
    // this formatting context's logical inline axis was requested.
    if run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::from(writing_mode.inline_axis()) {
        let outer_size =
            writing_mode.to_physical(LogicalSize { inline_size: container_outer_inline_size, block_size: 0.0 });
        return LayoutOutput::from_outer_size(outer_size)
            .with_block_constraint_dependency(content_inline_size_depends_on_block_constraints);
    }

    let container_percentage_resolution_block_size = definite_logical_size
        .block_size
        .or(outer_logical_size.block_size)
        .or(size_logical.block_size.maybe_max(min_size_logical.block_size))
        .or(min_size_logical.block_size);
    // Relative block-axis percentage insets only resolve against a definite
    // containing-block block size. A minimum may determine the eventual used
    // size, but it does not make an otherwise-auto block size definite.
    let relative_inset_percentage_resolution_block_size = definite_logical_size.block_size.or(size_logical.block_size);

    // 3. Perform final item layout and return the content block size.
    #[cfg_attr(not(feature = "content_size"), allow(unused_mut))]
    let (
        mut inflow_content_size,
        mut intrinsic_outer_block_size,
        first_child_block_start_margin_set,
        last_child_block_end_margin_set,
        mut first_baseline,
        mut last_baseline,
    ) = perform_final_layout_on_in_flow_children(
        tree,
        &mut items,
        BlockContainerLayoutContext {
            run_mode,
            outer_inline_size: container_outer_inline_size,
            available_block_size: container_content_box_size.block_size,
            percentage_resolution_block_size: container_percentage_resolution_block_size,
            relative_inset_percentage_resolution_block_size,
            content_box_inset: logical_content_box_inset,
            border: logical_border,
            scrollbar_inset: logical_scrollbar_gutter,
            text_align,
            writing_direction,
            ignored_margins_for_stretch,
            own_margins_collapse_with_children,
        },
        block_ctx,
    );

    // Root BFCs contain floats
    #[cfg(feature = "float_layout")]
    if block_ctx.is_bfc_root() || is_scroll_container {
        intrinsic_outer_block_size = intrinsic_outer_block_size.max(block_ctx.floated_block_size_contribution());
    }

    let container_outer_block_size = if apply_available_intrinsic_floor {
        let block_size_constraints = content_based_block_size
            .resolve(writing_mode, Some(container_outer_inline_size), intrinsic_outer_block_size)
            .resolve_against(size_logical.block_size, node_sizing.constraints.block_axis_constraints(writing_mode));
        let candidate_block_size =
            outer_logical_size.block_size.or(block_size_constraints.preferred).unwrap_or(intrinsic_outer_block_size);
        if writing_mode.to_logical(inputs.known_dimensions).block_size.is_some() {
            candidate_block_size
        } else {
            candidate_block_size.maybe_clamp(block_size_constraints.min, block_size_constraints.max)
        }
    } else {
        outer_logical_size
            .block_size
            .unwrap_or(intrinsic_outer_block_size.maybe_clamp(min_size_logical.block_size, max_size_logical.block_size))
    }
    .max(logical_padding_border_size.block_size);
    let final_logical_size =
        LogicalSize { inline_size: container_outer_inline_size, block_size: container_outer_block_size };
    let final_outer_size = writing_mode.to_physical(final_logical_size);

    // CSS2 §8.3.1: a block-end margin collapses with the last in-flow
    // child's block-end margin only while the automatic block size is not
    // being held open by its minimum.
    let block_size_constrained_by_minimum =
        matches!(min_size_logical.block_size, Some(size) if size > 0.0 && size >= container_outer_block_size);
    let own_block_end_margin_collapses_with_children =
        own_margins_collapse_with_children.end && !block_size_constrained_by_minimum;

    // Apply `align-content` to in-flow items if requested. Pending fragments
    // remain logical until this group offset and the final outer size are known.
    //
    // For block layout the entire stack of in-flow children is treated as a single alignment
    // subject. That means distribution keywords (`space-between`, `space-around`,
    // `space-evenly`, `stretch`) must invoke the single-subject fallback unconditionally —
    // which is what passing `num_items = 1` to `apply_alignment_fallback` does. The whole
    // group then shifts by one offset, with zero inter-item gap.
    if let Some(align_content) = align_content {
        let container_inner_block_size = container_outer_block_size - logical_content_box_inset.block_axis_sum();
        let inflow_content_block_size = intrinsic_outer_block_size - logical_content_box_inset.block_axis_sum();
        let free_space = container_inner_block_size - inflow_content_block_size;
        let any_in_flow = items
            .iter()
            .any(|item| item.pending_layout.as_ref().is_some_and(|pending| pending.participates_in_align_content));
        if any_in_flow {
            let keyword = apply_alignment_fallback(free_space, 1, align_content);
            let group_offset = compute_alignment_offset(free_space, 1, 0.0, keyword, false, true);
            first_baseline = first_baseline.map(|baseline| baseline + group_offset);
            last_baseline = last_baseline.map(|baseline| baseline + group_offset);
            for item in items.iter_mut() {
                if let Some(pending) = item.pending_layout.as_mut() {
                    if pending.participates_in_align_content {
                        pending.logical_offset.block_offset += group_offset;
                    }
                }
            }

            #[cfg(feature = "content_size")]
            {
                inflow_content_size = LogicalSize::ZERO;
                for item in items.iter() {
                    if let Some(pending) = item.pending_layout.as_ref() {
                        if !pending.participates_in_align_content {
                            continue;
                        }
                        let logical_size = writing_mode.to_logical(pending.layout.size);
                        let logical_content_size = writing_mode.to_logical(pending.layout.content_size);
                        let contribution_location = LogicalOffset {
                            inline_offset: pending.logical_offset.inline_offset
                                - logical_border.inline_start
                                - logical_scrollbar_gutter.inline_start,
                            block_offset: pending.logical_offset.block_offset
                                - logical_border.block_start
                                - logical_scrollbar_gutter.block_start,
                        };
                        inflow_content_size = inflow_content_size.f32_max(compute_logical_content_size_contribution(
                            contribution_location,
                            logical_size,
                            logical_content_size,
                            logical_overflow(item.overflow, writing_mode),
                        ));
                    }
                }
            }
        }
    }

    // Determine whether this node can be collapsed through
    let all_in_flow_children_can_be_collapsed_through = items.iter().all(|item| {
        #[cfg(feature = "float_layout")]
        if item.float.is_floated() {
            return true;
        }
        item.position == Position::Absolute || item.can_be_collapsed_through
    });
    let can_be_collapsed_through =
        !has_styles_preventing_being_collapsed_through && all_in_flow_children_can_be_collapsed_through;

    let mut output = LayoutOutput::from_sizes_and_baseline_sets(
        final_outer_size,
        Size::ZERO,
        physical_baseline(first_baseline, final_outer_size, writing_direction),
        physical_baseline(last_baseline, final_outer_size, writing_direction),
    )
    .with_block_constraint_dependency(
        content_inline_size_depends_on_block_constraints || items.iter().any(|item| item.depends_on_block_constraints),
    );
    let raw_logical_margin = writing_direction.to_logical_box_strut(raw_margin);
    output.top_margin = if own_margins_collapse_with_children.start {
        first_child_block_start_margin_set
    } else {
        let margin =
            raw_logical_margin.block_start.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        CollapsibleMarginSet::from_margin(margin)
    };
    output.bottom_margin = if own_block_end_margin_collapses_with_children {
        last_child_block_end_margin_set
    } else {
        let margin = raw_logical_margin.block_end.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        CollapsibleMarginSet::from_margin(margin)
    };
    output.margins_can_collapse_through = can_be_collapsed_through;

    // Short-circuit if computing size.
    //
    // The legacy `LayoutOutput` field names are physical, but block layout
    // carries block-start/end collapsing struts through them. Parent block
    // containers need those struts even during size-only passes.
    if run_mode == RunMode::ComputeSize {
        return output;
    }

    // Materialize flow-relative offsets at the physical fragment boundary.
    let converter = writing_direction.converter(final_outer_size);
    for item in items.iter() {
        if let Some(pending) = item.pending_layout.as_ref() {
            let mut layout = pending.layout;
            layout.location = converter.to_physical_point(pending.logical_offset, layout.size);
            tree.set_unrounded_layout(item.node_id, &layout);
        }
    }

    // 4. Layout absolutely positioned children
    let absolute_position_inset = border + scrollbar_gutter;
    let absolute_position_area = final_outer_size - absolute_position_inset.sum_axes();
    let absolute_position_offset = Point { x: absolute_position_inset.left, y: absolute_position_inset.top };
    let absolute_content_size = perform_absolute_layout_on_absolute_children(
        tree,
        &items,
        absolute_position_area,
        absolute_position_offset,
        writing_direction,
        final_outer_size,
    );

    #[cfg(feature = "content_size")]
    {
        // The container's own padding at the end of the content is part of its scrollable
        // overflow region, so it is included in the in-flow content size.
        inflow_content_size.inline_size += logical_padding.inline_end;
        inflow_content_size.block_size += logical_padding.block_end;
        let absolute_logical_size = writing_mode.to_logical(absolute_content_size);
        let logical_content_size = inflow_content_size.f32_max(absolute_logical_size);
        output.content_size = writing_mode.to_physical(logical_content_size);
    }

    // 5. Perform hidden layout on hidden children
    let len = tree.child_count(node_id);
    for order in 0..len {
        let child = tree.get_child_id(node_id, order);
        let child_style = tree.get_block_child_style(child);
        if child_style.box_generation_mode() == BoxGenerationMode::None {
            drop(child_style);
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

    output
}

/// Create a `Vec` of `BlockItem` structs where each item in the `Vec` represents a child of the current node
#[inline]
fn generate_item_list(
    tree: &mut impl LayoutBlockContainer,
    node: NodeId,
    writing_direction: WritingDirection,
    node_inner_size: LogicalSize<Option<f32>>,
    available_space: LogicalSize<AvailableSpace>,
    ignored_margins_for_stretch: Rect<bool>,
) -> Vec<BlockItem> {
    let writing_mode = writing_direction.mode;
    let physical_node_inner_size = writing_mode.to_physical(node_inner_size);
    let child_ids: Vec<_> = tree.child_ids(node).collect();
    child_ids
        .into_iter()
        .filter_map(|child_node_id| {
            let aspect_ratio = tree.get_resolved_aspect_ratio(child_node_id);
            let child_writing_mode = tree.get_writing_mode(child_node_id);
            let scrollbar_size = tree.get_scrollbar_insets(child_node_id).sum_axes();
            let child_style = tree.get_block_child_style(child_node_id);
            if child_style.box_generation_mode() == BoxGenerationMode::None {
                return None;
            }
            // When the container's inline size depends on its contents, CSS
            // Sizing resolves cyclic percentage padding/border and min-width
            // contributions against zero. Preferred and max sizes remain
            // unresolved until final layout, so keep their original
            // containing-block basis here.
            let mut logical_contribution_parent_size = node_inner_size;
            logical_contribution_parent_size.inline_size = logical_contribution_parent_size.inline_size.or(Some(0.0));
            let contribution_parent_size = writing_mode.to_physical(logical_contribution_parent_size);
            let contribution_inline_size = logical_contribution_parent_size.inline_size;
            let padding =
                child_style.padding().resolve_or_zero(contribution_inline_size, |val, basis| tree.calc(val, basis));
            let border =
                child_style.border().resolve_or_zero(contribution_inline_size, |val, basis| tree.calc(val, basis));
            let pb_sum = (padding + border).sum_axes();
            let box_sizing = child_style.box_sizing();
            let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
            let raw_size = child_style.size();
            let raw_min_size = child_style.min_size();
            let raw_max_size = child_style.max_size();
            let raw_logical_size = writing_mode.to_logical(raw_size);
            let raw_logical_min_size = writing_mode.to_logical(raw_min_size);
            let raw_logical_max_size = writing_mode.to_logical(raw_max_size);
            let child_block_size_depends_on_parent =
                [raw_logical_size.block_size, raw_logical_min_size.block_size, raw_logical_max_size.block_size]
                    .into_iter()
                    .any(|value| value.may_have_percentage_dependence() || value.is_stretch());
            let mut depends_on_block_constraints = child_block_size_depends_on_parent && aspect_ratio.is_some();
            let mut size = raw_size
                .maybe_resolve(physical_node_inner_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment);
            let mut min_size = raw_min_size
                .maybe_resolve(contribution_parent_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment);
            let mut max_size = raw_max_size
                .maybe_resolve(physical_node_inner_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment);
            let position = child_style.position();
            let overflow = child_style.overflow();
            let inset = child_style.inset();
            let margin = child_style.margin();

            #[cfg(feature = "float_layout")]
            let float = child_style.float();
            #[cfg(feature = "float_layout")]
            let clear = child_style.clear();
            #[cfg(feature = "float_layout")]
            let is_not_floated = float == Float::None;

            #[cfg(not(feature = "float_layout"))]
            let is_not_floated = true;

            let is_block = child_style.is_block();
            let uses_block_layout = child_style.uses_block_layout();
            let is_table = child_style.is_table();
            let is_replaced = child_style.is_compressible_replaced();
            let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
            let inline_auto_behavior = if position != Position::Absolute
                && is_not_floated
                && !is_table
                && !is_replaced
                && !child_writing_mode.is_orthogonal_to(writing_mode)
            {
                AutoSizeBehavior::StretchImplicit
            } else {
                AutoSizeBehavior::FitContent
            };

            // A writing-mode root establishes an independent formatting
            // context. This includes parallel modes with opposite block flow
            // (for example vertical-rl inside vertical-lr), not only
            // orthogonal axes.
            let is_in_same_bfc: bool = is_block
                && !is_table
                && position != Position::Absolute
                && is_not_floated
                && !is_scroll_container
                && child_writing_mode == writing_mode;

            drop(child_style);

            let mut preferred_inline_from_intrinsic_ratio = false;

            // Absolutely positioned boxes derive their available inline space
            // from their insets. Resolve those intrinsic keywords later, in
            // the absolute-layout seam, rather than against the whole parent.
            if position != Position::Absolute {
                let resolved_margin =
                    margin.resolve_or_zero(contribution_inline_size, |val, basis| tree.calc(val, basis));
                let logical_margin = writing_direction.to_logical_box_strut(resolved_margin);
                let child_available_logical_space = LogicalSize {
                    inline_size: node_inner_size
                        .inline_size
                        .map(AvailableSpace::Definite)
                        .unwrap_or(available_space.inline_size),
                    // An external block constraint does not make an automatic
                    // containing block definite. Only this container's known
                    // content-box size can resolve a child's block stretch.
                    block_size: node_inner_size
                        .block_size
                        .map(AvailableSpace::Definite)
                        .unwrap_or(AvailableSpace::MaxContent),
                };
                let available_inline_size =
                    child_available_logical_space.inline_size.maybe_sub(logical_margin.inline_axis_sum());
                let child_available_space = writing_mode.to_physical(child_available_logical_space);
                let item_ignored_margins = if is_in_same_bfc { ignored_margins_for_stretch } else { Rect::default() };
                let stretch = StretchSizeProperties::new(raw_size, raw_min_size, raw_max_size).resolve(
                    stretch_border_box_available_space(child_available_space, resolved_margin, item_ignored_margins),
                    pb_sum,
                );
                size = size.or(stretch.preferred);
                min_size = min_size.or(stretch.min);
                max_size = max_size.or(stretch.max);
                let intrinsic_inputs = ChildLayoutInput::new(
                    Size::NONE,
                    physical_node_inner_size,
                    writing_mode,
                    child_available_space,
                    SizingMode::ContentSize,
                    Line::TRUE,
                )
                .with_ignored_margins_for_stretch(item_ignored_margins)
                .with_block_auto_behavior(AutoSizeBehavior::FitContent);
                let ratio_dependent_sizing = resolve_ratio_dependent_intrinsic_sizing(
                    size,
                    min_size,
                    max_size,
                    aspect_ratio,
                    pb_sum,
                    writing_mode.inline_axis(),
                    child_block_size_depends_on_parent && aspect_ratio.is_some(),
                );
                let intrinsic = resolve_intrinsic_axis_constraints(
                    tree,
                    child_node_id,
                    intrinsic_inputs,
                    IntrinsicAxisInput {
                        preferred: IntrinsicPreferredSize::Authored(raw_logical_size.inline_size),
                        min: raw_logical_min_size.inline_size,
                        max: raw_logical_max_size.inline_size,
                        available_space: available_inline_size,
                        axis: writing_mode.inline_axis(),
                        ratio_dependent_sizing,
                    },
                );
                preferred_inline_from_intrinsic_ratio = intrinsic.preferred.applied_aspect_ratio
                    && writing_mode.inline_axis() == child_writing_mode.inline_axis();
                let mut logical_size = writing_mode.to_logical(size);
                let mut logical_min_size = writing_mode.to_logical(min_size);
                let mut logical_max_size = writing_mode.to_logical(max_size);
                logical_size.inline_size = logical_size.inline_size.or(intrinsic.preferred.value);
                logical_min_size.inline_size = logical_min_size.inline_size.or(intrinsic.min.value);
                logical_max_size.inline_size = logical_max_size.inline_size.or(intrinsic.max.value);
                size = writing_mode.to_physical(logical_size);
                min_size = writing_mode.to_physical(logical_min_size);
                max_size = writing_mode.to_physical(logical_max_size);
                depends_on_block_constraints |= intrinsic.depends_on_block_constraints();
            }

            let resolved = resolve_size_constraints(SizeConstraintInput {
                size,
                min_size,
                max_size,
                size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
                writing_mode: child_writing_mode,
                inline_auto_behavior,
                block_auto_behavior: AutoSizeBehavior::FitContent,
                transferred_sizes_mode: TransferredSizesMode::Normal,
                aspect_ratio,
                padding_border: pb_sum,
            });
            let block_axis_constraints = resolved.block_axis_constraints(child_writing_mode);
            let inline_automatic_minimum = RatioDependentAutomaticMinimum::new(
                resolved.inline_axis_constraints(child_writing_mode),
                resolved.aspect_ratio_applied.get_abs(child_writing_mode.inline_axis())
                    || preferred_inline_from_intrinsic_ratio,
                child_writing_mode.to_logical(raw_min_size).inline_size,
                is_scroll_container,
                is_replaced,
            );
            size = resolved.size;
            min_size = resolved.min_size;
            max_size = resolved.max_size;

            Some(BlockItem {
                node_id: child_node_id,
                order: 0,
                is_table,
                is_replaced,
                uses_block_layout,
                is_in_same_bfc,
                inline_auto_behavior,
                #[cfg(feature = "float_layout")]
                float,
                #[cfg(feature = "float_layout")]
                clear,
                size,
                min_size,
                max_size,
                inline_automatic_minimum,
                block_axis_constraints,
                overflow,
                scrollbar_size,
                position,
                inset,
                margin,
                padding,
                border,
                padding_border_sum: pb_sum,

                // Fields to be computed later (for now we initialise with dummy values)
                static_position: LogicalOffset::ZERO,
                can_be_collapsed_through: false,
                depends_on_block_constraints,
                pending_layout: None,
            })
        })
        .enumerate()
        .map(|(order, mut item)| {
            item.order = order as u32;
            item
        })
        .collect()
}

/// Compute the content-based inline size when the container inline size is not known.
#[inline]
fn determine_content_based_container_inline_size(
    tree: &mut impl LayoutPartialTree,
    items: &mut [BlockItem],
    available_inline_size: AvailableSpace,
    parent_writing_direction: WritingDirection,
    parent_inner_size: LogicalSize<Option<f32>>,
) -> (f32, bool) {
    let parent_writing_mode = parent_writing_direction.mode;
    let parent_size = parent_writing_mode.to_physical(parent_inner_size);

    let mut max_child_inline_size = 0.0;
    #[cfg(feature = "float_layout")]
    let mut float_contribution = FloatIntrinsicWidthCalculator::new(available_inline_size);
    let mut depends_on_block_constraints = false;
    for item in items.iter_mut().filter(|item| item.position != Position::Absolute) {
        // The containing block's inline size depends on this contribution, so
        // cyclic percentage margins resolve against zero rather than the
        // external available-space constraint.
        let logical_margin = parent_writing_direction
            .to_logical_box_strut(item.margin.resolve_or_zero(Some(0.0), |val, basis| tree.calc(val, basis)));
        let item_inline_margin_sum = logical_margin.inline_axis_sum();
        let contribution_available_space = parent_writing_mode
            .to_physical(LogicalSize { inline_size: available_inline_size, block_size: AvailableSpace::MinContent });
        let known_dimensions = item.size.maybe_clamp(item.min_size, item.max_size);
        let known_dimensions = apply_ratio_dependent_inline_automatic_minimum(
            tree,
            item,
            known_dimensions,
            ChildLayoutInput::new(
                known_dimensions,
                parent_size,
                parent_writing_mode,
                contribution_available_space,
                SizingMode::ContentSize,
                Line::TRUE,
            )
            .with_inline_auto_behavior(item.inline_auto_behavior)
            .with_block_auto_behavior(AutoSizeBehavior::FitContent),
        );
        let known_logical_size = parent_writing_mode.to_logical(known_dimensions);
        let min_logical_size = parent_writing_mode.to_logical(item.min_size);
        let max_logical_size = parent_writing_mode.to_logical(item.max_size);
        let inline_size = match known_logical_size.inline_size {
            Some(inline_size) => inline_size,
            None => {
                let measured = tree.measure_child_size_with_metadata(
                    item.node_id,
                    ChildLayoutInput::new(
                        known_dimensions,
                        parent_size,
                        parent_writing_mode,
                        contribution_available_space,
                        SizingMode::InherentSize,
                        Line::TRUE,
                    ),
                    RequestedAxis::from(parent_writing_mode.inline_axis()),
                );
                item.depends_on_block_constraints |= measured.depends_on_block_constraints;
                parent_writing_mode.to_logical(measured.size).inline_size
            }
        }
        .maybe_clamp(min_logical_size.inline_size, max_logical_size.inline_size);
        depends_on_block_constraints |= item.depends_on_block_constraints;

        let padding_border_inline_size = parent_writing_mode.to_logical(item.padding_border_sum).inline_size;
        let inline_size = f32_max(inline_size, padding_border_inline_size) + item_inline_margin_sum;

        #[cfg(feature = "float_layout")]
        if let Some(direction) = item.float.float_direction() {
            float_contribution.add_float(inline_size, direction, item.clear);
            continue;
        }

        max_child_inline_size = f32_max(max_child_inline_size, inline_size);
    }

    #[cfg(feature = "float_layout")]
    {
        max_child_inline_size = max_child_inline_size.max(float_contribution.result());
    }

    (max_child_inline_size, depends_on_block_constraints)
}

/// Resolve an item's preferred/min/max sizes against the containing block's
/// final percentage basis.
///
/// Item generation may run while that basis is indefinite in order to compute
/// the container's intrinsic inline size. Numeric percentages and intrinsic
/// keywords are therefore materialized again here, after the final basis and
/// margin-adjusted available inline size are known. Reusing the contribution
/// resolver here prevents a raw opposite-axis size from bypassing intrinsic
/// keyword and aspect-ratio ordering during final layout.
fn resolve_block_item_final_style(
    tree: &mut impl LayoutBlockContainer,
    item: &mut BlockItem,
    parent_size: Size<Option<f32>>,
    parent_writing_mode: WritingMode,
    available_space: Size<AvailableSpace>,
    ignored_margins_for_stretch: Rect<bool>,
) {
    let percentage_basis = parent_writing_mode.to_logical(parent_size).inline_size;
    let aspect_ratio = tree.get_resolved_aspect_ratio(item.node_id);
    let writing_mode = tree.get_writing_mode(item.node_id);
    let (raw_size, raw_min_size, raw_max_size, mut size, mut min_size, mut max_size, padding, border, overflow) = {
        let style = tree.get_block_child_style(item.node_id);
        let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let padding_border_sum = (padding + border).sum_axes();
        let box_sizing = style.box_sizing();
        let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };
        let raw_size = style.size();
        let raw_min_size = style.min_size();
        let raw_max_size = style.max_size();
        (
            raw_size,
            raw_min_size,
            raw_max_size,
            raw_size.maybe_resolve(parent_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment),
            raw_min_size
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            raw_max_size
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            padding,
            border,
            style.overflow(),
        )
    };
    let padding_border_sum = (padding + border).sum_axes();
    let resolved_margin = item.margin.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let margin_axis_sums =
        Size { width: resolved_margin.horizontal_axis_sum(), height: resolved_margin.vertical_axis_sum() };
    let stretch = StretchSizeProperties::new(raw_size, raw_min_size, raw_max_size).resolve(
        stretch_border_box_available_space(available_space, resolved_margin, ignored_margins_for_stretch),
        padding_border_sum,
    );
    size = size.or(stretch.preferred);
    min_size = min_size.or(stretch.min);
    max_size = max_size.or(stretch.max);
    let containing_inline_space = percentage_basis.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MaxContent);
    let available_inline_size =
        containing_inline_space.maybe_sub(margin_axis_sums.get_abs(parent_writing_mode.inline_axis()));
    let child_available_space = available_space;
    let raw_logical_size = parent_writing_mode.to_logical(raw_size);
    let raw_logical_min_size = parent_writing_mode.to_logical(raw_min_size);
    let raw_logical_max_size = parent_writing_mode.to_logical(raw_max_size);
    let opposite_axis_depends_on_parent =
        [raw_logical_size.block_size, raw_logical_min_size.block_size, raw_logical_max_size.block_size]
            .into_iter()
            .any(|value| value.may_have_percentage_dependence() || value.is_stretch());
    let ratio_dependent_sizing = resolve_ratio_dependent_intrinsic_sizing(
        size,
        min_size,
        max_size,
        aspect_ratio,
        padding_border_sum,
        parent_writing_mode.inline_axis(),
        opposite_axis_depends_on_parent && aspect_ratio.is_some(),
    );
    let intrinsic = resolve_intrinsic_axis_constraints(
        tree,
        item.node_id,
        ChildLayoutInput::new(
            Size::NONE,
            parent_size,
            parent_writing_mode,
            child_available_space,
            SizingMode::ContentSize,
            Line::TRUE,
        )
        .with_inline_auto_behavior(item.inline_auto_behavior)
        .with_block_auto_behavior(AutoSizeBehavior::FitContent)
        .with_ignored_margins_for_stretch(ignored_margins_for_stretch),
        IntrinsicAxisInput {
            preferred: IntrinsicPreferredSize::Authored(raw_logical_size.inline_size),
            min: raw_logical_min_size.inline_size,
            max: raw_logical_max_size.inline_size,
            available_space: available_inline_size,
            axis: parent_writing_mode.inline_axis(),
            ratio_dependent_sizing,
        },
    );
    let mut logical_size = parent_writing_mode.to_logical(size);
    let mut logical_min_size = parent_writing_mode.to_logical(min_size);
    let mut logical_max_size = parent_writing_mode.to_logical(max_size);
    logical_size.inline_size = logical_size.inline_size.or(intrinsic.preferred.value);
    logical_min_size.inline_size = logical_min_size.inline_size.or(intrinsic.min.value);
    logical_max_size.inline_size = logical_max_size.inline_size.or(intrinsic.max.value);
    size = parent_writing_mode.to_physical(logical_size);
    min_size = parent_writing_mode.to_physical(logical_min_size);
    max_size = parent_writing_mode.to_physical(logical_max_size);
    item.depends_on_block_constraints |= intrinsic.depends_on_block_constraints();

    let resolved = resolve_size_constraints(SizeConstraintInput {
        size,
        min_size,
        max_size,
        size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
        writing_mode,
        inline_auto_behavior: item.inline_auto_behavior,
        block_auto_behavior: AutoSizeBehavior::FitContent,
        transferred_sizes_mode: TransferredSizesMode::Normal,
        aspect_ratio,
        padding_border: padding_border_sum,
    });
    let intrinsic_ratio_supplied_child_inline =
        intrinsic.preferred.applied_aspect_ratio && parent_writing_mode.inline_axis() == writing_mode.inline_axis();
    let inline_automatic_minimum = RatioDependentAutomaticMinimum::new(
        resolved.inline_axis_constraints(writing_mode),
        resolved.aspect_ratio_applied.get_abs(writing_mode.inline_axis()) || intrinsic_ratio_supplied_child_inline,
        writing_mode.to_logical(raw_min_size).inline_size,
        overflow.x.is_scroll_container() || overflow.y.is_scroll_container(),
        item.is_replaced,
    );

    item.size = resolved.size.or(item.size);
    item.min_size = resolved.min_size.or(item.min_size);
    item.max_size = resolved.max_size.or(item.max_size);
    item.inline_automatic_minimum = inline_automatic_minimum;
    item.block_axis_constraints = resolved.block_axis_constraints(writing_mode);
    item.padding = padding;
    item.border = border;
    item.padding_border_sum = padding_border_sum;
}

/// Merge the ratio-dependent min-intrinsic inline size at the child sizing
/// boundary where the ratio-derived preferred size is still provisional.
///
/// This operation is shared by intrinsic contribution and final layout. It
/// updates the item's used constraints as well as the immediate known size so
/// a content-sized ancestor observes the same minimum as the final fragment.
fn apply_ratio_dependent_inline_automatic_minimum(
    tree: &mut impl LayoutPartialTree,
    item: &mut BlockItem,
    known_dimensions: Size<Option<f32>>,
    child_input: ChildLayoutInput,
) -> Size<Option<f32>> {
    let Some(automatic_minimum) = item.inline_automatic_minimum else {
        return known_dimensions;
    };

    let writing_mode = tree.get_writing_mode(item.node_id);
    let inline_axis = writing_mode.inline_axis();
    let intrinsic = measure_intrinsic_axis(
        tree,
        item.node_id,
        ChildLayoutInput { known_dimensions, ..child_input },
        AvailableSpace::MinContent,
        inline_axis,
    );
    item.depends_on_block_constraints |= intrinsic.depends_on_block_constraints;
    let (used_minimum, used_maximum) = automatic_minimum.resolve(intrinsic.size.get_abs(inline_axis));

    let mut logical_min_size = writing_mode.to_logical(item.min_size);
    let mut logical_max_size = writing_mode.to_logical(item.max_size);
    logical_min_size.inline_size = used_minimum;
    logical_max_size.inline_size = used_maximum;
    item.min_size = writing_mode.to_physical(logical_min_size);
    item.max_size = writing_mode.to_physical(logical_max_size);

    let mut logical_known_dimensions = writing_mode.to_logical(known_dimensions);
    logical_known_dimensions.inline_size = logical_known_dimensions.inline_size.maybe_clamp(used_minimum, used_maximum);
    writing_mode.to_physical(logical_known_dimensions)
}

/// Complete an in-flow block item's logical block size once its inline
/// constraint is known.
///
/// A preferred ratio may provide a provisional auto block size, but the
/// ratio-dependent automatic minimum still comes from the real content
/// contribution. Resolve both at this formatting-context boundary before the
/// result is labeled as an exact child `known_dimensions` input.
fn resolve_block_item_known_dimensions(
    tree: &mut impl LayoutBlockContainer,
    item: &mut BlockItem,
    child_input: ChildLayoutInput,
) -> Size<Option<f32>> {
    let mut known_dimensions = child_input.known_dimensions;
    let child_writing_mode = tree.get_writing_mode(item.node_id);
    let aspect_ratio = tree.get_resolved_aspect_ratio(item.node_id);
    let is_scroll_container_for_automatic_minimum = tree.is_scroll_container_for_automatic_minimum(item.node_id);
    let properties = {
        let style = tree.get_block_child_style(item.node_id);
        let size = child_writing_mode.to_logical(style.size());
        let min_size = child_writing_mode.to_logical(style.min_size());
        let max_size = child_writing_mode.to_logical(style.max_size());
        BlockSizeProperties::new(size.block_size, min_size.block_size, max_size.block_size)
    };
    let resolver = ContentBasedBlockSize::new(
        properties,
        aspect_ratio,
        item.padding_border_sum,
        child_input.block_auto_behavior,
        child_writing_mode.to_logical(child_input.available_space).block_size,
        is_scroll_container_for_automatic_minimum,
        item.is_replaced,
    );
    let auto_size_is_content_based = child_input.block_auto_behavior.is_content_based(aspect_ratio.is_some());
    if item.inline_automatic_minimum.is_none() && !resolver.requires_intrinsic_measurement() {
        return known_dimensions;
    }

    let child_input =
        |known_dimensions| ChildLayoutInput { known_dimensions, definite_dimensions: known_dimensions, ..child_input };

    known_dimensions =
        apply_ratio_dependent_inline_automatic_minimum(tree, item, known_dimensions, child_input(known_dimensions));

    if !resolver.requires_intrinsic_measurement() {
        return known_dimensions;
    }

    let mut measurement_dimensions = child_writing_mode.to_logical(known_dimensions);
    if properties.preferred_is_content_based(auto_size_is_content_based) {
        measurement_dimensions.block_size = None;
    }
    let measurement_dimensions = child_writing_mode.to_physical(measurement_dimensions);
    let intrinsic =
        resolve_content_based_block_size_constraints(tree, item.node_id, child_input(measurement_dimensions), resolver);
    item.depends_on_block_constraints |= intrinsic.depends_on_block_constraints;

    let logical_size = child_writing_mode.to_logical(item.size);
    let resolved = intrinsic.resolve_against(logical_size.block_size, item.block_axis_constraints);
    let minimum_border_box_size = child_writing_mode.to_logical(item.padding_border_sum).block_size;
    let mut used_size = child_writing_mode.to_logical(measurement_dimensions);
    used_size.block_size = used_size
        .block_size
        .or(resolved.preferred)
        .maybe_clamp(resolved.min, resolved.max)
        .maybe_max(Some(minimum_border_box_size));
    child_writing_mode.to_physical(used_size)
}

/// Immutable container state shared while positioning in-flow block children.
#[derive(Clone, Copy, Debug)]
struct BlockContainerLayoutContext {
    /// Whether this pass measures the box or commits final fragments.
    run_mode: RunMode,
    /// Used logical border-box inline size of the container.
    outer_inline_size: f32,
    /// Known logical content-box block size offered to in-flow children.
    /// `None` preserves content-based fallback for an automatic container.
    available_block_size: Option<f32>,
    /// Definite logical block size available for descendant percentages.
    percentage_resolution_block_size: Option<f32>,
    /// Definite logical block size available for relative percentage insets.
    relative_inset_percentage_resolution_block_size: Option<f32>,
    /// Padding, border, and scrollbar inset around the content box.
    content_box_inset: LogicalBoxStrut<f32>,
    /// Used logical border widths.
    border: LogicalBoxStrut<f32>,
    /// Physical scrollbar gutters projected into the container's logical axes.
    scrollbar_inset: LogicalBoxStrut<f32>,
    /// Inline alignment inherited by anonymous block content.
    text_align: TextAlign,
    /// Writing mode and inline direction that own this formatting context.
    writing_direction: WritingDirection,
    /// Physical margin sides ignored by explicit stretch for children that
    /// participate in this block formatting context.
    ignored_margins_for_stretch: Rect<bool>,
    /// Whether block-start/end margins may collapse with children.
    own_margins_collapse_with_children: Line<bool>,
}

/// A child fragment's baseline set viewed from its block container.
///
/// Baseline sets are tied to a writing mode, not just a physical axis. A
/// fragment in a different writing mode therefore cannot export its stored
/// first/last baselines into this block container. Keeping the compatibility
/// check inside the projection object also prevents block-end baseline
/// synthesis from bypassing the same rule.
#[derive(Clone, Copy, Debug)]
struct BlockChildBaselineProjection {
    /// Writing direction that owns the destination baseline set.
    container_writing_direction: WritingDirection,
    /// Writing mode that owns the child fragment's stored baseline set.
    child_writing_mode: WritingMode,
    /// Physical child border-box size used to reverse block-axis offsets.
    child_size: Size<f32>,
}

impl BlockChildBaselineProjection {
    /// Bind a child fragment's baseline data to one container projection.
    #[inline(always)]
    const fn new(
        container_writing_direction: WritingDirection,
        child_writing_mode: WritingMode,
        child_size: Size<f32>,
    ) -> Self {
        Self { container_writing_direction, child_writing_mode, child_size }
    }

    /// Whether the child exposes a baseline set in the container's writing
    /// mode. Parallel modes with opposite block flow are distinct sets.
    #[inline(always)]
    fn has_compatible_baseline_set(self) -> bool {
        self.child_writing_mode == self.container_writing_direction.mode
    }

    /// Project a compatible physical fragment baseline into the container's
    /// logical block axis.
    #[inline(always)]
    fn project(self, baseline: Point<Option<f32>>) -> Option<f32> {
        if !self.has_compatible_baseline_set() {
            return None;
        }

        if self.container_writing_direction.mode.is_horizontal() {
            baseline.y
        } else {
            baseline.x.map(|offset| {
                if self.container_writing_direction.is_block_flow_reversed() {
                    self.child_size.width - offset
                } else {
                    offset
                }
            })
        }
    }
}

#[cfg(feature = "content_size")]
/// Project physical overflow axes into the current formatting context.
fn logical_overflow(overflow: Point<Overflow>, writing_mode: WritingMode) -> LogicalSize<Overflow> {
    writing_mode.to_logical(Size { width: overflow.x, height: overflow.y })
}

#[cfg(feature = "content_size")]
/// Compute one scrollable-overflow contribution without leaving logical axes.
fn compute_logical_content_size_contribution(
    location: LogicalOffset<f32>,
    size: LogicalSize<f32>,
    content_size: LogicalSize<f32>,
    overflow: LogicalSize<Overflow>,
) -> LogicalSize<f32> {
    let result = compute_content_size_contribution(
        Point { x: location.inline_offset, y: location.block_offset },
        Size { width: size.inline_size, height: size.block_size },
        Size { width: content_size.inline_size, height: content_size.block_size },
        Point { x: overflow.inline_size, y: overflow.block_size },
    );
    LogicalSize { inline_size: result.width, block_size: result.height }
}

/// Compute each child's final size and position.
#[inline]
fn perform_final_layout_on_in_flow_children(
    tree: &mut impl LayoutBlockContainer,
    items: &mut [BlockItem],
    context: BlockContainerLayoutContext,
    block_ctx: &mut BlockContext<'_>,
) -> (LogicalSize<f32>, f32, CollapsibleMarginSet, CollapsibleMarginSet, Option<f32>, Option<f32>) {
    let BlockContainerLayoutContext {
        run_mode,
        outer_inline_size: container_outer_inline_size,
        available_block_size: container_available_block_size,
        percentage_resolution_block_size: container_percentage_resolution_block_size,
        relative_inset_percentage_resolution_block_size,
        content_box_inset,
        border,
        scrollbar_inset,
        text_align,
        writing_direction,
        ignored_margins_for_stretch,
        own_margins_collapse_with_children,
    } = context;
    let writing_mode = writing_direction.mode;
    let direction = writing_direction.direction;
    let container_inner_inline_size = container_outer_inline_size - content_box_inset.inline_axis_sum();
    let child_constraint_block_size =
        container_available_block_size.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MaxContent);
    let container_percentage_resolution_block_size =
        container_percentage_resolution_block_size.maybe_sub(content_box_inset.block_axis_sum());
    let parent_logical_size = LogicalSize {
        inline_size: Some(container_inner_inline_size),
        block_size: container_percentage_resolution_block_size,
    };
    let parent_size = writing_mode.to_physical(parent_logical_size);
    let margin_percentage_basis = parent_logical_size.inline_size.unwrap_or(0.0);
    let relative_inset_parent_size = writing_mode.to_physical(LogicalSize {
        inline_size: Some(container_inner_inline_size),
        block_size: relative_inset_percentage_resolution_block_size.maybe_sub(content_box_inset.block_axis_sum()),
    });
    #[cfg(feature = "float_layout")]
    if block_ctx.is_bfc_root() {
        block_ctx.set_inline_size(container_outer_inline_size);
    }

    // Once the block-start margin stops collapsing with children, its strut is
    // resolved and floats adjoining ancestor struts no longer adjoin it.
    #[cfg(feature = "float_layout")]
    if !own_margins_collapse_with_children.start {
        block_ctx.commit_strut();
    }

    #[cfg_attr(not(feature = "content_size"), allow(unused_mut))]
    let mut inflow_content_size = LogicalSize::ZERO;
    let mut committed_block_offset = content_box_inset.block_start;
    let mut block_offset_for_absolute = content_box_inset.block_start;
    let mut first_child_block_start_margin_set = CollapsibleMarginSet::ZERO;
    let mut active_collapsible_margin_set = CollapsibleMarginSet::ZERO;
    let mut is_collapsing_with_first_margin_set = true;
    let mut first_baseline: Option<f32> = None;
    let mut last_baseline: Option<f32> = None;
    // Whether the active margin set contains the margins of a self-collapsing element with
    // clearance. Such margins collapse with the margins of following siblings but the resulting
    // margin does not collapse with the block-end margin of the parent.
    let mut active_margin_set_has_clearance = false;

    #[cfg(feature = "float_layout")]
    let mut has_active_floats = block_ctx.has_active_floats(committed_block_offset);
    #[cfg(not(feature = "float_layout"))]
    let has_active_floats = false;

    for item in items.iter_mut() {
        if item.position == Position::Absolute {
            item.static_position = LogicalOffset {
                inline_offset: content_box_inset.inline_start,
                block_offset: block_offset_for_absolute,
            };
        } else {
            let item_ignored_margins_for_stretch =
                if item.is_in_same_bfc { ignored_margins_for_stretch } else { Rect::default() };
            let container_child_available_space = writing_mode.to_physical(LogicalSize {
                inline_size: AvailableSpace::Definite(container_inner_inline_size),
                block_size: child_constraint_block_size,
            });
            resolve_block_item_final_style(
                tree,
                item,
                parent_size,
                writing_mode,
                container_child_available_space,
                item_ignored_margins_for_stretch,
            );
            let item_margin =
                writing_direction.to_logical_box_strut(item.margin.map(|margin| {
                    margin.resolve_to_option(margin_percentage_basis, |val, basis| tree.calc(val, basis))
                }));
            let item_non_auto_margin = item_margin.map(|m| m.unwrap_or(0.0));
            let item_non_auto_inline_margin_sum = item_non_auto_margin.inline_axis_sum();

            let scrollbar_size = item.scrollbar_size;

            // Handle floated boxes
            #[cfg(feature = "float_layout")]
            if let Some(float_direction) = item.float.float_direction() {
                has_active_floats = true;

                // Child constraint spaces include margins, matching Blink's
                // ConstraintSpace contract. The child sizing boundary removes
                // them once while the block parent keeps the margin box for
                // float placement.
                let child_constraint_inline_size = container_inner_inline_size.max(0.0);
                let child_available_space = writing_mode.to_physical(LogicalSize {
                    inline_size: AvailableSpace::Definite(child_constraint_inline_size),
                    block_size: child_constraint_block_size,
                });
                let known_dimensions = item.size.maybe_clamp(item.min_size, item.max_size);
                let known_dimensions = resolve_block_item_known_dimensions(
                    tree,
                    item,
                    ChildLayoutInput::new(
                        known_dimensions,
                        parent_size,
                        writing_mode,
                        child_available_space,
                        SizingMode::ContentSize,
                        Line::FALSE,
                    )
                    .with_inline_auto_behavior(item.inline_auto_behavior)
                    .with_block_auto_behavior(AutoSizeBehavior::FitContent)
                    .with_ignored_margins_for_stretch(item_ignored_margins_for_stretch),
                );
                let item_layout = tree.perform_child_layout(
                    item.node_id,
                    ChildLayoutInput::new(
                        known_dimensions,
                        parent_size,
                        writing_mode,
                        child_available_space,
                        SizingMode::InherentSize,
                        // A float establishes a new block formatting context: its margins do not
                        // collapse with the margins of its children
                        Line::FALSE,
                    )
                    .with_ignored_margins_for_stretch(item_ignored_margins_for_stretch),
                );
                let logical_item_size = writing_mode.to_logical(item_layout.size);
                let margin_box = logical_item_size + item_non_auto_margin.sum_axes();

                // Floats that occur between collapsing margins are positioned as if they had an otherwise
                // empty anonymous block parent taking part in the flow, so the pending collapsible margins
                // contribute to the float's minimum block offset (unless those margins collapse with the
                // container's own block-start margin, in which case they are applied outside the container).
                //
                // In the latter case the position of the float is not fully resolved: margins contributed
                // by later siblings can still collapse into the strut and move the container (and float).
                // Such floats force clearance on cleared elements whose margins adjoin the same strut.
                let adjoins_unresolved_strut =
                    is_collapsing_with_first_margin_set && own_margins_collapse_with_children.start;
                let block_offset_for_float = if adjoins_unresolved_strut {
                    committed_block_offset
                } else {
                    committed_block_offset + active_collapsible_margin_set.resolve()
                };

                let bfc_location = block_ctx.place_floated_box(
                    margin_box,
                    block_offset_for_float,
                    float_direction,
                    item.clear,
                    adjoins_unresolved_strut,
                );
                let mut logical_location = logical_from_bfc_offset(
                    bfc_location,
                    margin_box.inline_size,
                    container_outer_inline_size,
                    direction,
                );

                // Convert the margin-box location returned by float placement into a border-box location
                // for the output Layout
                logical_location.block_offset += item_non_auto_margin.block_start;
                logical_location.inline_offset += item_non_auto_margin.inline_start;

                let resolved_margin = writing_direction.to_physical_box_strut(item_non_auto_margin);
                item.pending_layout = Some(PendingBlockLayout {
                    layout: Layout {
                        order: item.order,
                        size: item_layout.size,
                        #[cfg(feature = "content_size")]
                        content_size: item_layout.content_size,
                        scrollbar_size,
                        location: Point::ZERO,
                        padding: item.padding,
                        border: item.border,
                        margin: resolved_margin,
                    },
                    logical_offset: logical_location,
                    participates_in_align_content: false,
                });

                #[cfg(feature = "content_size")]
                {
                    // TODO: Should content size of floated boxes count as "inflow_content_size"
                    // or should it be counted separately?
                    let contribution_location = LogicalOffset {
                        inline_offset: logical_location.inline_offset
                            - border.inline_start
                            - scrollbar_inset.inline_start,
                        block_offset: logical_location.block_offset - border.block_start - scrollbar_inset.block_start,
                    };
                    let logical_content_size = writing_mode.to_logical(item_layout.content_size);
                    inflow_content_size = inflow_content_size.f32_max(compute_logical_content_size_contribution(
                        contribution_location,
                        logical_item_size,
                        logical_content_size,
                        logical_overflow(item.overflow, writing_mode),
                    ));
                }

                continue;
            }

            // Handle non-floated boxes

            let mut block_margin_offset: f32 = 0.0;
            #[cfg(feature = "float_layout")]
            let mut item_avoids_floats = false;
            #[cfg(feature = "float_layout")]
            let mut item_moved_past_float = false;

            let (stretch_inline_size, float_avoiding_position) = if item.is_in_same_bfc {
                let stretch_inline_size = container_inner_inline_size - item_non_auto_inline_margin_sum;
                let position = LogicalOffset::ZERO;

                (stretch_inline_size, position)
            } else {
                'block: {
                    // Set block margin offset for a different-BFC child.
                    if !is_collapsing_with_first_margin_set || !own_margins_collapse_with_children.start {
                        block_margin_offset = active_collapsible_margin_set
                            .collapse_with_margin(item_non_auto_margin.block_start)
                            .resolve();
                    };
                    let min_block_offset = committed_block_offset + block_margin_offset;

                    // In addition to the running flag, check the float context directly:
                    // floats placed by the subtree of a preceding in-flow sibling (in the same
                    // BFC) are not reflected in the flag
                    #[cfg(feature = "float_layout")]
                    if has_active_floats || block_ctx.has_active_floats(min_block_offset) {
                        let line_margins = logical_line_to_bfc_sides(
                            Line { start: item_non_auto_margin.inline_start, end: item_non_auto_margin.inline_end },
                            direction,
                        );
                        // An automatic inline size resolves to at least the negation of
                        // its margin sum, keeping the margin box non-negative.
                        let min_auto_inline_size = -item_non_auto_inline_margin_sum;

                        // Find the earliest slot at or beyond the minimum block
                        // offset with enough inline space for the border box.
                        let mut slot_segment = None;
                        let slot = loop {
                            let slot = block_ctx.find_bfc_slot(
                                min_block_offset,
                                line_margins,
                                direction,
                                item.clear,
                                slot_segment,
                            );
                            let Some(segment_id) = slot.segment_id else { break slot };
                            let item_size = writing_mode.to_logical(item.size);
                            let min_size = writing_mode.to_logical(item.min_size);
                            let max_size = writing_mode.to_logical(item.max_size);
                            let inline_size = item_size
                                .inline_size
                                .unwrap_or(slot.stretch_width.max(min_auto_inline_size))
                                .maybe_clamp(min_size.inline_size, max_size.inline_size);
                            if inline_size <= slot.border_width + 0.001 {
                                break slot;
                            }
                            slot_segment = Some(segment_id);
                        };

                        // Moving in the block direction to avoid a float
                        // separates the item's block-start margin from the
                        // parent's collapsing strut.
                        if slot.y > min_block_offset {
                            item_moved_past_float = true;
                        }

                        has_active_floats = slot.segment_id.is_some();
                        item_avoids_floats = true;
                        let stretch_inline_size = slot.stretch_width.max(min_auto_inline_size);
                        break 'block (
                            stretch_inline_size,
                            logical_from_bfc_offset(
                                BfcOffset { line_offset: slot.x, block_offset: slot.y },
                                slot.border_width,
                                container_outer_inline_size,
                                direction,
                            ),
                        );
                    }

                    if !has_active_floats {
                        let stretch_inline_size = container_inner_inline_size - item_non_auto_inline_margin_sum;
                        break 'block (
                            stretch_inline_size,
                            LogicalOffset {
                                inline_offset: content_box_inset.inline_start,
                                block_offset: min_block_offset,
                            },
                        );
                    }

                    unreachable!("One of the above cases will always be hit");
                }
            };

            // A block formatting context owns the used inline size of a child
            // that participates in normal block stretch. Fit-content children
            // (including orthogonal, floated, table, and replaced boxes) keep
            // that axis unresolved and consume the same value only as
            // available space in their own formatting algorithm. This is the
            // important distinction for orthogonal fallback constraints: a
            // 600px fallback may cap wrapping, but it must not become a fixed
            // 600px child size.
            let known_dimensions = if item.inline_auto_behavior == AutoSizeBehavior::FitContent {
                item.size.maybe_clamp(item.min_size, item.max_size)
            } else {
                let mut logical_size = writing_mode.to_logical(item.size);
                let logical_min_size = writing_mode.to_logical(item.min_size);
                let logical_max_size = writing_mode.to_logical(item.max_size);
                logical_size.inline_size = Some(
                    logical_size
                        .inline_size
                        .unwrap_or(stretch_inline_size)
                        .maybe_clamp(logical_min_size.inline_size, logical_max_size.inline_size),
                );
                writing_mode.to_physical(logical_size).maybe_clamp(item.min_size, item.max_size)
            };

            let child_constraint_inline_size = (stretch_inline_size + item_non_auto_inline_margin_sum).max(0.0);
            let child_available_space = writing_mode.to_physical(LogicalSize {
                inline_size: AvailableSpace::Definite(child_constraint_inline_size),
                block_size: child_constraint_block_size,
            });
            let known_dimensions = resolve_block_item_known_dimensions(
                tree,
                item,
                ChildLayoutInput::new(
                    known_dimensions,
                    parent_size,
                    writing_mode,
                    child_available_space,
                    SizingMode::ContentSize,
                    if item.is_in_same_bfc { Line::TRUE } else { Line::FALSE },
                )
                .with_inline_auto_behavior(item.inline_auto_behavior)
                .with_block_auto_behavior(AutoSizeBehavior::FitContent)
                .with_ignored_margins_for_stretch(item_ignored_margins_for_stretch),
            );
            let inputs = LayoutInput {
                run_mode,
                sizing_mode: SizingMode::InherentSize,
                sizing_purpose: SizingPurpose::Layout,
                axis: RequestedAxis::Both,
                inline_auto_behavior: item.inline_auto_behavior,
                block_auto_behavior: crate::AutoSizeBehavior::FitContent,
                orthogonal_fallback: OrthogonalFallback::UseInitialContainingBlock,
                known_dimensions,
                definite_dimensions: known_dimensions,
                parent_size,
                parent_writing_mode: writing_mode,
                available_space: child_available_space,
                ignored_margins_for_stretch: item_ignored_margins_for_stretch,
                vertical_margins_are_collapsible: if item.is_in_same_bfc { Line::TRUE } else { Line::FALSE },
            };

            #[cfg(feature = "float_layout")]
            let clear_threshold = block_ctx.cleared_threshold(item.clear);
            #[cfg(feature = "float_layout")]
            let clear_pos = clear_threshold.unwrap_or(f32::NEG_INFINITY);
            #[cfg(not(feature = "float_layout"))]
            let clear_pos = f32::NEG_INFINITY;

            let item_layout = if item.is_in_same_bfc {
                // Replaced elements may not have a known inline size; their
                // measure function sizes them instead of stretch sizing.
                let inline_size = writing_mode.to_logical(known_dimensions).inline_size.unwrap_or(stretch_inline_size);

                // TODO: account for auto margins
                let inset_start = item_non_auto_margin.inline_start + content_box_inset.inline_start;
                let inset_end = container_outer_inline_size - inline_size - inset_start;
                let line_insets = logical_line_to_bfc_sides(Line { start: inset_start, end: inset_end }, direction);

                // Compute child layout
                let mut child_block_ctx = block_ctx.sub_context(
                    (block_offset_for_absolute + item_non_auto_margin.block_start).max(clear_pos),
                    line_insets,
                );
                let output = tree.compute_block_child_layout(item.node_id, inputs, Some(&mut child_block_ctx));

                // Extract float contribution from child block context
                #[cfg(feature = "float_layout")]
                {
                    let child_contribution = child_block_ctx.floated_block_size_contribution();
                    let child_block_start_adjoining_floats = child_block_ctx.block_start_adjoining_floats();
                    block_ctx.add_child_floated_block_size_contribution(block_offset_for_absolute + child_contribution);
                    // Floats placed while the child's block-start strut was unresolved
                    // also adjoin this block's current strut
                    block_ctx.merge_adjoining_floats(child_block_start_adjoining_floats);
                }

                output
            } else {
                tree.compute_child_layout(item.node_id, inputs)
            };
            item.depends_on_block_constraints |= item_layout.block_constraint_dependency();
            let final_size = item_layout.size;
            let final_logical_size = writing_mode.to_logical(final_size);

            let block_start_margin_set =
                item_layout.top_margin.collapse_with_margin(item_margin.block_start.unwrap_or(0.0));
            let block_end_margin_set =
                item_layout.bottom_margin.collapse_with_margin(item_margin.block_end.unwrap_or(0.0));

            // Expand auto margins to fill available space
            // Note: Vertical auto-margins for relatively positioned block items simply resolve to 0.
            // See: https://www.w3.org/TR/CSS21/visudet.html#abs-non-replaced-width
            let free_inline_space = f32_max(0.0, stretch_inline_size - final_logical_size.inline_size);
            let inline_axis_auto_margin_size = {
                let auto_margin_count =
                    item_margin.inline_start.is_none() as u8 + item_margin.inline_end.is_none() as u8;
                if auto_margin_count > 0 {
                    free_inline_space / auto_margin_count as f32
                } else {
                    0.0
                }
            };
            let resolved_logical_margin = LogicalBoxStrut {
                inline_start: item_margin.inline_start.unwrap_or(inline_axis_auto_margin_size),
                inline_end: item_margin.inline_end.unwrap_or(inline_axis_auto_margin_size),
                block_start: block_start_margin_set.resolve(),
                block_end: block_end_margin_set.resolve(),
            };
            let resolved_margin = writing_direction.to_physical_box_strut(resolved_logical_margin);

            // Resolve item inset
            let inset = item
                .inset
                .zip_size(relative_inset_parent_size, |p, s| p.maybe_resolve(s, |val, basis| tree.calc(val, basis)));
            let logical_inset = writing_direction.to_logical_box_strut(inset);
            let inset_offset = LogicalOffset {
                inline_offset: logical_inset
                    .inline_start
                    .or(logical_inset.inline_end.map(|offset| -offset))
                    .unwrap_or(0.0),
                block_offset: logical_inset
                    .block_start
                    .or(logical_inset.block_end.map(|offset| -offset))
                    .unwrap_or(0.0),
            };

            // Resolve the active block-axis margin strut for a same-BFC child.
            if item.is_in_same_bfc
                && (!is_collapsing_with_first_margin_set || !own_margins_collapse_with_children.start)
            {
                block_margin_offset = active_collapsible_margin_set.collapse_with_set(block_start_margin_set).resolve()
            };

            // Compute clearance (CSS2.2 9.5.2). Clearance is introduced when
            // the hypothetical block-start border edge is before the
            // block-end of a relevant float.
            #[cfg(feature = "float_layout")]
            let mut has_clearance = false;
            #[cfg(not(feature = "float_layout"))]
            let has_clearance = false;
            #[cfg(feature = "float_layout")]
            if item.is_in_same_bfc {
                if let Some(threshold) = clear_threshold {
                    // The hypothetical position always includes the item's collapsed block-start margin set, even
                    // when those margins collapse with the container's own block-start margin (and are thus applied
                    // outside the container): in that case they still move the container (and hence the item)
                    // relative to the floats.
                    let hypothetical_block_offset = committed_block_offset
                        + active_collapsible_margin_set.collapse_with_set(block_start_margin_set).resolve();
                    // Clearance is forced (regardless of the hypothetical position) if a relevant float is
                    // adjoining the strut that the item's block-start margin would collapse into:
                    // if the margins were allowed to collapse they would pull the float down with the item,
                    // so clearance separates the two at the float's block-end.
                    let forced_clearance = block_ctx.has_adjoining_float(item.clear);
                    if forced_clearance || hypothetical_block_offset < threshold {
                        has_clearance = true;
                        // Clearance stops the item's block-start margin from collapsing with preceding margins.
                        // If those margins escape through the container's block-start, subtract that escaped
                        // strut from the local cleared position.
                        let escaped_margin =
                            if is_collapsing_with_first_margin_set && own_margins_collapse_with_children.start {
                                active_collapsible_margin_set.resolve()
                            } else {
                                0.0
                            };
                        block_margin_offset = threshold - committed_block_offset - escaped_margin;
                    }
                }
            }

            item.can_be_collapsed_through = item_layout.margins_can_collapse_through && !has_clearance;
            item.static_position = if item.is_in_same_bfc {
                let uncleared_block_offset = committed_block_offset + active_collapsible_margin_set.resolve();
                LogicalOffset {
                    inline_offset: content_box_inset.inline_start,
                    block_offset: uncleared_block_offset.max(clear_pos),
                }
            } else {
                float_avoiding_position
            };
            let mut logical_location = if item.is_in_same_bfc {
                LogicalOffset {
                    inline_offset: content_box_inset.inline_start
                        + inset_offset.inline_offset
                        + resolved_logical_margin.inline_start,
                    block_offset: committed_block_offset + block_margin_offset + inset_offset.block_offset,
                }
            } else {
                // When the item avoids floats, its non-auto margins are already accounted for in the
                // slot's border-box position/width (margins may overlap floats), so only the auto
                // portion of the resolved margin is added here.
                #[cfg(feature = "float_layout")]
                let (extra_margin_start, _extra_margin_end) = if item_avoids_floats {
                    (
                        resolved_logical_margin.inline_start - item_non_auto_margin.inline_start,
                        resolved_logical_margin.inline_end - item_non_auto_margin.inline_end,
                    )
                } else {
                    (resolved_logical_margin.inline_start, resolved_logical_margin.inline_end)
                };
                #[cfg(not(feature = "float_layout"))]
                let (extra_margin_start, _extra_margin_end) =
                    (resolved_logical_margin.inline_start, resolved_logical_margin.inline_end);

                LogicalOffset {
                    inline_offset: float_avoiding_position.inline_offset
                        + extra_margin_start
                        + inset_offset.inline_offset,
                    block_offset: float_avoiding_position.block_offset + inset_offset.block_offset,
                }
            };

            // Apply alignment
            let item_outer_inline_size = final_logical_size.inline_size + resolved_logical_margin.inline_axis_sum();
            if item_outer_inline_size < container_inner_inline_size {
                let free_inline_space = container_inner_inline_size - item_outer_inline_size;
                match (text_align, direction) {
                    (TextAlign::Auto, _) => {
                        // Do nothing
                    }
                    (TextAlign::LegacyLeft, Direction::Ltr) => {
                        // Do nothing. Inline-start aligned by default.
                    }
                    (TextAlign::LegacyLeft, Direction::Rtl) => logical_location.inline_offset += free_inline_space,
                    (TextAlign::LegacyRight, Direction::Ltr) => logical_location.inline_offset += free_inline_space,
                    (TextAlign::LegacyRight, Direction::Rtl) => {
                        // Do nothing. Inline-start aligned by default.
                    }
                    (TextAlign::LegacyCenter, _) => logical_location.inline_offset += free_inline_space / 2.0,
                }
            }

            let child_baselines =
                BlockChildBaselineProjection::new(writing_direction, tree.get_writing_mode(item.node_id), final_size);

            // A block container's first baseline is the first baseline of its first in-flow child
            // that has one in this container's writing mode.
            if first_baseline.is_none() {
                first_baseline = child_baselines
                    .project(item_layout.first_baselines)
                    .map(|baseline| logical_location.block_offset + baseline);
            }

            // CSS inline-block baseline propagation walks normal-flow block
            // descendants. Block-layout children contribute their last
            // baseline; other formatting contexts contribute their first.
            // A scroll-container block instead forces synthesis at its
            // block-end margin edge (CSS2 10.8 / CSS Inline 3).
            if child_baselines.has_compatible_baseline_set() && !item.is_table {
                let child_baseline = if item.uses_block_layout && !item.is_replaced {
                    if item.overflow.x.is_scroll_container() || item.overflow.y.is_scroll_container() {
                        Some(final_logical_size.block_size + resolved_logical_margin.block_end)
                    } else {
                        child_baselines.project(item_layout.last_baselines)
                    }
                } else {
                    child_baselines.project(item_layout.first_baselines)
                };
                if let Some(baseline) = child_baseline {
                    last_baseline = Some(logical_location.block_offset + baseline);
                }
            }

            // Defer fragment materialization so `align-content` can shift the
            // logical block offset before the physical top-left is known.
            item.pending_layout = Some(PendingBlockLayout {
                layout: Layout {
                    order: item.order,
                    size: item_layout.size,
                    #[cfg(feature = "content_size")]
                    content_size: item_layout.content_size,
                    scrollbar_size,
                    location: Point::ZERO,
                    padding: item.padding,
                    border: item.border,
                    margin: resolved_margin,
                },
                logical_offset: logical_location,
                participates_in_align_content: true,
            });

            #[cfg(feature = "content_size")]
            {
                let contribution_location = LogicalOffset {
                    inline_offset: logical_location.inline_offset - border.inline_start - scrollbar_inset.inline_start,
                    block_offset: logical_location.block_offset - border.block_start - scrollbar_inset.block_start,
                };
                let logical_content_size = writing_mode.to_logical(item_layout.content_size);
                inflow_content_size = inflow_content_size.f32_max(compute_logical_content_size_contribution(
                    contribution_location,
                    final_logical_size,
                    logical_content_size,
                    logical_overflow(item.overflow, writing_mode),
                ));
            }

            // Update the first child's block-start collapsing margin set.
            //
            // An item's cleared block-start margin does not collapse through
            // the container, so clearance terminates the first-child strut.
            #[cfg(feature = "float_layout")]
            if is_collapsing_with_first_margin_set && item_moved_past_float {
                // The item's block-start margin separated from the float and must not
                // propagate to the parent
                is_collapsing_with_first_margin_set = false;
            }
            if is_collapsing_with_first_margin_set && has_clearance {
                is_collapsing_with_first_margin_set = false;
            } else if is_collapsing_with_first_margin_set {
                if item.can_be_collapsed_through {
                    first_child_block_start_margin_set = first_child_block_start_margin_set
                        .collapse_with_set(block_start_margin_set)
                        .collapse_with_set(block_end_margin_set);
                } else {
                    first_child_block_start_margin_set =
                        first_child_block_start_margin_set.collapse_with_set(block_start_margin_set);
                    is_collapsing_with_first_margin_set = false;
                }
            }

            // Update active_collapsible_margin_set
            if item.can_be_collapsed_through {
                active_collapsible_margin_set = active_collapsible_margin_set
                    .collapse_with_set(block_start_margin_set)
                    .collapse_with_set(block_end_margin_set);
                block_offset_for_absolute =
                    committed_block_offset + final_logical_size.block_size + block_margin_offset;
            } else {
                committed_block_offset =
                    logical_location.block_offset - inset_offset.block_offset + final_logical_size.block_size;
                // A self-collapsing item with clearance is not collapsed through (its margins do not collapse
                // with margins of preceding siblings), but its block-start and block-end margins still collapse with each
                // other and with the margins of following siblings.
                if has_clearance && item_layout.margins_can_collapse_through {
                    // The element's border edge stays at the cleared position, but its collapsed margin
                    // extends beyond it: the border edge sits one block-start
                    // margin inside the strut, so following content uses the remainder.
                    committed_block_offset -= block_start_margin_set.resolve();
                    active_collapsible_margin_set = block_start_margin_set.collapse_with_set(block_end_margin_set);
                    active_margin_set_has_clearance = true;
                } else {
                    active_collapsible_margin_set = block_end_margin_set;
                    active_margin_set_has_clearance = false;
                }
                block_offset_for_absolute = committed_block_offset + active_collapsible_margin_set.resolve();
                // Committing in-flow content resolves the position of the current margin-collapse strut,
                // so floats placed before this point no longer force clearance
                #[cfg(feature = "float_layout")]
                block_ctx.commit_strut();
            }
        }
    }

    // A cleared self-collapsing element's strut cannot escape through the
    // parent's block-end, so it contributes to the parent's content block size.
    let last_child_block_end_margin_set =
        if active_margin_set_has_clearance { CollapsibleMarginSet::ZERO } else { active_collapsible_margin_set };
    let block_end_margin_offset = if active_margin_set_has_clearance {
        active_collapsible_margin_set.resolve()
    } else if own_margins_collapse_with_children.end {
        0.0
    } else {
        last_child_block_end_margin_set.resolve()
    };

    committed_block_offset += content_box_inset.block_end + block_end_margin_offset;
    let content_block_size = f32_max(0.0, committed_block_offset);
    (
        inflow_content_size,
        content_block_size,
        first_child_block_start_margin_set,
        last_child_block_end_margin_set,
        first_baseline,
        last_baseline,
    )
}

/// Resolve auto margins in one axis of an absolutely positioned box.
///
/// Auto margins only participate when both insets in the axis are definite.
/// In the inline axis, negative free space is assigned to the non-dominant
/// side so the direction's start edge remains visible. In the block axis it
/// is shared equally, matching CSS Positioned Layout and browser behavior.
#[inline]
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

/// Perform absolute layout on all absolutely positioned children.
#[inline]
fn perform_absolute_layout_on_absolute_children(
    tree: &mut impl LayoutBlockContainer,
    items: &[BlockItem],
    area_size: Size<f32>,
    area_offset: Point<f32>,
    writing_direction: WritingDirection,
    containing_outer_size: Size<f32>,
) -> Size<f32> {
    let direction = writing_direction.direction;
    let writing_mode = writing_direction.mode;
    let converter = writing_direction.converter(containing_outer_size);
    let area_width = area_size.width;
    let area_height = area_size.height;
    let percentage_basis = writing_mode.to_logical(area_size).inline_size;

    #[cfg_attr(not(feature = "content_size"), allow(unused_mut))]
    let mut absolute_content_size = Size::ZERO;

    for item in items.iter().filter(|item| item.position == Position::Absolute) {
        let aspect_ratio = tree.get_resolved_aspect_ratio(item.node_id);
        let child_writing_mode = tree.get_writing_mode(item.node_id);
        let child_style = tree.get_block_child_style(item.node_id);

        // Skip items that are display:none or are not position:absolute
        if child_style.box_generation_mode() == BoxGenerationMode::None || child_style.position() != Position::Absolute
        {
            continue;
        }

        let margin = child_style
            .margin()
            .map(|margin| margin.resolve_to_option(percentage_basis, |val, basis| tree.calc(val, basis)));
        let padding = child_style.padding().resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis));
        let border = child_style.border().resolve_or_zero(Some(percentage_basis), |val, basis| tree.calc(val, basis));
        let padding_border_sum = (padding + border).sum_axes();
        let box_sizing = child_style.box_sizing();
        let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };

        // Resolve inset
        let left = child_style.inset().left.maybe_resolve(area_width, |val, basis| tree.calc(val, basis));
        let right = child_style.inset().right.maybe_resolve(area_width, |val, basis| tree.calc(val, basis));
        let top = child_style.inset().top.maybe_resolve(area_height, |val, basis| tree.calc(val, basis));
        let bottom = child_style.inset().bottom.maybe_resolve(area_height, |val, basis| tree.calc(val, basis));
        let block_auto_behavior = match child_writing_mode.block_axis() {
            AbsoluteAxis::Horizontal if left.is_some() && right.is_some() => AutoSizeBehavior::StretchImplicit,
            AbsoluteAxis::Vertical if top.is_some() && bottom.is_some() => AutoSizeBehavior::StretchImplicit,
            _ => AutoSizeBehavior::FitContent,
        };
        let inline_auto_behavior = match child_writing_mode.inline_axis() {
            AbsoluteAxis::Horizontal if left.is_some() && right.is_some() => AutoSizeBehavior::StretchImplicit,
            AbsoluteAxis::Vertical if top.is_some() && bottom.is_some() => AutoSizeBehavior::StretchImplicit,
            _ => AutoSizeBehavior::FitContent,
        };

        // Keep the unresolved values: intrinsic widths need the absolute
        // containing block after insets and non-auto margins are known.
        let raw_size = child_style.size();
        let raw_min_size = child_style.min_size();
        let raw_max_size = child_style.max_size();

        // Compute numeric known dimensions from min/max/inherent size styles.
        let mut style_size =
            raw_size.maybe_resolve(area_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);
        let mut min_size =
            raw_min_size.maybe_resolve(area_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);
        let mut max_size =
            raw_max_size.maybe_resolve(area_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);

        drop(child_style);

        let static_edge = converter.to_physical_point(item.static_position, Size::ZERO);
        let static_position_in_area = Point { x: static_edge.x - area_offset.x, y: static_edge.y - area_offset.y };
        let static_position_opportunity = Size {
            width: if direction.is_rtl() { static_position_in_area.x } else { area_width - static_position_in_area.x },
            height: area_height - static_position_in_area.y,
        };
        let imcb = InsetModifiedContainingBlock::new(
            area_size,
            Rect { left, right, top, bottom },
            static_position_opportunity,
            margin,
        );
        let available_width = imcb.stretch_border_box_opportunity().width;
        let child_available_size = imcb.margin_box_opportunity();
        let implicit_auto_stretch_size = imcb.implicit_auto_stretch_size();
        let authored_stretch = StretchSizeProperties::new(raw_size, raw_min_size, raw_max_size)
            .resolve(imcb.authored_stretch_available_space(), padding_border_sum);
        style_size = style_size.or(authored_stretch.preferred);
        min_size = min_size.or(authored_stretch.min);
        max_size = max_size.or(authored_stretch.max);
        let intrinsic_inputs = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            sizing_purpose: SizingPurpose::IntrinsicContribution,
            axis: RequestedAxis::Horizontal,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: crate::AutoSizeBehavior::FitContent,
            orthogonal_fallback: OrthogonalFallback::UseInitialContainingBlock,
            known_dimensions: Size::NONE,
            definite_dimensions: Size::NONE,
            parent_size: area_size.map(Some),
            parent_writing_mode: writing_mode,
            available_space: Size {
                width: AvailableSpace::Definite(child_available_size.width),
                height: AvailableSpace::Definite(child_available_size.height),
            },
            ignored_margins_for_stretch: Rect::default(),
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let ratio_dependent_sizing = resolve_ratio_dependent_intrinsic_sizing(
            style_size,
            min_size,
            max_size,
            aspect_ratio,
            padding_border_sum,
            AbsoluteAxis::Horizontal,
            aspect_ratio.is_some()
                && [raw_size.height, raw_min_size.height, raw_max_size.height]
                    .into_iter()
                    .any(|value| value.may_have_percentage_dependence() || value.is_stretch()),
        );
        let intrinsic = resolve_intrinsic_width_constraints(
            tree,
            item.node_id,
            intrinsic_inputs,
            IntrinsicWidthInput {
                preferred: raw_size.width,
                min: raw_min_size.width,
                max: raw_max_size.width,
                available_space: AvailableSpace::Definite(f32_max(available_width, 0.0)),
                ratio_dependent_sizing,
            },
        );
        style_size.width = style_size.width.or(intrinsic.preferred.value);
        min_size.width = min_size.width.or(intrinsic.min.value);
        max_size.width = max_size.width.or(intrinsic.max.value);

        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: style_size,
            min_size,
            max_size,
            size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
            writing_mode: child_writing_mode,
            inline_auto_behavior,
            block_auto_behavior,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio,
            padding_border: padding_border_sum,
        });
        let block_size_resolver = AbsoluteBlockSizeResolver::new(AbsoluteBlockSizeInput {
            writing_mode: child_writing_mode,
            size: raw_size,
            min_size: raw_min_size,
            max_size: raw_max_size,
            aspect_ratio,
            padding_border: padding_border_sum,
            block_auto_behavior,
            is_scroll_container: item.overflow.x.is_scroll_container() || item.overflow.y.is_scroll_container(),
            is_replaced: item.is_replaced,
            constraint_sources: resolved.block_axis_constraints(child_writing_mode),
        });
        let mut min_size = resolved.min_size.or(padding_border_sum.map(Some)).maybe_max(padding_border_sum);
        let mut max_size = resolved.max_size;
        let mut known_dimensions = resolve_formatting_context_size(FormattingContextSizeInput {
            size: resolved.size,
            size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
            writing_mode: child_writing_mode,
            inline_auto_behavior,
            block_auto_behavior,
            stretch_size: implicit_auto_stretch_size,
            aspect_ratio,
            padding_border: padding_border_sum,
        })
        .maybe_clamp(min_size, max_size);

        // If width is still auto then one or both horizontal insets are also auto. CSS 2.2
        // 10.3.7 requires a non-replaced box to use shrink-to-fit width. Replaced boxes instead
        // follow 10.3.8: their leaf sizing function consumes the IMCB constraint directly.
        if known_dimensions.width.is_none() && !item.is_replaced {
            known_dimensions.width = Some(fit_content_width(
                tree,
                item.node_id,
                ChildLayoutInput::new(
                    known_dimensions,
                    area_size.map(Some),
                    writing_mode,
                    Size {
                        width: AvailableSpace::Definite(child_available_size.width),
                        height: AvailableSpace::Definite(
                            child_available_size.height.maybe_clamp(min_size.height, max_size.height),
                        ),
                    },
                    SizingMode::ContentSize,
                    Line::FALSE,
                ),
                available_width,
            ));
            known_dimensions = known_dimensions
                .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border_sum)
                .maybe_clamp(min_size, max_size);
        }

        let sizing = block_size_resolver.resolve(
            tree,
            item.node_id,
            ChildLayoutInput::new(
                known_dimensions,
                area_size.map(Some),
                writing_mode,
                Size {
                    width: AvailableSpace::Definite(child_available_size.width),
                    height: AvailableSpace::Definite(child_available_size.height),
                },
                SizingMode::ContentSize,
                Line::FALSE,
            ),
            AbsoluteBoxSizing { size: known_dimensions, min_size, max_size },
        );
        known_dimensions = sizing.size;
        min_size = sizing.min_size;
        max_size = sizing.max_size;

        let measured_size = tree.measure_child_size_both(
            item.node_id,
            ChildLayoutInput::new(
                known_dimensions,
                area_size.map(Some),
                writing_mode,
                Size {
                    width: AvailableSpace::Definite(
                        child_available_size.width.maybe_clamp(min_size.width, max_size.width),
                    ),
                    height: AvailableSpace::Definite(
                        child_available_size.height.maybe_clamp(min_size.height, max_size.height),
                    ),
                },
                SizingMode::ContentSize,
                Line::FALSE,
            ),
        );

        let final_size = known_dimensions.unwrap_or(measured_size).maybe_clamp(min_size, max_size);
        let static_position = converter.to_physical_point(item.static_position, final_size);

        let layout_output = tree.compute_child_layout(
            item.node_id,
            LayoutInput {
                known_dimensions: final_size.map(Some),
                definite_dimensions: known_dimensions,
                parent_size: area_size.map(Some),
                parent_writing_mode: writing_mode,
                available_space: Size {
                    width: AvailableSpace::Definite(
                        child_available_size.width.maybe_clamp(min_size.width, max_size.width),
                    ),
                    height: AvailableSpace::Definite(
                        child_available_size.height.maybe_clamp(min_size.height, max_size.height),
                    ),
                },
                ignored_margins_for_stretch: Rect::default(),
                sizing_mode: SizingMode::ContentSize,
                sizing_purpose: SizingPurpose::Layout,
                axis: RequestedAxis::Both,
                inline_auto_behavior: AutoSizeBehavior::FitContent,
                block_auto_behavior: crate::AutoSizeBehavior::FitContent,
                orthogonal_fallback: OrthogonalFallback::UseInitialContainingBlock,
                run_mode: RunMode::PerformLayout,
                vertical_margins_are_collapsible: Line::FALSE,
            },
        );

        let horizontal_margin = resolve_absolute_axis_margins(
            Line { start: margin.left, end: margin.right },
            Line { start: left, end: right },
            area_width,
            final_size.width,
            false,
            !direction.is_rtl(),
        );
        let vertical_margin = resolve_absolute_axis_margins(
            Line { start: margin.top, end: margin.bottom },
            Line { start: top, end: bottom },
            area_height,
            final_size.height,
            true,
            true,
        );
        let resolved_margin = Rect {
            left: horizontal_margin.start,
            right: horizontal_margin.end,
            top: vertical_margin.start,
            bottom: vertical_margin.end,
        };

        let x_offset = match (left, right) {
            (Some(left), Some(right)) => {
                if direction.is_rtl() {
                    area_size.width - final_size.width - right - resolved_margin.right
                } else {
                    left + resolved_margin.left
                }
            }
            (Some(left), None) => left + resolved_margin.left,
            (None, Some(right)) => area_size.width - final_size.width - right - resolved_margin.right,
            (None, None) => {
                if direction.is_rtl() {
                    static_position.x - resolved_margin.right - area_offset.x
                } else {
                    static_position.x + resolved_margin.left - area_offset.x
                }
            }
        };
        let location = Point {
            x: x_offset + area_offset.x,
            y: top
                .map(|top| top + resolved_margin.top)
                .or(bottom.map(|bottom| area_size.height - final_size.height - bottom - resolved_margin.bottom))
                .maybe_add(area_offset.y)
                .unwrap_or(static_position.y + resolved_margin.top),
        };
        let scrollbar_size = item.scrollbar_size;

        tree.set_unrounded_layout(
            item.node_id,
            &Layout {
                order: item.order,
                size: final_size,
                #[cfg(feature = "content_size")]
                content_size: layout_output.content_size,
                scrollbar_size,
                location,
                padding,
                border,
                margin: resolved_margin,
            },
        );

        #[cfg(feature = "content_size")]
        {
            let relative_location = Point { x: location.x - area_offset.x, y: location.y - area_offset.y };
            absolute_content_size = absolute_content_size.f32_max(compute_content_size_contribution(
                relative_location,
                final_size,
                layout_output.content_size,
                item.overflow,
            ));
        }
    }

    absolute_content_size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_baseline_projection_requires_the_same_writing_mode() {
        let size = Size { width: 40.0, height: 20.0 };

        let horizontal = BlockChildBaselineProjection::new(
            WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
            WritingMode::HorizontalTb,
            size,
        );
        assert_eq!(horizontal.project(Point { x: None, y: Some(12.0) }), Some(12.0));

        let orthogonal = BlockChildBaselineProjection::new(
            WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
            WritingMode::VerticalRl,
            size,
        );
        assert_eq!(orthogonal.project(Point { x: None, y: Some(12.0) }), None);

        let opposite_block_flow = BlockChildBaselineProjection::new(
            WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr),
            WritingMode::VerticalLr,
            size,
        );
        assert_eq!(opposite_block_flow.project(Point { x: Some(12.0), y: None }), None);
    }
}
