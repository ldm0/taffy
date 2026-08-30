//! Computes the CSS block layout algorithm in the case that the block container being laid out contains only block-level boxes
use crate::geometry::{
    Line, LogicalBoxStrut, LogicalOffset, LogicalSize, LogicalStaticPosition, Point, Rect, Size, WritingDirection,
};
use crate::style::{AlignContentKeyword, AvailableSpace, CoreStyle, LengthPercentageAuto, Overflow, Position};
use crate::style_helpers::TaffyMaxContent;
use crate::tree::{
    AutoSizeBehavior, ChildLayoutInput, CollapsibleMarginSet, Layout, LayoutInput, LayoutOutput, RunMode, SizingMode,
    SizingPurpose,
};
use crate::tree::{LayoutPartialTree, LayoutPartialTreeExt, NodeId};
use crate::util::debug::debug_log;
use crate::util::sys::f32_max;
use crate::util::sys::Vec;
use crate::util::MaybeMath;
use crate::util::{MaybeResolve, ResolveOrZero};
use crate::{
    BlockContainerStyle, BlockItemStyle, BoxGenerationMode, BoxSizing, Direction, LayoutBlockContainer, RequestedAxis,
    TextAlign, WritingMode,
};

use super::common::absolute::{layout_out_of_flow_item, OutOfFlowItem};
use super::common::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use super::common::baseline::{logical_block_baseline, physical_baseline};
use super::common::intrinsic_size::{
    fit_content_inline_size, intrinsic_content_size_from_initial_geometry, measure_aspect_ratio_automatic_minimum,
    measure_child_intrinsic_contribution, replaced_min_content_contribution_is_cyclic,
    resolve_intrinsic_axis_constraints, resolve_node_size_constraints, BlockSizeProperties, ContentBasedBlockSize,
    IntrinsicAxisInput, IntrinsicAxisValue, NodeSizeConstraintInput, ResolvedNodeSizing,
};
use super::common::stretch::resolve_stretch_size_constraints;
use crate::tree::OutOfFlowContainingBlock;

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
    pub fn bfc_layout_opportunities(
        &self,
        min_block_offset: f32,
        margins: [f32; 2],
        direction: Direction,
        clear: Clear,
    ) -> Vec<BfcSlot> {
        let mut opportunities = self.bfc.float_context.bfc_layout_opportunities(
            min_block_offset + self.block_offset,
            self.content_box_line_insets,
            margins,
            direction,
            clear,
        );
        for opportunity in &mut opportunities {
            opportunity.y -= self.block_offset;
            opportunity.x -= self.line_insets[0];
        }
        opportunities
    }

    /// Search for one BFC line/block-space suitable for a box that establishes
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

    /// A percentage in the replaced box's preferred or maximum logical
    /// inline size makes its min-content contribution cyclic. Preserve that
    /// provenance so a direct preferred length cannot bypass child sizing.
    has_cyclic_replaced_inline_contribution: bool,

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
    /// Content-derived logical inline minimum whose authored/transferred
    /// ordering must be reapplied when percentages become definite.
    automatic_inline_minimum: Option<f32>,
    /// The overflow style of the item
    overflow: Point<Overflow>,
    /// Total physical space occupied by the item's scrollbar gutters.
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
    static_position: LogicalStaticPosition,
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
    let style = tree.get_block_container_style(node_id);

    // Pull these out earlier to avoid borrowing issues
    let overflow = style.overflow();
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let aspect_ratio = if inputs.sizing_mode == SizingMode::InherentSize {
        resolved_aspect_ratio
    } else {
        resolved_aspect_ratio.disabled()
    };
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let padding_border_size = (padding + border).sum_axes();
    let content_box_inset_size = padding_border_size + scrollbar_insets.sum_axes();
    let contained_outer_size = size_containment.resolve_outer_size(Size::ZERO, content_box_inset_size);
    let contained_outer_block_size = writing_mode.to_logical(contained_outer_size).block_size;
    let box_sizing = style.box_sizing();
    let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
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
        padding_border_size,
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
        .with_block_constraint_dependency(node_sizing.depends_on_block_constraints)
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
    let percentage_basis = inputs.constraint_space(writing_mode).margin_padding_percentage_basis();
    let LayoutInput { available_space, run_mode, block_margins_are_collapsible, table_cell, .. } = inputs;

    let scrollbar_gutter = tree.get_scrollbar_insets(node_id);
    let style = tree.get_block_container_style(node_id);
    let raw_margin = style.margin();
    let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
    let direction = style.direction();
    let writing_direction = WritingDirection::new(writing_mode, direction);

    let padding_border = padding + border;
    let content_box_inset = padding_border + scrollbar_gutter;
    let logical_padding = writing_direction.to_logical_box_strut(padding);
    let logical_border = writing_direction.to_logical_box_strut(border);
    let logical_scroll_origin_inset = logical_border + writing_direction.to_logical_box_strut(scrollbar_gutter);
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
        start: block_margins_are_collapsible.start
            && !is_scroll_container
            && style.position() == Position::Relative
            && logical_padding.block_start == 0.0
            && logical_border.block_start == 0.0,
        end: block_margins_are_collapsible.end
            && !is_scroll_container
            && style.position() == Position::Relative
            && logical_padding.block_end == 0.0
            && logical_border.block_end == 0.0,
    };
    let has_structural_styles_preventing_collapsing_through = !style.is_block()
        || block_ctx.is_bfc_root()
        || is_scroll_container
        || style.position() == Position::Absolute
        || logical_padding.block_start > 0.0
        || logical_padding.block_end > 0.0
        || logical_border.block_start > 0.0
        || logical_border.block_end > 0.0;

    let text_align = style.text_align();
    let align_content = style.align_content();
    drop(style);

    let available_logical_space = writing_mode.to_logical(available_space);

    // A child with `stretch` fills the containing block's margin box. In a
    // shared BFC, a margin on an unseparated parent edge is ignored for that
    // sizing calculation even though normal margin collapsing still controls
    // fragment placement. Keep this as constraint-space state at the parent
    // boundary, matching Blink's IgnoreMarginsForStretch flag.
    let logical_ignore_stretch_margins = LogicalBoxStrut {
        inline_start: false,
        inline_end: false,
        block_start: !block_ctx.is_bfc_root() && logical_padding_border.block_start == 0.0,
        block_end: !block_ctx.is_bfc_root() && logical_padding_border.block_end == 0.0,
    };
    let ignore_stretch_margins = writing_direction.to_physical_box_strut(logical_ignore_stretch_margins);

    // 1. Generate items
    let mut items = generate_item_list(
        tree,
        node_id,
        writing_direction,
        container_content_box_size,
        available_logical_space,
        ignore_stretch_margins,
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
    if let (RunMode::ComputeSize, Some(container_outer_block_size)) = (run_mode, outer_logical_size.block_size) {
        let outer_size = writing_mode.to_physical(LogicalSize {
            inline_size: container_outer_inline_size,
            block_size: container_outer_block_size,
        });
        return LayoutOutput::from_outer_size(outer_size)
            .with_block_constraint_dependency(content_inline_size_depends_on_block_constraints);
    }

    // We can also short-circuit when only the physical axis corresponding to
    // this formatting context's logical inline axis was requested.
    if run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::from(writing_mode.inline_axis()) {
        let outer_size =
            writing_mode.to_physical(LogicalSize { inline_size: container_outer_inline_size, block_size: 0.0 });
        return LayoutOutput::from_outer_size(outer_size)
            .with_block_constraint_dependency(content_inline_size_depends_on_block_constraints);
    }

    let container_percentage_resolution_block_size = definite_logical_size.block_size;
    // Relative block-axis percentage insets only resolve against a definite
    // containing-block block size. A minimum may determine the eventual used
    // size, but it does not make an otherwise-auto block size definite.
    let relative_inset_percentage_resolution_block_size = definite_logical_size.block_size;

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
            node_id,
            run_mode,
            outer_inline_size: container_outer_inline_size,
            percentage_resolution_block_size: container_percentage_resolution_block_size,
            relative_inset_percentage_resolution_block_size,
            content_box_inset: logical_content_box_inset,
            scroll_origin_inset: logical_scroll_origin_inset,
            text_align,
            writing_direction,
            own_margins_collapse_with_children,
            ignore_stretch_margins: logical_ignore_stretch_margins,
        },
        block_ctx,
    );

    // Root BFCs contain floats
    #[cfg(feature = "float_layout")]
    if block_ctx.is_bfc_root() || is_scroll_container {
        intrinsic_outer_block_size = intrinsic_outer_block_size.max(block_ctx.floated_block_size_contribution());
    }

    let block_size_constraints = content_based_block_size
        .resolve(writing_mode, Some(container_outer_inline_size), intrinsic_outer_block_size)
        .resolve_against(size_logical.block_size, content_based_block_size.resolved_constraints());
    let container_outer_block_size = outer_logical_size
        .block_size
        .or(block_size_constraints.preferred)
        .unwrap_or(intrinsic_outer_block_size)
        .maybe_clamp(block_size_constraints.min, block_size_constraints.max)
        .maybe_max(Some(logical_padding_border_size.block_size));
    let final_logical_size =
        LogicalSize { inline_size: container_outer_inline_size, block_size: container_outer_block_size };
    let final_outer_size = writing_mode.to_physical(final_logical_size);

    // Keep the trailing margin strut separate while measuring intrinsic block
    // size, then decide whether it may escape only after the used block size is
    // known. A definite initial size, a preferred-ratio transfer, or a min/max
    // clamp that changes the measured size captures the strut instead of
    // turning it into either intrinsic content or an external margin. This
    // mirrors Blink's FinishLayout ordering:
    // compute intrinsic size, compute the final fragment size, then clear the
    // end margin strut when the initial size was definite or the final size
    // differs from the intrinsic size.
    let own_block_end_margin_collapses_with_children = own_margins_collapse_with_children.end
        && definite_logical_size.block_size.is_none()
        && container_outer_block_size == intrinsic_outer_block_size;

    // A baseline-aligned table cell always exposes a baseline at its
    // block-end content edge when none of its descendants supplies one. The
    // external table formatter consumes this during row measurement and may
    // feed the resolved shared row baseline back through `table_cell` for the
    // final layout. This mirrors Blink's `FinalizeTableCellLayout` rather than
    // synthesizing a generic block baseline at the table adapter boundary.
    if table_cell.is_some()
        && align_content.is_some_and(|alignment| alignment.keyword() == AlignContentKeyword::Baseline)
        && first_baseline.is_none()
    {
        let fallback_baseline = (intrinsic_outer_block_size - logical_content_box_inset.block_end).max(0.0);
        first_baseline = Some(fallback_baseline);
        last_baseline = Some(fallback_baseline);
    }

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
        let any_out_of_flow = items.iter().any(|item| item.position == Position::Absolute);
        if any_in_flow || any_out_of_flow {
            // Table baseline alignment is a row-provided constraint, not a
            // distribution fallback. Only a cell with in-flow content moves;
            // an OOF-only cell keeps its static-position candidate at the
            // cell's block start, matching CSS Tables. When in-flow content
            // does move, its OOF candidates move with the same fragment group.
            let row_baseline = table_cell
                .and_then(|cell| cell.alignment_baseline)
                .filter(|_| align_content.keyword() == AlignContentKeyword::Baseline);
            let group_offset = if let Some(row_baseline) = row_baseline {
                any_in_flow.then(|| row_baseline - first_baseline.expect("table cells synthesize a first baseline"))
            } else {
                let keyword = apply_alignment_fallback(free_space, 1, align_content);
                Some(compute_alignment_offset(free_space, 1, 0.0, keyword, false, true))
            };
            if let Some(group_offset) = group_offset {
                first_baseline = first_baseline.map(|baseline| baseline + group_offset);
                last_baseline = last_baseline.map(|baseline| baseline + group_offset);
                for item in items.iter_mut() {
                    if let Some(pending) = item.pending_layout.as_mut() {
                        if pending.participates_in_align_content {
                            pending.logical_offset.block_offset += group_offset;
                        }
                    }
                    if item.position == Position::Absolute {
                        item.static_position.offset.block_offset += group_offset;
                        if run_mode == RunMode::PerformLayout {
                            tree.set_out_of_flow_static_position(node_id, item.node_id, item.static_position);
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
                                    - logical_scroll_origin_inset.inline_start,
                                block_offset: pending.logical_offset.block_offset
                                    - logical_scroll_origin_inset.block_start,
                            };
                            inflow_content_size =
                                inflow_content_size.f32_max(compute_logical_content_size_contribution(
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
    }

    // Determine whether this node can be collapsed through
    let all_in_flow_children_can_be_collapsed_through = items.iter().all(|item| {
        #[cfg(feature = "float_layout")]
        if item.float.is_floated() {
            return true;
        }
        item.position == Position::Absolute || item.can_be_collapsed_through
    });
    // CSS Sizing's intrinsic block-size keywords and cyclic percentages
    // "behave as auto" for the legacy CSS2 margin-collapse conditions. Base
    // collapse-through on the final used block size instead of the authored
    // size syntax so zero fixed sizes, intrinsic sizes, ratios and min/max
    // clamps all follow the same rule.
    let can_be_collapsed_through = !has_structural_styles_preventing_collapsing_through
        && container_outer_block_size == 0.0
        && all_in_flow_children_can_be_collapsed_through;

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
        node_id,
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
    ignore_stretch_margins: Rect<bool>,
) -> Vec<BlockItem> {
    let writing_mode = writing_direction.mode;
    let physical_node_inner_size = writing_mode.to_physical(node_inner_size);
    let numeric_children: Vec<_> = tree.child_ids(node).collect();
    let candidate_count = tree.out_of_flow_candidate_count(node);
    let candidates: Vec<_> = (0..candidate_count).map(|index| tree.get_out_of_flow_candidate(node, index)).collect();
    let mut child_ids = Vec::with_capacity(numeric_children.len() + candidates.len());
    for insertion_index in 0..=numeric_children.len() {
        child_ids.extend(
            candidates
                .iter()
                .filter(|candidate| candidate.insertion_index.min(numeric_children.len()) == insertion_index)
                .map(|candidate| candidate.node),
        );
        if let Some(child) = numeric_children.get(insertion_index) {
            child_ids.push(*child);
        }
    }
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
            let child_logical_size = child_writing_mode.to_logical(raw_size);
            let child_logical_min_size = child_writing_mode.to_logical(raw_min_size);
            let child_logical_max_size = child_writing_mode.to_logical(raw_max_size);
            let child_block_size_depends_on_parent =
                [raw_logical_size.block_size, raw_logical_min_size.block_size, raw_logical_max_size.block_size]
                    .into_iter()
                    .any(|value| value.may_have_percentage_dependence() || value.is_stretch());
            let mut depends_on_block_constraints = child_block_size_depends_on_parent && aspect_ratio.has_ratio();
            let mut automatic_inline_minimum = None;
            let mut intrinsic_context = None;
            let mut cyclic_replaced_inline_contribution = false;
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
            let should_stretch_auto_inline_size =
                tree.should_stretch_auto_inline_size_in_block_container(child_node_id);
            let establishes_new_formatting_context = tree.establishes_new_formatting_context(child_node_id);
            let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
            let inline_auto_behavior = if position != Position::Absolute
                && is_not_floated
                && !is_table
                && !is_replaced
                && should_stretch_auto_inline_size
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
                && !establishes_new_formatting_context
                && position != Position::Absolute
                && is_not_floated
                && !is_scroll_container
                && child_writing_mode == writing_mode;

            drop(child_style);

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
                    block_size: available_space.block_size,
                };
                let available_inline_size =
                    child_available_logical_space.inline_size.maybe_sub(logical_margin.inline_axis_sum());
                let child_available_space = writing_mode.to_physical(LogicalSize {
                    inline_size: available_inline_size,
                    block_size: child_available_logical_space.block_size.maybe_sub(logical_margin.block_axis_sum()),
                });
                let stretch_margin = Rect {
                    left: if ignore_stretch_margins.left { 0.0 } else { resolved_margin.left },
                    right: if ignore_stretch_margins.right { 0.0 } else { resolved_margin.right },
                    top: if ignore_stretch_margins.top { 0.0 } else { resolved_margin.top },
                    bottom: if ignore_stretch_margins.bottom { 0.0 } else { resolved_margin.bottom },
                };
                // A definite external constraint does not make an auto
                // containing-block block size definite. Only the used
                // content-box size can resolve block-axis stretch.
                let stretch_containing_space = writing_mode.to_physical(LogicalSize {
                    inline_size: child_available_logical_space.inline_size,
                    block_size: node_inner_size
                        .block_size
                        .map(AvailableSpace::Definite)
                        .unwrap_or(AvailableSpace::MaxContent),
                });
                let stretch_available_space = Size {
                    width: stretch_containing_space
                        .width
                        .into_option()
                        .maybe_sub(stretch_margin.left + stretch_margin.right),
                    height: stretch_containing_space
                        .height
                        .into_option()
                        .maybe_sub(stretch_margin.top + stretch_margin.bottom),
                };
                let stretch = resolve_stretch_size_constraints(
                    raw_size,
                    raw_min_size,
                    raw_max_size,
                    stretch_available_space,
                    pb_sum,
                );
                size = size.or(stretch.preferred);
                min_size = min_size.or(stretch.min);
                max_size = max_size.or(stretch.max);
                let intrinsic_axis = child_writing_mode.inline_axis();
                let intrinsic_inputs = LayoutInput {
                    orthogonal_fallback: crate::tree::OrthogonalFallback::UseInitialContainingBlock,
                    run_mode: RunMode::ComputeSize,
                    sizing_mode: SizingMode::InherentSize,
                    sizing_purpose: SizingPurpose::IntrinsicContribution,
                    axis: intrinsic_axis.into(),
                    inline_auto_behavior: AutoSizeBehavior::FitContent,
                    block_auto_behavior: AutoSizeBehavior::FitContent,
                    known_dimensions: Size::NONE,
                    definite_dimensions: Size::NONE,
                    parent_size: physical_node_inner_size,
                    parent_writing_mode: writing_mode,
                    available_space: child_available_space,
                    block_margins_are_collapsible: Line::TRUE,
                    table_cell: None,
                };
                let intrinsic_available_size = child_available_space.get_abs(intrinsic_axis);
                let has_cyclic_replaced_inline_contribution = is_replaced
                    && replaced_min_content_contribution_is_cyclic(
                        intrinsic_inputs,
                        child_writing_mode,
                        raw_size,
                        raw_max_size,
                    );
                intrinsic_context = Some((intrinsic_axis, intrinsic_inputs, intrinsic_available_size));
                cyclic_replaced_inline_contribution = has_cyclic_replaced_inline_contribution;
            }

            let preferred_size_is_indefinite = size.map(|size| size.is_none());
            let mut resolved = resolve_size_constraints(SizeConstraintInput {
                size,
                preferred_size_is_indefinite,
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
            if let Some((intrinsic_axis, intrinsic_inputs, intrinsic_available_size)) = intrinsic_context {
                let content_size_override = if is_replaced {
                    IntrinsicAxisValue::default()
                } else {
                    intrinsic_content_size_from_initial_geometry(
                        intrinsic_axis,
                        resolved.initial_geometry(),
                        aspect_ratio,
                        pb_sum,
                    )
                };
                let intrinsic = resolve_intrinsic_axis_constraints(
                    tree,
                    child_node_id,
                    intrinsic_inputs,
                    IntrinsicAxisInput {
                        preferred: child_logical_size.inline_size,
                        min: child_logical_min_size.inline_size,
                        max: child_logical_max_size.inline_size,
                        available_space: intrinsic_available_size,
                        axis: intrinsic_axis,
                        content_size_override,
                    },
                );
                resolved.apply_late_intrinsic_axis(
                    intrinsic_axis,
                    intrinsic.preferred,
                    intrinsic.preferred_aspect_ratio_applied,
                    intrinsic.min,
                    intrinsic.max,
                );
                depends_on_block_constraints |= intrinsic.depends_on_block_constraints;
                let automatic_minimum = measure_aspect_ratio_automatic_minimum(
                    tree,
                    child_node_id,
                    intrinsic_inputs,
                    intrinsic_axis,
                    pb_sum,
                    resolved,
                );
                resolved.apply_automatic_minimum(intrinsic_axis, automatic_minimum.value);
                automatic_inline_minimum = automatic_minimum.value;
                depends_on_block_constraints |= automatic_minimum.depends_on_block_constraints;
            }
            size = resolved.size;
            min_size = resolved.min_size;
            max_size = resolved.max_size;

            Some(BlockItem {
                node_id: child_node_id,
                order: 0,
                is_table,
                is_replaced,
                has_cyclic_replaced_inline_contribution: cyclic_replaced_inline_contribution,
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
                automatic_inline_minimum,
                overflow,
                scrollbar_size,
                position,
                inset,
                margin,
                padding,
                border,
                padding_border_sum: pb_sum,

                // Fields to be computed later (for now we initialise with dummy values)
                static_position: LogicalStaticPosition::default(),
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
) -> (f32, bool) {
    let parent_writing_mode = parent_writing_direction.mode;

    let mut max_child_inline_size = 0.0;
    #[cfg(feature = "float_layout")]
    let mut float_contribution = FloatIntrinsicWidthCalculator::new(available_inline_size);
    let mut depends_on_block_constraints = false;
    for item in items.iter_mut().filter(|item| item.position != Position::Absolute) {
        let mut known_dimensions = item.size.maybe_clamp(item.min_size, item.max_size);
        let mut known_logical_size = parent_writing_mode.to_logical(known_dimensions);
        if item.has_cyclic_replaced_inline_contribution {
            known_logical_size.inline_size = None;
            known_dimensions = parent_writing_mode.to_physical(known_logical_size);
        }
        let min_logical_size = parent_writing_mode.to_logical(item.min_size);
        let max_logical_size = parent_writing_mode.to_logical(item.max_size);

        // The containing block's inline size depends on this contribution, so
        // cyclic percentage margins resolve against zero rather than the
        // external available-space constraint.
        let logical_margin = parent_writing_direction
            .to_logical_box_strut(item.margin.resolve_or_zero(Some(0.0), |val, basis| tree.calc(val, basis)));
        let item_inline_margin_sum = logical_margin.inline_axis_sum();
        let inline_size = match known_logical_size.inline_size {
            Some(inline_size) => inline_size,
            None => {
                let measured = measure_child_intrinsic_contribution(
                    tree,
                    item.node_id,
                    ChildLayoutInput::new(
                        known_dimensions,
                        Size::NONE,
                        parent_writing_mode,
                        parent_writing_mode.to_physical(LogicalSize {
                            inline_size: available_inline_size.maybe_sub(item_inline_margin_sum),
                            block_size: AvailableSpace::MinContent,
                        }),
                        SizingMode::InherentSize,
                        Line::TRUE,
                    ),
                    parent_writing_mode.inline_axis(),
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
/// the container's intrinsic inline size. Numeric percentage values are therefore
/// materialized again here, after that inline size is known. Intrinsic
/// keyword measurements from the contribution phase are retained when the raw
/// style cannot be reduced to a numeric value.
fn resolve_block_item_final_style(
    tree: &mut impl LayoutBlockContainer,
    item: &mut BlockItem,
    parent_size: Size<Option<f32>>,
    parent_writing_mode: WritingMode,
) {
    let percentage_basis = parent_writing_mode.to_logical(parent_size).inline_size;
    let aspect_ratio = tree.get_resolved_aspect_ratio(item.node_id);
    let child_writing_mode = tree.get_writing_mode(item.node_id);
    let (size, min_size, max_size, padding, border) = {
        let style = tree.get_block_child_style(item.node_id);
        let padding = style.padding().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let border = style.border().resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let padding_border_sum = (padding + border).sum_axes();
        let box_sizing = style.box_sizing();
        let box_sizing_adjustment = if box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };
        let raw_size = style.size();
        let preferred_size =
            raw_size.maybe_resolve(parent_size, |val, basis| tree.calc(val, basis)).maybe_add(box_sizing_adjustment);
        let mut resolved = resolve_size_constraints(SizeConstraintInput {
            size: preferred_size,
            preferred_size_is_indefinite: preferred_size.map(|size| size.is_none()),
            min_size: style
                .min_size()
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            max_size: style
                .max_size()
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            size_is_auto: raw_size.map(|dimension| dimension.is_auto()),
            writing_mode: child_writing_mode,
            inline_auto_behavior: item.inline_auto_behavior,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio,
            padding_border: padding_border_sum,
        });
        resolved.apply_automatic_minimum(child_writing_mode.inline_axis(), item.automatic_inline_minimum);
        (resolved.size, resolved.min_size, resolved.max_size, padding, border)
    };

    item.size = size.or(item.size);
    item.min_size = min_size.or(item.min_size);
    item.max_size = max_size.or(item.max_size);
    item.padding = padding;
    item.border = border;
    item.padding_border_sum = (padding + border).sum_axes();
}

/// Immutable container state shared while positioning in-flow block children.
#[derive(Clone, Copy, Debug)]
struct BlockContainerLayoutContext {
    /// Numeric node that owns this block formatting context.
    node_id: NodeId,
    /// Whether this pass measures the box or commits final fragments.
    run_mode: RunMode,
    /// Used logical border-box inline size of the container.
    outer_inline_size: f32,
    /// Definite logical block size available for descendant percentages.
    percentage_resolution_block_size: Option<f32>,
    /// Definite logical block size available for relative percentage insets.
    relative_inset_percentage_resolution_block_size: Option<f32>,
    /// Padding, border, and scrollbar inset around the content box.
    content_box_inset: LogicalBoxStrut<f32>,
    /// Leading border and scrollbar inset excluded from scrollable overflow.
    scroll_origin_inset: LogicalBoxStrut<f32>,
    /// Inline alignment inherited by anonymous block content.
    text_align: TextAlign,
    /// Writing mode and inline direction that own this formatting context.
    writing_direction: WritingDirection,
    /// Whether block-start/end margins may collapse with children.
    own_margins_collapse_with_children: Line<bool>,
    /// Child margins omitted while resolving the CSS `stretch` keyword.
    ignore_stretch_margins: LogicalBoxStrut<bool>,
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
        node_id,
        run_mode,
        outer_inline_size: container_outer_inline_size,
        percentage_resolution_block_size: container_percentage_resolution_block_size,
        relative_inset_percentage_resolution_block_size,
        content_box_inset,
        scroll_origin_inset,
        text_align,
        writing_direction,
        own_margins_collapse_with_children,
        ignore_stretch_margins,
    } = context;
    let writing_mode = writing_direction.mode;
    let direction = writing_direction.direction;
    let container_inner_inline_size = container_outer_inline_size - content_box_inset.inline_axis_sum();
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
            item.static_position = LogicalStaticPosition::new(LogicalOffset {
                inline_offset: content_box_inset.inline_start,
                block_offset: block_offset_for_absolute,
            });
            if run_mode == RunMode::PerformLayout {
                tree.set_out_of_flow_static_position(node_id, item.node_id, item.static_position);
            }
        } else {
            resolve_block_item_final_style(tree, item, parent_size, writing_mode);
            let item_margin =
                writing_direction.to_logical_box_strut(item.margin.map(|margin| {
                    margin.resolve_to_option(margin_percentage_basis, |val, basis| tree.calc(val, basis))
                }));
            let item_non_auto_margin = item_margin.map(|m| m.unwrap_or(0.0));
            let item_non_auto_inline_margin_sum = item_non_auto_margin.inline_axis_sum();
            let stretch_block_margin_sum =
                if ignore_stretch_margins.block_start { 0.0 } else { item_non_auto_margin.block_start }
                    + if ignore_stretch_margins.block_end { 0.0 } else { item_non_auto_margin.block_end };
            let child_available_block_space = container_percentage_resolution_block_size
                .map(|size| AvailableSpace::Definite(f32_max(0.0, size - stretch_block_margin_sum)))
                .unwrap_or(AvailableSpace::MaxContent);

            let scrollbar_size = item.scrollbar_size;

            // Handle floated boxes
            #[cfg(feature = "float_layout")]
            if let Some(float_direction) = item.float.float_direction() {
                has_active_floats = true;

                // A float with an automatic inline size is shrink-to-fit
                // (fit-content) sized against the available inline space.
                let available_inline_size = container_inner_inline_size - item_non_auto_inline_margin_sum;
                let child_available_space = writing_mode.to_physical(LogicalSize {
                    inline_size: AvailableSpace::Definite(available_inline_size),
                    block_size: child_available_block_space,
                });
                let logical_item_size = writing_mode.to_logical(item.size);
                let logical_min_size = writing_mode.to_logical(item.min_size);
                let logical_max_size = writing_mode.to_logical(item.max_size);
                let fitted_inline_size = if logical_item_size.inline_size.is_none() && !item.is_replaced {
                    let intrinsic_known_dimensions = item.size.maybe_clamp(item.min_size, item.max_size);
                    let fitted = fit_content_inline_size(
                        tree,
                        item.node_id,
                        ChildLayoutInput::new(
                            intrinsic_known_dimensions,
                            parent_size,
                            writing_mode,
                            child_available_space,
                            SizingMode::ContentSize,
                            Line::FALSE,
                        ),
                        available_inline_size,
                        writing_mode.inline_axis(),
                    )
                    .maybe_clamp(logical_min_size.inline_size, logical_max_size.inline_size)
                    .max(writing_mode.to_logical(item.padding_border_sum).inline_size);
                    Some(fitted)
                } else {
                    None
                };
                let known_dimensions =
                    writing_mode.to_physical(LogicalSize { inline_size: fitted_inline_size, block_size: None });
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
                    .with_definite_dimensions(known_dimensions)
                    .with_inline_auto_behavior(item.inline_auto_behavior),
                );
                let used_logical_item_size = writing_mode.to_logical(item_layout.size);
                let margin_box = used_logical_item_size + item_non_auto_margin.sum_axes();

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
                        inline_offset: logical_location.inline_offset - scroll_origin_inset.inline_start,
                        block_offset: logical_location.block_offset - scroll_origin_inset.block_start,
                    };
                    let logical_content_size = writing_mode.to_logical(item_layout.content_size);
                    inflow_content_size = inflow_content_size.f32_max(compute_logical_content_size_contribution(
                        contribution_location,
                        used_logical_item_size,
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

            let logical_item_size = writing_mode.to_logical(item.size);
            let logical_min_size = writing_mode.to_logical(item.min_size);
            let logical_max_size = writing_mode.to_logical(item.max_size);
            let child_inputs_for_inline_size = |stretch_inline_size| {
                let child_available_space = writing_mode.to_physical(LogicalSize {
                    inline_size: AvailableSpace::Definite(stretch_inline_size),
                    block_size: child_available_block_space,
                });
                LayoutInput {
                    orthogonal_fallback: crate::tree::OrthogonalFallback::UseInitialContainingBlock,
                    run_mode,
                    sizing_mode: SizingMode::InherentSize,
                    sizing_purpose: SizingPurpose::Layout,
                    axis: RequestedAxis::Both,
                    inline_auto_behavior: item.inline_auto_behavior,
                    block_auto_behavior: AutoSizeBehavior::FitContent,
                    known_dimensions: Size::NONE,
                    definite_dimensions: Size::NONE,
                    parent_size,
                    parent_writing_mode: writing_mode,
                    available_space: child_available_space,
                    block_margins_are_collapsible: Line::FALSE,
                    table_cell: None,
                }
            };
            let (stretch_inline_size, float_avoiding_position, independent_layout) = if item.is_in_same_bfc {
                let stretch_inline_size = container_inner_inline_size - item_non_auto_inline_margin_sum;
                let position = LogicalOffset::ZERO;

                (stretch_inline_size, position, None)
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

                        // Layout against each two-dimensional opportunity in
                        // source order. The inline size can determine the
                        // child's block size through wrapping or aspect-ratio,
                        // so fit cannot be decided before child layout.
                        let opportunities =
                            block_ctx.bfc_layout_opportunities(min_block_offset, line_margins, direction, item.clear);
                        for slot in opportunities {
                            let stretch_inline_size = slot.stretch_width.max(min_auto_inline_size);
                            let item_layout = tree
                                .compute_child_layout(item.node_id, child_inputs_for_inline_size(stretch_inline_size));
                            let final_logical_size = writing_mode.to_logical(item_layout.size);
                            let fits_opportunity = slot.segment_id.is_none()
                                || (final_logical_size.inline_size <= slot.border_width + 0.001
                                    && final_logical_size.block_size <= slot.block_size + 0.001);
                            if !fits_opportunity {
                                continue;
                            }

                            // Moving in the block direction to avoid a float
                            // separates the item's block-start margin from the
                            // parent's collapsing strut.
                            if slot.y > min_block_offset {
                                item_moved_past_float = true;
                            }

                            has_active_floats = slot.segment_id.is_some();
                            item_avoids_floats = true;
                            break 'block (
                                stretch_inline_size,
                                logical_from_bfc_offset(
                                    BfcOffset { line_offset: slot.x, block_offset: slot.y },
                                    slot.border_width,
                                    container_outer_inline_size,
                                    direction,
                                ),
                                Some(item_layout),
                            );
                        }

                        unreachable!("BFC opportunities always include the unrestricted space below floats");
                    }

                    if !has_active_floats {
                        let stretch_inline_size = container_inner_inline_size - item_non_auto_inline_margin_sum;
                        break 'block (
                            stretch_inline_size,
                            LogicalOffset {
                                inline_offset: content_box_inset.inline_start,
                                block_offset: min_block_offset,
                            },
                            None,
                        );
                    }

                    unreachable!("One of the above cases will always be hit");
                }
            };

            // The child owns its authored sizes. The parent supplies only the
            // available space and its auto-inline policy; `known_dimensions`
            // remains reserved for sizes actually fixed by a formatting
            // context (for example a Grid area or a flexed main size).
            let anticipated_inline_size = logical_item_size
                .inline_size
                .or_else(|| (!item.inline_auto_behavior.is_fit_content()).then_some(stretch_inline_size))
                .maybe_clamp(logical_min_size.inline_size, logical_max_size.inline_size)
                .unwrap_or(stretch_inline_size);

            let mut inputs = child_inputs_for_inline_size(stretch_inline_size);
            inputs.block_margins_are_collapsible = if item.is_in_same_bfc { Line::TRUE } else { Line::FALSE };

            #[cfg(feature = "float_layout")]
            let clear_threshold = block_ctx.cleared_threshold(item.clear);
            #[cfg(feature = "float_layout")]
            let clear_pos = clear_threshold.unwrap_or(f32::NEG_INFINITY);
            #[cfg(not(feature = "float_layout"))]
            let clear_pos = f32::NEG_INFINITY;

            let item_layout = if item.is_in_same_bfc {
                // TODO: account for auto margins
                let inset_start = item_non_auto_margin.inline_start + content_box_inset.inline_start;
                let inset_end = container_outer_inline_size - anticipated_inline_size - inset_start;
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
                independent_layout.unwrap_or_else(|| tree.compute_child_layout(item.node_id, inputs))
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
            // Placement consumes collapsed block-axis margin struts, but the
            // public Layout result represents this box's own used margins.
            // Descendant margins that collapse through the box must not leak
            // into CSSOM-facing geometry. Block-axis auto margins resolve to
            // zero; inline-axis auto margins retain their distributed space.
            let used_margin = writing_direction.to_physical_box_strut(LogicalBoxStrut {
                inline_start: resolved_logical_margin.inline_start,
                inline_end: resolved_logical_margin.inline_end,
                block_start: item_margin.block_start.unwrap_or(0.0),
                block_end: item_margin.block_end.unwrap_or(0.0),
            });

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
            let static_offset = if item.is_in_same_bfc {
                let uncleared_block_offset = committed_block_offset + active_collapsible_margin_set.resolve();
                LogicalOffset {
                    inline_offset: content_box_inset.inline_start,
                    block_offset: uncleared_block_offset.max(clear_pos),
                }
            } else {
                float_avoiding_position
            };
            item.static_position = LogicalStaticPosition::new(static_offset);
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

            // A block container's first baseline is the first baseline of its first in-flow child
            // that has one.
            if first_baseline.is_none() {
                first_baseline = logical_block_baseline(item_layout.first_baselines, final_size, writing_direction)
                    .map(|baseline| logical_location.block_offset + baseline);
            }

            // CSS inline-block baseline propagation walks normal-flow block
            // descendants. Block-layout children contribute their last
            // baseline; other formatting contexts contribute their first.
            // A scroll-container block instead forces synthesis at its
            // block-end margin edge (CSS2 10.8 / CSS Inline 3).
            if !item.is_table {
                let child_baseline = if item.uses_block_layout && !item.is_replaced {
                    if item.overflow.x.is_scroll_container() || item.overflow.y.is_scroll_container() {
                        Some(final_logical_size.block_size + resolved_logical_margin.block_end)
                    } else {
                        logical_block_baseline(item_layout.last_baselines, final_size, writing_direction)
                    }
                } else {
                    logical_block_baseline(item_layout.first_baselines, final_size, writing_direction)
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
                    margin: used_margin,
                },
                logical_offset: logical_location,
                participates_in_align_content: true,
            });

            #[cfg(feature = "content_size")]
            {
                let contribution_location = LogicalOffset {
                    inline_offset: logical_location.inline_offset - scroll_origin_inset.inline_start,
                    block_offset: logical_location.block_offset - scroll_origin_inset.block_start,
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

/// Lay out positioned children through the shared out-of-flow resolver.
fn perform_absolute_layout_on_absolute_children(
    tree: &mut impl LayoutBlockContainer,
    containing_block_node_id: NodeId,
    items: &[BlockItem],
    area_size: Size<f32>,
    area_offset: Point<f32>,
    writing_direction: WritingDirection,
    containing_outer_size: Size<f32>,
) -> Size<f32> {
    let containing_block =
        OutOfFlowContainingBlock { outer_size: containing_outer_size, area_offset, area_size, writing_direction };
    let mut content_size = Size::ZERO;

    for item in items.iter().filter(|item| item.position == Position::Absolute) {
        if !tree.is_out_of_flow_containing_block(containing_block_node_id, item.node_id) {
            continue;
        }
        let containing_block =
            tree.get_out_of_flow_containing_block(containing_block_node_id, item.node_id, containing_block);
        let static_position = tree
            .get_out_of_flow_static_position(
                containing_block_node_id,
                item.node_id,
                containing_block.outer_size,
                containing_block.writing_direction,
            )
            .unwrap_or(item.static_position);
        if let Some(output) = layout_out_of_flow_item(
            tree,
            OutOfFlowItem { node: item.node_id, order: item.order, static_position },
            containing_block,
        ) {
            content_size = content_size.f32_max(output.content_size);
        }
    }

    content_size
}
