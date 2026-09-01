//! Contains GridItem used to represent a single grid item during layout
use super::GridTrack;
use crate::compute::common::alignment::resolve_self_alignment;
use crate::compute::common::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use crate::compute::common::baseline::{determine_baseline_group, determine_baseline_writing_mode, BaselineGroup};
use crate::compute::common::intrinsic_size::resolve_intrinsic_axis_size;
use crate::compute::grid::OriginZeroLine;
use crate::geometry::AbstractAxis;
use crate::geometry::{InBothAbstractAxis, Line, LogicalSize, Rect, Size};
use crate::style::{
    AlignItems, AlignSelf, AvailableSpace, Dimension, LengthPercentageAuto, Overflow, ResolvedAspectRatio,
};
use crate::tree::{ChildLayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{AutoSizeBehavior, BoxSizing, GridItemStyle, LengthPercentage, WritingDirection, WritingMode};
use core::ops::Range;

/// The baseline coordinate system and sharing group for one grid axis.
///
/// Grid baseline alignment is independent in the inline and block axes. The
/// context is resolved once from the container and item writing modes, then
/// shared by track sizing, final alignment, and baseline propagation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::compute::grid) struct GridBaselineContext {
    /// Writing mode in which the child's fragment baseline is interpreted.
    pub writing_mode: WritingMode,
    /// The start-side (major) or end-side (minor) sharing group.
    pub group: BaselineGroup,
}

/// Shared track baseline used to position a final grid item fragment.
///
/// Intrinsic sizing uses `GridItem::baseline_shim`; final placement instead
/// compares the final fragment against this stable track metric.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::compute::grid) struct GridBaselineAlignment {
    /// Writing mode in which the final fragment baseline is requested.
    pub writing_mode: WritingMode,
    /// Start-side or end-side baseline-sharing group.
    pub group: BaselineGroup,
    /// Greatest baseline distance stored by the selected grid track.
    pub track_baseline: f32,
}

impl GridBaselineContext {
    /// Resolve the baseline coordinate system and sharing group for one item
    /// alignment axis.
    fn resolve(
        container: WritingDirection,
        child_writing_mode: WritingMode,
        is_parallel_context: bool,
        alignment: AlignSelf,
    ) -> Self {
        let writing_mode = determine_baseline_writing_mode(container, child_writing_mode, is_parallel_context);
        let group =
            determine_baseline_group(container, writing_mode, is_parallel_context, alignment.is_last_baseline(), false);
        Self { writing_mode, group }
    }
}

/// The authored sizing source Grid uses for an item's minimum contribution.
///
/// A numeric used size cannot answer this question: percentages and
/// `aspect-ratio` transfers can resolve an authored `auto` size before track
/// sizing asks for its contribution. Keep the source decision typed so the
/// Grid algorithm does not infer provenance from a resolved value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MinimumContributionSource {
    /// Substitute the used minimum size for a containing-block-dependent
    /// preferred size.
    UsedMinimum,
    /// Measure the ordinary min-content contribution for a definite or
    /// min-content preferred size.
    MinContent,
    /// Measure the max-content contribution for a max-content preferred size.
    MaxContent,
}

/// The border-box part of a Grid item's minimum contribution and the rule
/// that may clamp its complete outer contribution.
///
/// The fixed-track clamp applies only to a content-based automatic minimum.
/// Keeping that provenance beside the cached measurement prevents authored
/// minimums from being clamped merely because they have the same numeric size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in super::super) struct GridItemMinimumContribution {
    /// The measured contribution through the item's border-box edge.
    border_box_size: f32,
    /// Whether and how a definite spanned-track maximum may clamp the value.
    clamp: MinimumContributionClamp,
}

/// Track-dependent clamping selected from the authored sizing source.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MinimumContributionClamp {
    /// The contribution comes from an authored size or an ordinary zero
    /// minimum and is not limited by the spanned tracks.
    None,
    /// Clamp the content-based automatic minimum to the definite maximum of
    /// the spanned tracks, without crossing the item's outer inset floor.
    FixedTrackMaximum {
        /// The item's padding and border sum in the measured axis.
        border_padding_floor: f32,
    },
}

impl GridItemMinimumContribution {
    /// Construct a contribution that is independent of track maxima.
    #[inline(always)]
    fn unclamped(border_box_size: f32) -> Self {
        Self { border_box_size, clamp: MinimumContributionClamp::None }
    }

    /// Construct a content-based automatic minimum with its inset floor.
    #[inline(always)]
    fn content_based(border_box_size: f32, border_padding_floor: f32) -> Self {
        Self { border_box_size, clamp: MinimumContributionClamp::FixedTrackMaximum { border_padding_floor } }
    }

    /// Resolve the complete outer contribution after margins, baseline shims,
    /// and the definite maximum of the spanned tracks are known.
    #[inline(always)]
    pub fn outer_size(self, margin_axis_sum: f32, fixed_track_maximum: impl FnOnce() -> Option<f32>) -> f32 {
        let outer_size = self.border_box_size + margin_axis_sum;
        let clamped_size = match self.clamp {
            MinimumContributionClamp::FixedTrackMaximum { border_padding_floor } => fixed_track_maximum()
                .map(|maximum| outer_size.min(maximum.max(margin_axis_sum + border_padding_floor)))
                .unwrap_or(outer_size),
            MinimumContributionClamp::None => outer_size,
        };
        clamped_size.max(0.0)
    }
}

/// Represents a single grid item
#[derive(Debug)]
pub(in super::super) struct GridItem {
    /// The id of the node that this item represents
    pub node: NodeId,

    /// Logical axes and progression directions of the grid container that
    /// establishes this item's containing block.
    pub parent_writing_direction: WritingDirection,

    /// The order of the item in the children array
    ///
    /// We sort the list of grid items during track sizing. This field allows us to sort back the original order
    /// for final positioning
    pub source_order: u16,

    /// The item's definite row-start and row-end, as resolved by the placement algorithm
    /// (in origin-zero coordinates)
    pub row: Line<OriginZeroLine>,
    /// The items definite column-start and column-end, as resolved by the placement algorithm
    /// (in origin-zero coordinates)
    pub column: Line<OriginZeroLine>,

    /// Is it a compressible replaced element?
    /// https://drafts.csswg.org/css-sizing-3/#min-content-zero
    pub is_compressible_replaced: bool,
    /// The item's overflow style
    pub overflow: LogicalSize<Overflow>,
    /// The item's box_sizing style
    pub box_sizing: BoxSizing,
    /// The item's size style
    pub size: Size<Dimension>,
    /// The item's min_size style
    pub min_size: Size<Dimension>,
    /// The item's max_size style
    pub max_size: Size<Dimension>,
    /// The used aspect ratio and the CSS sizing box that it constrains.
    pub aspect_ratio: Option<ResolvedAspectRatio>,
    /// The item's padding style
    pub padding: Rect<LengthPercentage>,
    /// The item's border style
    pub border: Rect<LengthPercentage>,
    /// The item's margin style
    pub margin: Rect<LengthPercentageAuto>,
    /// The item's align_self property, or the parent's align_items property is not set
    pub align_self: AlignSelf,
    /// The item's justify_self property, or the parent's justify_items property is not set
    pub justify_self: AlignSelf,
    /// Baseline coordinate systems and sharing groups for column/row alignment.
    pub baseline_context: InBothAbstractAxis<GridBaselineContext>,
    /// The selected baseline measured for each alignment axis. These are
    /// separate from the baselines retained from final item layout.
    pub alignment_baseline: InBothAbstractAxis<Option<f32>>,
    /// Temporary baseline shims included in intrinsic size contributions. Final
    /// placement reads the shared metric from the selected track instead.
    pub baseline_shim: InBothAbstractAxis<f32>,
    /// Used alignment when a synthesized baseline would make intrinsic track
    /// sizing cyclic. `None` preserves the authored baseline alignment.
    pub baseline_fallback: InBothAbstractAxis<Option<AlignSelf>>,

    /// The item's definite row-start and row-end (same as `row` field, except in a different coordinate system)
    /// (as indexes into the Vec<GridTrack> stored in a grid's AbstractAxisTracks)
    pub row_indexes: Line<u16>,
    /// The items definite column-start and column-end (same as `column` field, except in a different coordinate system)
    /// (as indexes into the Vec<GridTrack> stored in a grid's AbstractAxisTracks)
    pub column_indexes: Line<u16>,

    /// Whether the item crosses a flexible row
    pub crosses_flexible_row: bool,
    /// Whether the item crosses a flexible column
    pub crosses_flexible_column: bool,
    /// Whether the item crosses a intrinsic row
    pub crosses_intrinsic_row: bool,
    /// Whether the item crosses a intrinsic column
    pub crosses_intrinsic_column: bool,

    // Caches for intrinsic size computation. These caches are only valid for a single run of the track-sizing algorithm.
    /// Cache for the known_dimensions input to intrinsic sizing computation
    pub grid_area_size_cache: Option<Size<Option<f32>>>,
    /// Cache for the min-content size
    pub min_content_contribution_cache: LogicalSize<Option<f32>>,
    /// Cache for the minimum contribution
    pub minimum_contribution_cache: LogicalSize<Option<GridItemMinimumContribution>>,
    /// Cache for the max-content size
    pub max_content_contribution_cache: LogicalSize<Option<f32>>,
    /// Whether an intrinsic item contribution observed a dependency on the
    /// grid area's block-size.
    pub depends_on_block_constraints: bool,

    /// Normal-flow logical block offset used to propagate the container's baseline.
    pub baseline_block_offset: f32,
    /// Final logical block size. Used to synthesize a missing baseline.
    pub block_size: f32,
    /// First logical block-axis baseline from the item's final layout.
    pub first_baseline: Option<f32>,
    /// Last logical block-axis baseline from the item's final layout.
    pub last_baseline: Option<f32>,
}

impl GridItem {
    /// Create a new item given a concrete placement in both axes
    pub fn new_with_placement_style_and_order<S: GridItemStyle>(
        node: NodeId,
        parent_writing_direction: WritingDirection,
        placement: InBothAbstractAxis<Line<OriginZeroLine>>,
        style: S,
        parent_alignment: InBothAbstractAxis<AlignItems>,
        source_order: u16,
    ) -> Self {
        let align_self = style.align_self().unwrap_or(parent_alignment.block);
        let justify_self = style.justify_self().unwrap_or(parent_alignment.inline);
        GridItem {
            node,
            parent_writing_direction,
            source_order,
            row: placement.block,
            column: placement.inline,
            is_compressible_replaced: style.is_compressible_replaced(),
            overflow: {
                let overflow = style.overflow();
                parent_writing_direction.mode.to_logical(Size { width: overflow.x, height: overflow.y })
            },
            box_sizing: style.box_sizing(),
            size: style.size(),
            min_size: style.min_size(),
            max_size: style.max_size(),
            aspect_ratio: style.aspect_ratio().and_then(|ratio| ResolvedAspectRatio::new(ratio, style.box_sizing())),
            padding: style.padding(),
            border: style.border(),
            margin: style.margin(),
            align_self,
            justify_self,
            baseline_context: InBothAbstractAxis {
                inline: GridBaselineContext::resolve(
                    parent_writing_direction,
                    parent_writing_direction.mode,
                    false,
                    justify_self,
                ),
                block: GridBaselineContext::resolve(
                    parent_writing_direction,
                    parent_writing_direction.mode,
                    true,
                    align_self,
                ),
            },
            alignment_baseline: InBothAbstractAxis { inline: None, block: None },
            baseline_shim: InBothAbstractAxis { inline: 0.0, block: 0.0 },
            baseline_fallback: InBothAbstractAxis { inline: None, block: None },
            row_indexes: Line { start: 0, end: 0 }, // Properly initialised later
            column_indexes: Line { start: 0, end: 0 }, // Properly initialised later
            crosses_flexible_row: false,            // Properly initialised later
            crosses_flexible_column: false,         // Properly initialised later
            crosses_intrinsic_row: false,           // Properly initialised later
            crosses_intrinsic_column: false,        // Properly initialised later
            grid_area_size_cache: None,
            min_content_contribution_cache: LogicalSize { inline_size: None, block_size: None },
            max_content_contribution_cache: LogicalSize { inline_size: None, block_size: None },
            minimum_contribution_cache: LogicalSize { inline_size: None, block_size: None },
            depends_on_block_constraints: false,
            baseline_block_offset: 0.0,
            block_size: 0.0,
            first_baseline: None,
            last_baseline: None,
        }
    }

    /// Resolve both baseline contexts from the item's inherited writing mode.
    /// The layout tree owns that node-level state; the numeric Grid style
    /// projection is not an authoritative substitute.
    pub fn resolve_baseline_context(&mut self, child_writing_mode: WritingMode) {
        self.baseline_context = InBothAbstractAxis {
            inline: GridBaselineContext::resolve(
                self.parent_writing_direction,
                child_writing_mode,
                false,
                self.justify_self,
            ),
            block: GridBaselineContext::resolve(
                self.parent_writing_direction,
                child_writing_mode,
                true,
                self.align_self,
            ),
        };
    }

    /// Return the shared alignment context used by this item on `axis`.
    /// Major baselines participate in the start-most spanned track and minor
    /// baselines in the end-most one, after mapping logical flow to the
    /// physical-low-to-high track vectors.
    pub fn baseline_sharing_track_index(&self, axis: AbstractAxis) -> usize {
        let span = self.placement_indexes(axis);
        let group = self.baseline_context.get(axis).group;
        let logical_start_is_physical_low = !self.parent_writing_direction.is_logical_axis_reversed(axis);
        if (group == BaselineGroup::Major) == logical_start_is_physical_low {
            usize::from(span.start) + 1
        } else {
            usize::from(span.end.saturating_sub(1))
        }
    }

    /// Resolve the final baseline-alignment input from the selected track.
    pub fn final_baseline_alignment(&self, axis: AbstractAxis, tracks: &[GridTrack]) -> Option<GridBaselineAlignment> {
        if !self.participates_in_baseline_alignment(axis) {
            return None;
        }
        let context = self.baseline_context.get(axis);
        let track_baseline = tracks.get(self.baseline_sharing_track_index(axis))?.baseline(context.group)?;
        Some(GridBaselineAlignment { writing_mode: context.writing_mode, group: context.group, track_baseline })
    }

    /// Return the authored self-alignment in one logical Grid axis.
    pub fn alignment(&self, axis: AbstractAxis) -> AlignSelf {
        match axis {
            AbstractAxis::Inline => self.justify_self,
            AbstractAxis::Block => self.align_self,
        }
    }

    /// Return the alignment after applying Grid's cyclic baseline fallback.
    pub fn used_alignment(&self, axis: AbstractAxis) -> AlignSelf {
        self.baseline_fallback.get(axis).unwrap_or_else(|| self.alignment(axis))
    }

    /// Whether an in-flow auto margin suppresses self-alignment in `axis`.
    fn has_auto_margin(&self, axis: AbstractAxis) -> bool {
        let margin = self.parent_writing_direction.to_logical_box_strut(self.margin);
        match axis {
            AbstractAxis::Inline => margin.inline_start.is_auto() || margin.inline_end.is_auto(),
            AbstractAxis::Block => margin.block_start.is_auto() || margin.block_end.is_auto(),
        }
    }

    /// Whether this item requests baseline alignment after auto-margin
    /// precedence is applied, but before cyclic sizing fallback.
    pub fn specifies_baseline_alignment(&self, axis: AbstractAxis) -> bool {
        self.alignment(axis).is_baseline() && !self.has_auto_margin(axis)
    }

    /// Whether this item participates in a resolved baseline-sharing group.
    pub fn participates_in_baseline_alignment(&self, axis: AbstractAxis) -> bool {
        self.used_alignment(axis).is_baseline() && !self.has_auto_margin(axis)
    }

    /// Update Grid's cyclic baseline fallback after measuring the fragment.
    ///
    /// A synthesized baseline cannot participate when the item's size in the
    /// alignment axis depends on an intrinsically-sized track. Flexible tracks
    /// count as intrinsic only while that Grid axis remains indefinite.
    pub fn resolve_baseline_fallback(
        &mut self,
        axis: AbstractAxis,
        child_writing_mode: WritingMode,
        has_synthesized_baseline: bool,
        axis_size_is_definite: bool,
    ) {
        let spans_content_sized_track =
            self.crosses_intrinsic_track(axis) || (!axis_size_is_definite && self.crosses_flexible_track(axis));
        let child_is_parallel = !self.parent_writing_direction.mode.is_orthogonal_to(child_writing_mode);
        let size_axis_is_child_block = child_is_parallel == (axis == AbstractAxis::Block);
        let logical_size = child_writing_mode.to_logical(self.size);
        let logical_min_size = child_writing_mode.to_logical(self.min_size);
        let logical_max_size = child_writing_mode.to_logical(self.max_size);
        let sizes = if size_axis_is_child_block {
            [logical_size.block_size, logical_min_size.block_size, logical_max_size.block_size]
        } else {
            [logical_size.inline_size, logical_min_size.inline_size, logical_max_size.inline_size]
        };
        let size_depends_on_track =
            sizes.into_iter().any(|size| size.may_have_percentage_dependence() || size.is_stretch());
        let fallback = if has_synthesized_baseline && spans_content_sized_track && size_depends_on_track {
            Some(match self.baseline_context.get(axis).group {
                BaselineGroup::Major => AlignSelf::START,
                BaselineGroup::Minor => AlignSelf::END,
            })
        } else {
            None
        };
        *self.baseline_fallback.get_mut(axis) = fallback;
    }

    /// This item's placement in the specified axis in OriginZero coordinates
    pub fn placement(&self, axis: AbstractAxis) -> Line<OriginZeroLine> {
        match axis {
            AbstractAxis::Block => self.row,
            AbstractAxis::Inline => self.column,
        }
    }

    /// This item's placement in the specified axis as GridTrackVec indices
    pub fn placement_indexes(&self, axis: AbstractAxis) -> Line<u16> {
        match axis {
            AbstractAxis::Block => self.row_indexes,
            AbstractAxis::Inline => self.column_indexes,
        }
    }

    /// Returns a range which can be used as an index into the GridTrackVec in the specified axis
    /// which will produce a sub-slice of covering all the tracks and lines that this item spans
    /// excluding the lines that bound it.
    pub fn track_range_excluding_lines(&self, axis: AbstractAxis) -> Range<usize> {
        let indexes = self.placement_indexes(axis);
        (indexes.start as usize + 1)..(indexes.end as usize)
    }

    /// Returns the number of tracks that this item spans in the specified axis
    pub fn span(&self, axis: AbstractAxis) -> u16 {
        match axis {
            AbstractAxis::Block => self.row.span(),
            AbstractAxis::Inline => self.column.span(),
        }
    }

    /// Returns the pre-computed value indicating whether the grid item crosses a flexible track in
    /// the specified axis
    pub fn crosses_flexible_track(&self, axis: AbstractAxis) -> bool {
        match axis {
            AbstractAxis::Inline => self.crosses_flexible_column,
            AbstractAxis::Block => self.crosses_flexible_row,
        }
    }

    /// Returns the pre-computed value indicating whether the grid item crosses an intrinsic track in
    /// the specified axis
    pub fn crosses_intrinsic_track(&self, axis: AbstractAxis) -> bool {
        match axis {
            AbstractAxis::Inline => self.crosses_intrinsic_column,
            AbstractAxis::Block => self.crosses_intrinsic_row,
        }
    }

    /// Select the minimum-contribution algorithm from the authored preferred
    /// size, before percentage resolution or aspect-ratio transfer.
    #[inline]
    fn minimum_contribution_source(&self, axis: AbstractAxis) -> MinimumContributionSource {
        let preferred_size = self.parent_writing_direction.mode.to_logical(self.size);
        let preferred = match axis {
            AbstractAxis::Inline => preferred_size.inline_size,
            AbstractAxis::Block => preferred_size.block_size,
        };
        let depends_on_containing_block =
            preferred.may_have_percentage_dependence() || preferred.is_fit_content() || preferred.is_stretch();

        if preferred.is_auto() || depends_on_containing_block {
            MinimumContributionSource::UsedMinimum
        } else if preferred.is_max_content() {
            MinimumContributionSource::MaxContent
        } else {
            MinimumContributionSource::MinContent
        }
    }

    /// Resolve an authored intrinsic `min-size` as the border-box value used
    /// by Grid's minimum-contribution algorithm.
    fn resolve_intrinsic_minimum_size(
        &mut self,
        tree: &mut impl LayoutPartialTree,
        physical_axis: crate::AbsoluteAxis,
        grid_area_size: Size<Option<f32>>,
    ) -> Size<Option<f32>> {
        let authored_minimum = self.min_size.get_abs(physical_axis);
        if !authored_minimum.is_intrinsic() && !authored_minimum.is_stretch() {
            return Size::NONE;
        }

        let available_space =
            grid_area_size.map(|size| size.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MaxContent));
        let (inline_auto_behavior, block_auto_behavior) = self.intrinsic_auto_size_behaviors(tree);
        let intrinsic_minimum = resolve_intrinsic_axis_size(
            tree,
            self.node,
            ChildLayoutInput::new(
                Size::NONE,
                grid_area_size,
                self.parent_writing_direction.mode,
                available_space,
                SizingMode::ContentSize,
                Line::FALSE,
            )
            .with_inline_auto_behavior(inline_auto_behavior)
            .with_block_auto_behavior(block_auto_behavior)
            .without_orthogonal_fallback(),
            authored_minimum,
            available_space.get_abs(physical_axis),
            physical_axis,
        );
        self.depends_on_block_constraints |= intrinsic_minimum.depends_on_block_constraints;

        match physical_axis {
            crate::AbsoluteAxis::Horizontal => Size { width: intrinsic_minimum.value, height: None },
            crate::AbsoluteAxis::Vertical => Size { width: None, height: intrinsic_minimum.value },
        }
    }

    /// Resolve the Grid-owned auto-size policy for an intrinsic item probe.
    ///
    /// Authored preferred/min/max sizes remain child-owned. In particular,
    /// they must not be materialized into `known_dimensions`, whose axes mean
    /// that the parent formatting context fixed an exact used size. The child
    /// combines this policy with the grid area and its own sizing properties
    /// at the normal node-sizing boundary.
    fn intrinsic_auto_size_behaviors(&self, tree: &impl LayoutPartialTree) -> (AutoSizeBehavior, AutoSizeBehavior) {
        let normal_auto_size = if self.is_compressible_replaced {
            AutoSizeBehavior::FitContent
        } else {
            AutoSizeBehavior::StretchImplicit
        };
        let (horizontal_alignment, vertical_alignment) = if self.parent_writing_direction.mode.is_horizontal() {
            (self.justify_self, self.align_self)
        } else {
            (self.align_self, self.justify_self)
        };
        let mut horizontal = resolve_self_alignment(horizontal_alignment, AlignSelf::START, normal_auto_size).auto_size;
        let mut vertical = resolve_self_alignment(vertical_alignment, AlignSelf::START, normal_auto_size).auto_size;
        if self.margin.left.is_auto() || self.margin.right.is_auto() {
            horizontal = AutoSizeBehavior::FitContent;
        }
        if self.margin.top.is_auto() || self.margin.bottom.is_auto() {
            vertical = AutoSizeBehavior::FitContent;
        }

        match tree.get_writing_mode(self.node).inline_axis() {
            crate::AbsoluteAxis::Horizontal => (horizontal, vertical),
            crate::AbsoluteAxis::Vertical => (vertical, horizontal),
        }
    }

    /// Whether `min-size: auto` uses this item's content-based minimum in the
    /// selected grid axis.
    ///
    /// Both track predicates are deliberately scoped to this item's span. A
    /// flexible or auto-min track elsewhere in the grid cannot affect the
    /// item's automatic minimum.
    #[inline]
    fn uses_content_based_automatic_minimum(&self, axis: AbstractAxis, axis_tracks: &[GridTrack]) -> bool {
        let spanned_tracks = &axis_tracks[self.track_range_excluding_lines(axis)];
        let spans_auto_min_track = spanned_tracks.iter().any(|track| track.min_track_sizing_function.is_auto());
        let spans_multiple_tracks = self.span(axis) > 1;

        spans_auto_min_track && (!spans_multiple_tracks || !self.crosses_flexible_track(axis))
    }

    /// For an item spanning multiple tracks, the upper limit used to calculate its limited min-/max-content contribution is the
    /// sum of the fixed max track sizing functions of any tracks it spans, and is applied if it only spans such tracks.
    pub fn spanned_track_limit(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        axis_parent_size: Option<f32>,
        resolve_calc_value: &dyn Fn(*const (), f32) -> f32,
    ) -> Option<f32> {
        let spanned_tracks = &axis_tracks[self.track_range_excluding_lines(axis)];
        let tracks_all_fixed = spanned_tracks.iter().all(|track| {
            track.max_track_sizing_function.definite_limit(axis_parent_size, resolve_calc_value).is_some()
        });
        if tracks_all_fixed {
            let limit: f32 = spanned_tracks
                .iter()
                .map(|track| {
                    track.max_track_sizing_function.definite_limit(axis_parent_size, resolve_calc_value).unwrap()
                })
                .sum();
            Some(limit)
        } else {
            None
        }
    }

    /// Similar to the spanned_track_limit, but excludes FitContent arguments from the limit.
    /// Used to clamp the automatic minimum contributions of an item
    pub fn spanned_fixed_track_limit(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        axis_parent_size: Option<f32>,
        resolve_calc_value: &dyn Fn(*const (), f32) -> f32,
    ) -> Option<f32> {
        let spanned_tracks = &axis_tracks[self.track_range_excluding_lines(axis)];
        let tracks_all_fixed = spanned_tracks.iter().all(|track| {
            track.max_track_sizing_function.definite_value(axis_parent_size, resolve_calc_value).is_some()
        });
        if tracks_all_fixed {
            let limit: f32 = spanned_tracks
                .iter()
                .map(|track| {
                    track.max_track_sizing_function.definite_value(axis_parent_size, resolve_calc_value).unwrap()
                })
                .sum();
            Some(limit)
        } else {
            None
        }
    }

    /// Returns the grid area's size in the specified axis when every spanned track has a definite fixed size.
    ///
    /// During intrinsic sizing, percentages on grid items resolve against the size of the grid area,
    /// not the grid container. If the spanned tracks in an axis are not all definite yet, the grid
    /// area is still indefinite in that axis and percentage-dependent values must stay unresolved here.
    ///
    /// Spec:
    /// https://www.w3.org/TR/css-grid-1/#grid-item-sizing
    /// https://www.w3.org/TR/css-grid-1/#algo-overview
    ///
    /// Compute the available_space to be passed to the child sizing functions
    /// These are estimates based on either the max track sizing function or the provisional base size in the opposite
    /// axis to the one currently being sized.
    /// https://www.w3.org/TR/css-grid-1/#algo-overview
    pub fn grid_area_size(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        other_axis_tracks: &[GridTrack],
        available_space: LogicalSize<Option<f32>>,
        get_track_size_estimate: impl Fn(&GridTrack, Option<f32>) -> Option<f32>,
        resolve_calc_value: &impl Fn(*const (), f32) -> f32,
    ) -> Size<Option<f32>> {
        let mut size = LogicalSize { inline_size: None, block_size: None };
        size.set(
            axis,
            axis_tracks[self.track_range_excluding_lines(axis)]
                .iter()
                .map(|track| {
                    let min_size = track
                        .min_track_sizing_function
                        .definite_value(available_space.get(axis), resolve_calc_value)?;
                    let max_size = track
                        .max_track_sizing_function
                        .definite_value(available_space.get(axis), resolve_calc_value)?;

                    if min_size == max_size {
                        Some(track.base_size)
                    } else {
                        None
                    }
                })
                .sum::<Option<f32>>(),
        );

        size.set(
            axis.other(),
            other_axis_tracks[self.track_range_excluding_lines(axis.other())]
                .iter()
                .map(|track| {
                    get_track_size_estimate(track, available_space.get(axis.other()))
                        .map(|size| size + track.content_alignment_adjustment)
                })
                .sum::<Option<f32>>(),
        );

        self.parent_writing_direction.mode.to_physical(size)
    }

    /// Retrieve the available_space from the cache or compute them using the passed parameters
    pub fn grid_area_size_cached(
        &mut self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        other_axis_tracks: &[GridTrack],
        available_space: LogicalSize<Option<f32>>,
        get_track_size_estimate: impl Fn(&GridTrack, Option<f32>) -> Option<f32>,
        resolve_calc_value: &impl Fn(*const (), f32) -> f32,
    ) -> Size<Option<f32>> {
        self.grid_area_size_cache.unwrap_or_else(|| {
            let grid_area_size = self.grid_area_size(
                axis,
                axis_tracks,
                other_axis_tracks,
                available_space,
                get_track_size_estimate,
                resolve_calc_value,
            );
            self.grid_area_size_cache = Some(grid_area_size);
            grid_area_size
        })
    }

    /// Compute the item's resolved margins for size contributions. Inline-axis percentage margins resolve
    /// to zero while sizing that axis, preventing a cyclic dependency in every writing mode.
    #[inline(always)]
    pub fn margins_axis_sums_with_baseline_shims(
        &self,
        inner_node_inline_size: Option<f32>,
        tree: &impl LayoutPartialTree,
    ) -> LogicalSize<f32> {
        let writing_direction = self.parent_writing_direction;
        let logical_margin = writing_direction.to_logical_box_strut(self.margin);
        let mut resolved_logical_margin = crate::geometry::LogicalBoxStrut {
            inline_start: logical_margin.inline_start.resolve_or_zero(Some(0.0), |val, basis| tree.calc(val, basis)),
            inline_end: logical_margin.inline_end.resolve_or_zero(Some(0.0), |val, basis| tree.calc(val, basis)),
            block_start: logical_margin
                .block_start
                .resolve_or_zero(inner_node_inline_size, |val, basis| tree.calc(val, basis)),
            block_end: logical_margin
                .block_end
                .resolve_or_zero(inner_node_inline_size, |val, basis| tree.calc(val, basis)),
        };
        match self.baseline_context.inline.group {
            BaselineGroup::Major => resolved_logical_margin.inline_start += self.baseline_shim.inline,
            BaselineGroup::Minor => resolved_logical_margin.inline_end += self.baseline_shim.inline,
        }
        match self.baseline_context.block.group {
            BaselineGroup::Major => resolved_logical_margin.block_start += self.baseline_shim.block,
            BaselineGroup::Minor => resolved_logical_margin.block_end += self.baseline_shim.block,
        }
        LogicalSize {
            inline_size: resolved_logical_margin.inline_start + resolved_logical_margin.inline_end,
            block_size: resolved_logical_margin.block_start + resolved_logical_margin.block_end,
        }
    }

    /// Build the constraint used to measure an intrinsic contribution.
    ///
    /// Min- and max-content constraints describe an item's inline axis. When
    /// the grid track direction maps to the item's block axis, browsers obtain
    /// the contribution by laying the item out with an indefinite block size;
    /// in Taffy's constraint model that is represented by `MaxContent`. Using
    /// `MinContent` in the block axis can shrink nested flex and grid formatting
    /// contexts before their block contribution has been measured.
    fn intrinsic_contribution_available_space(
        &mut self,
        tree: &impl LayoutPartialTree,
        axis: AbstractAxis,
        available_space: Size<Option<f32>>,
        inline_constraint: AvailableSpace,
    ) -> Size<AvailableSpace> {
        let track_axis = axis.to_absolute(self.parent_writing_direction.mode);
        let measures_item_inline_axis = track_axis == tree.get_writing_mode(self.node).inline_axis();
        let constraint = if measures_item_inline_axis { inline_constraint } else { AvailableSpace::MaxContent };

        // An orthogonal item's block contribution to a column can change once
        // the grid's block size constrains the item's inline axis. Propagate
        // that dependency so an intrinsic parent measurement is not reused for
        // the final block constraint. This is Blink's
        // `is_sizing_dependent_on_block_size` case in
        // `BlockContributionSize`.
        if axis == AbstractAxis::Inline && !measures_item_inline_axis {
            self.depends_on_block_constraints = true;
        }

        let logical_available_space = self.parent_writing_direction.mode.to_logical(available_space);
        self.parent_writing_direction.mode.to_physical(
            logical_available_space
                .map(|size| size.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MaxContent))
                .with(axis, logical_available_space.get(axis).map(AvailableSpace::Definite).unwrap_or(constraint)),
        )
    }

    /// Compute the item's min content contribution from the provided parameters
    pub fn min_content_contribution(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        let (inline_auto_behavior, block_auto_behavior) = self.intrinsic_auto_size_behaviors(tree);
        // The child sees the grid area as its containing block during intrinsic measurement, so
        // percentage box properties resolve against the grid area when that size is definite.
        // Spec:
        // https://www.w3.org/TR/css-grid-1/#grid-item-sizing
        // https://www.w3.org/TR/css-grid-1/#algo-overview
        let contribution_available_space =
            self.intrinsic_contribution_available_space(tree, axis, available_space, AvailableSpace::MinContent);
        let measured = tree.measure_child_size_with_metadata(
            self.node,
            ChildLayoutInput::new(
                Size::NONE,
                grid_area_size,
                self.parent_writing_direction.mode,
                contribution_available_space,
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .with_inline_auto_behavior(inline_auto_behavior)
            .with_block_auto_behavior(block_auto_behavior)
            .without_orthogonal_fallback(),
            axis.to_absolute(self.parent_writing_direction.mode).into(),
        );
        self.depends_on_block_constraints |= measured.depends_on_block_constraints;
        measured.size.get_abs(axis.to_absolute(self.parent_writing_direction.mode))
    }

    /// Retrieve the item's min content contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn min_content_contribution_cached(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        self.min_content_contribution_cache.get(axis).unwrap_or_else(|| {
            let size = self.min_content_contribution(axis, tree, grid_area_size, available_space);
            self.min_content_contribution_cache.set(axis, Some(size));
            size
        })
    }

    /// Compute the item's max content contribution from the provided parameters
    pub fn max_content_contribution(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        let (inline_auto_behavior, block_auto_behavior) = self.intrinsic_auto_size_behaviors(tree);
        // See the min-content path above. Max-content measurement uses the same containing-block
        // basis so percentage-dependent item geometry is measured from the grid area rather than
        // from the container.
        let contribution_available_space =
            self.intrinsic_contribution_available_space(tree, axis, available_space, AvailableSpace::MaxContent);
        let measured = tree.measure_child_size_with_metadata(
            self.node,
            ChildLayoutInput::new(
                Size::NONE,
                grid_area_size,
                self.parent_writing_direction.mode,
                contribution_available_space,
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .with_inline_auto_behavior(inline_auto_behavior)
            .with_block_auto_behavior(block_auto_behavior)
            .without_orthogonal_fallback(),
            axis.to_absolute(self.parent_writing_direction.mode).into(),
        );
        self.depends_on_block_constraints |= measured.depends_on_block_constraints;
        measured.size.get_abs(axis.to_absolute(self.parent_writing_direction.mode))
    }

    /// Retrieve the item's max content contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn max_content_contribution_cached(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        self.max_content_contribution_cache.get(axis).unwrap_or_else(|| {
            let size = self.max_content_contribution(axis, tree, grid_area_size, available_space);
            self.max_content_contribution_cache.set(axis, Some(size));
            size
        })
    }

    /// The minimum contribution of an item is the smallest outer size it can have.
    /// Specifically:
    ///   - If the item’s computed preferred size behaves as auto or depends on the size of its containing block in the relevant axis:
    ///     Its minimum contribution is the outer size that would result from assuming the item’s used minimum size as its preferred size;
    ///   - Else the item’s minimum contribution is its min-content contribution.
    ///
    /// Because the minimum contribution often depends on the size of the item’s content, it is considered a type of intrinsic size contribution.
    /// See: https://www.w3.org/TR/css-grid-1/#min-size-auto
    pub fn minimum_contribution(
        &mut self,
        tree: &mut impl LayoutPartialTree,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: Size<Option<f32>>,
    ) -> GridItemMinimumContribution {
        match self.minimum_contribution_source(axis) {
            MinimumContributionSource::MinContent => {
                return GridItemMinimumContribution::unclamped(self.min_content_contribution_cached(
                    axis,
                    tree,
                    grid_area_size,
                    grid_area_size,
                ));
            }
            MinimumContributionSource::MaxContent => {
                return GridItemMinimumContribution::unclamped(self.max_content_contribution_cached(
                    axis,
                    tree,
                    grid_area_size,
                    grid_area_size,
                ));
            }
            MinimumContributionSource::UsedMinimum => {}
        }

        let physical_axis = match axis {
            AbstractAxis::Inline => self.parent_writing_direction.mode.inline_axis(),
            AbstractAxis::Block => self.parent_writing_direction.mode.block_axis(),
        };
        let percentage_basis = self.parent_writing_direction.mode.to_logical(grid_area_size).inline_size;
        let padding = self.padding.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let border = self.border.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment =
            if self.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
        // A percentage min-size whose grid-area basis is still indefinite is
        // cyclic during track sizing. CSS Sizing resolves that percentage
        // against zero, while the preferred size remains unresolved until the
        // final grid area is known. This also preserves the fixed component of
        // calc() values.
        let minimum_percentage_basis = grid_area_size.map(|basis| Some(basis.unwrap_or(0.0)));
        let mut resolved_min_size = self
            .min_size
            .maybe_resolve(minimum_percentage_basis, |val, basis| tree.calc(val, basis))
            .maybe_add(box_sizing_adjustment);
        resolved_min_size =
            resolved_min_size.or(self.resolve_intrinsic_minimum_size(tree, physical_axis, grid_area_size));

        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: self
                .size
                .maybe_resolve(grid_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            min_size: resolved_min_size,
            max_size: self
                .max_size
                .maybe_resolve(grid_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            size_is_auto: self.size.map(|dimension| dimension.is_auto()),
            writing_mode: tree.get_writing_mode(self.node),
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: self.aspect_ratio,
            padding_border: padding_border_size,
        });
        if let Some(minimum) = resolved.min_size.get_abs(physical_axis) {
            return GridItemMinimumContribution::unclamped(minimum);
        }

        let overflow = self.overflow.get(axis);
        if let Some(minimum) = overflow.maybe_into_automatic_min_size() {
            return GridItemMinimumContribution::unclamped(minimum);
        }

        // Automatic minimum size. See https://www.w3.org/TR/css-grid-1/#min-size-auto
        if !self.uses_content_based_automatic_minimum(axis, axis_tracks) {
            return GridItemMinimumContribution::unclamped(0.0);
        }

        let mut minimum_contribution = self.min_content_contribution_cached(axis, tree, grid_area_size, grid_area_size);

        // If the item is a compressible replaced element, and has a definite preferred size or maximum size in the
        // relevant axis, the size suggestion is capped by those sizes; for this purpose, any indefinite percentages
        // in these sizes are resolved against zero (and considered definite).
        if self.is_compressible_replaced {
            let size = self.size.get_abs(physical_axis).maybe_resolve(Some(0.0), |val, basis| tree.calc(val, basis));
            let max_size =
                self.max_size.get_abs(physical_axis).maybe_resolve(Some(0.0), |val, basis| tree.calc(val, basis));
            minimum_contribution = minimum_contribution.maybe_min(size).maybe_min(max_size);
        }

        // The content-based automatic minimum is the only contribution that
        // may be clamped by a fixed maximum on the spanned tracks. The clamp
        // is resolved later, once the outer margins and baseline shim are
        // available; its floor is this border and padding sum.
        GridItemMinimumContribution::content_based(minimum_contribution, padding_border_size.get_abs(physical_axis))
    }

    /// Retrieve the item's minimum contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn minimum_contribution_cached(
        &mut self,
        tree: &mut impl LayoutPartialTree,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: Size<Option<f32>>,
    ) -> GridItemMinimumContribution {
        self.minimum_contribution_cache.get(axis).unwrap_or_else(|| {
            let size = self.minimum_contribution(tree, axis, axis_tracks, grid_area_size);
            self.minimum_contribution_cache.set(axis, Some(size));
            size
        })
    }
}
