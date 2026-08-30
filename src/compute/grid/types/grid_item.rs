//! Contains GridItem used to represent a single grid item during layout
use super::GridTrack;
use crate::compute::common::alignment::resolve_self_alignment;
use crate::compute::common::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use crate::compute::common::baseline::{
    determine_baseline_group, determine_baseline_writing_mode, BaselineGroup, FontBaseline,
};
use crate::compute::common::intrinsic_size::{measure_child_intrinsic_contribution, resolve_minimum_size};
use crate::compute::grid::OriginZeroLine;
use crate::geometry::AbstractAxis;
use crate::geometry::{InBothAbstractAxis, Line, LogicalSize, Rect, Size};
use crate::style::{
    AlignItems, AlignSelf, AvailableSpace, Dimension, LengthPercentageAuto, Overflow, ResolvedAspectRatio,
};
use crate::tree::{AutoSizeBehavior, ChildLayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, GridItemStyle, LengthPercentage, WritingDirection, WritingMode};
use core::ops::Range;

/// The baseline coordinate system and sharing group for one grid axis.
///
/// Grid baseline alignment is defined independently for columns and rows.
/// Keeping this metadata on the item lets track sizing and final alignment use
/// the same baseline interpretation instead of inferring it from physical axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::compute::grid) struct GridBaselineContext {
    /// Writing mode in which the child's fragment baseline is interpreted.
    pub writing_mode: WritingMode,
    /// The start-side (major) or end-side (minor) sharing group.
    pub group: BaselineGroup,
}

impl GridBaselineContext {
    /// Resolve one grid-axis baseline context from the container and child
    /// writing modes.
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

/// The sizing input Grid uses to calculate an item's minimum contribution.
///
/// A preferred size whose used value is selected by the containing block must
/// not become definite merely because another axis transferred a value through
/// `aspect-ratio`. In that case Grid substitutes the used minimum size. Other
/// preferred sizes contribute through the ordinary min-content measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MinimumContributionSource {
    /// Substitute the used minimum size for the preferred size.
    UsedMinimum,
    /// Measure the item's ordinary min-content contribution.
    MinContent,
}

/// Represents a single grid item
#[derive(Debug)]
pub(in super::super) struct GridItem {
    /// The id of the node that this item represents
    pub node: NodeId,

    /// Logical axes and progression directions of the grid container that
    /// establishes this item's containing block.
    pub parent_writing_direction: WritingDirection,

    /// Font baseline computed for the parent grid container.
    pub parent_font_baseline: FontBaseline,

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
    pub aspect_ratio: ResolvedAspectRatio,
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
    /// The item's first baseline measured for each alignment axis. These are
    /// separate from the baselines retained from final item layout.
    pub alignment_baseline: InBothAbstractAxis<Option<f32>>,
    /// Baseline shims applied as extra margin toward each sharing-group edge.
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
    pub minimum_contribution_cache: LogicalSize<Option<f32>>,
    /// Cache for the max-content size
    pub max_content_contribution_cache: LogicalSize<Option<f32>>,
    /// Whether an intrinsic item contribution observed a dependency on the
    /// grid area's block-size.
    pub depends_on_block_constraints: bool,

    /// Final logical block offset. Used to propagate the container's baseline.
    pub block_offset: f32,
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
        let align_self = GridItemStyle::align_self(&style).unwrap_or(parent_alignment.block);
        let justify_self = GridItemStyle::justify_self(&style).unwrap_or(parent_alignment.inline);
        GridItem {
            node,
            parent_writing_direction,
            parent_font_baseline: FontBaseline::from_writing_mode(parent_writing_direction.mode),
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
            aspect_ratio: ResolvedAspectRatio::from_option(style.aspect_ratio(), style.box_sizing()),
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
            block_offset: 0.0,
            block_size: 0.0,
            first_baseline: None,
            last_baseline: None,
        }
    }

    /// Resolve both baseline alignment contexts from the item's inherited
    /// writing mode. This is a node-level property, so the layout tree is the
    /// authoritative source rather than the numeric grid style projection.
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
    /// Major baselines participate in the start-most spanned track; minor
    /// baselines participate in the end-most one, both in logical order.
    pub fn baseline_sharing_track(&self, axis: AbstractAxis) -> OriginZeroLine {
        let span = self.placement(axis);
        let group = self.baseline_context.get(axis).group;
        let start_is_low = !self.parent_writing_direction.is_logical_axis_reversed(axis);
        if (group == BaselineGroup::Major) == start_is_low {
            span.start
        } else {
            span.end - 1
        }
    }

    /// Return the authored self-alignment in one logical grid axis.
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

    /// Update Grid's cyclic baseline fallback after measuring the fragment.
    ///
    /// A synthesized baseline cannot participate when the item's size in the
    /// alignment axis is percentage/stretch-dependent on an intrinsic or flex
    /// track. The fallback edge follows the item's baseline-sharing group.
    pub fn resolve_baseline_fallback(
        &mut self,
        axis: AbstractAxis,
        child_writing_mode: WritingMode,
        has_synthesized_baseline: bool,
    ) {
        let spans_content_sized_track = self.crosses_intrinsic_track(axis) || self.crosses_flexible_track(axis);
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

    /// Select the source required by Grid's minimum-contribution definition.
    ///
    /// This decision is based on the authored preferred size, before preferred
    /// aspect-ratio transfer. A ratio-derived numeric size retains the source
    /// semantics of `auto` and therefore cannot bypass the used minimum size.
    #[inline]
    fn minimum_contribution_source(&self, axis: AbstractAxis) -> MinimumContributionSource {
        let physical_axis = axis.to_absolute(self.parent_writing_direction.mode);
        let preferred = self.size.get_abs(physical_axis);
        let uses_containing_block = preferred.may_have_percentage_dependence()
            || preferred.is_stretch()
            || preferred.is_fit_content_keyword()
            || preferred.is_fit_content_function();

        if preferred.is_auto() || uses_containing_block {
            MinimumContributionSource::UsedMinimum
        } else {
            MinimumContributionSource::MinContent
        }
    }

    /// For an item spanning multiple tracks, the upper limit used to calculate its limited min-/max-content contribution is the
    /// sum of the fixed max track sizing functions of any tracks it spans, and is applied if it only spans such tracks.
    pub fn spanned_track_limit(
        &mut self,
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
        &mut self,
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

    /// Resolve the grid-owned auto-size policy for an intrinsic item probe.
    ///
    /// Authored preferred/min/max sizes remain child-owned. In particular, they
    /// must not be materialized into `known_dimensions`, whose axes mean that
    /// the parent formatting context has fixed the exact used size. The child
    /// combines this policy with the grid area and its own sizing properties at
    /// the normal constraint-space boundary.
    fn intrinsic_auto_size_behaviors(&self, tree: &impl LayoutPartialTree) -> (AutoSizeBehavior, AutoSizeBehavior) {
        let child_writing_mode = tree.get_writing_mode(self.node);
        let normal_auto_size = if self.is_compressible_replaced {
            AutoSizeBehavior::FitContent
        } else {
            AutoSizeBehavior::StretchImplicit
        };
        let logical_auto_size = InBothAbstractAxis {
            inline: resolve_self_alignment(
                self.used_alignment(AbstractAxis::Inline),
                AlignSelf::START,
                normal_auto_size,
            )
            .auto_size,
            block: resolve_self_alignment(self.used_alignment(AbstractAxis::Block), AlignSelf::START, normal_auto_size)
                .auto_size,
        };
        let (mut horizontal_auto_size, mut vertical_auto_size) = if self.parent_writing_direction.mode.is_horizontal() {
            (logical_auto_size.inline, logical_auto_size.block)
        } else {
            (logical_auto_size.block, logical_auto_size.inline)
        };
        if self.margin.left.is_auto() || self.margin.right.is_auto() {
            horizontal_auto_size = AutoSizeBehavior::FitContent;
        }
        if self.margin.top.is_auto() || self.margin.bottom.is_auto() {
            vertical_auto_size = AutoSizeBehavior::FitContent;
        }
        if child_writing_mode.is_horizontal() {
            (horizontal_auto_size, vertical_auto_size)
        } else {
            (vertical_auto_size, horizontal_auto_size)
        }
    }

    /// Remove this item's margins from the space offered for alignment
    /// stretch, while leaving the grid area itself available as the child's
    /// percentage containing block.
    ///
    /// These are distinct constraint-space inputs. Passing the full grid area
    /// as available space makes a stretched item measure descendants at a
    /// width that still includes its own margins, which feeds an inflated
    /// block contribution back into intrinsic track sizing.
    fn intrinsic_child_available_space(
        &self,
        tree: &mut impl LayoutPartialTree,
        available_space: Size<Option<f32>>,
    ) -> Size<Option<f32>> {
        let percentage_basis = self.parent_writing_direction.mode.to_logical(available_space).inline_size;
        let margins = self.margins_axis_sums_with_baseline_shims(percentage_basis, tree);
        available_space.maybe_sub(margins)
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
    ) -> Size<f32> {
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
        writing_direction.to_physical_box_strut(resolved_logical_margin).sum_axes()
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
        let child_available_space = self.intrinsic_child_available_space(tree, available_space);
        // The child sees the grid area as its containing block during intrinsic measurement, so
        // percentage box properties resolve against the grid area when that size is definite.
        // Spec:
        // https://www.w3.org/TR/css-grid-1/#grid-item-sizing
        // https://www.w3.org/TR/css-grid-1/#algo-overview
        let measured = measure_child_intrinsic_contribution(
            tree,
            self.node,
            ChildLayoutInput::new(
                Size::NONE,
                grid_area_size,
                self.parent_writing_direction.mode,
                child_available_space.map(|opt| match opt {
                    Some(size) => AvailableSpace::Definite(size),
                    None => AvailableSpace::MinContent,
                }),
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .without_orthogonal_fallback()
            .with_inline_auto_behavior(inline_auto_behavior)
            .with_block_auto_behavior(block_auto_behavior),
            axis.to_absolute(self.parent_writing_direction.mode),
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
        let child_available_space = self.intrinsic_child_available_space(tree, available_space);
        // See the min-content path above. Max-content measurement uses the same containing-block
        // basis so percentage-dependent item geometry is measured from the grid area rather than
        // from the container.
        let measured = measure_child_intrinsic_contribution(
            tree,
            self.node,
            ChildLayoutInput::new(
                Size::NONE,
                grid_area_size,
                self.parent_writing_direction.mode,
                child_available_space.map(|opt| match opt {
                    Some(size) => AvailableSpace::Definite(size),
                    None => AvailableSpace::MaxContent,
                }),
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .without_orthogonal_fallback()
            .with_inline_auto_behavior(inline_auto_behavior)
            .with_block_auto_behavior(block_auto_behavior),
            axis.to_absolute(self.parent_writing_direction.mode),
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
        inner_node_size: LogicalSize<Option<f32>>,
    ) -> f32 {
        let percentage_basis = self.parent_writing_direction.mode.to_logical(grid_area_size).inline_size;
        let padding = self.padding.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let border = self.border.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment =
            if self.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
        let preferred_size = self
            .size
            .maybe_resolve(grid_area_size, |val, basis| tree.calc(val, basis))
            .maybe_add(box_sizing_adjustment);
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: preferred_size,
            preferred_size_is_indefinite: preferred_size.map(|size| size.is_none()),
            min_size: resolve_minimum_size(self.min_size, grid_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
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
        let physical_axis = axis.to_absolute(self.parent_writing_direction.mode);
        match self.minimum_contribution_source(axis) {
            MinimumContributionSource::MinContent => {
                self.min_content_contribution_cached(axis, tree, grid_area_size, grid_area_size)
            }
            MinimumContributionSource::UsedMinimum => resolved
                .min_size
                .get_abs(physical_axis)
                .or_else(|| self.overflow.get(axis).maybe_into_automatic_min_size())
                .unwrap_or_else(|| {
                    // Automatic minimum size. See https://www.w3.org/TR/css-grid-1/#min-size-auto

                    // Otherwise, the automatic minimum size is zero, as usual.
                    if self.uses_content_based_automatic_minimum(axis, axis_tracks) {
                        let mut minimum_contribution =
                            self.min_content_contribution_cached(axis, tree, grid_area_size, grid_area_size);

                        // If the item is a compressible replaced element, and has a definite preferred size or maximum size in the
                        // relevant axis, the size suggestion is capped by those sizes; for this purpose, any indefinite percentages
                        // in these sizes are resolved against zero (and considered definite).
                        if self.is_compressible_replaced {
                            let size = self
                                .size
                                .get_abs(physical_axis)
                                .maybe_resolve(Some(0.0), |val, basis| tree.calc(val, basis));
                            let max_size = self
                                .max_size
                                .get_abs(physical_axis)
                                .maybe_resolve(Some(0.0), |val, basis| tree.calc(val, basis));
                            minimum_contribution = minimum_contribution.maybe_min(size).maybe_min(max_size);
                        }

                        // The content-based minimum size is additionally clamped by the sum of any fixed max track sizing
                        // functions of the tracks the item spans. Note that this clamp does not apply to explicitly specified
                        // preferred or minimum sizes, and that the argument to fit-content() does not clamp the content-based
                        // minimum size in the same way as a fixed max track sizing function.
                        let limit = self.spanned_fixed_track_limit(
                            axis,
                            axis_tracks,
                            inner_node_size.get(axis),
                            &|val, basis| tree.resolve_calc_value(val, basis),
                        );
                        minimum_contribution.maybe_min(limit)
                    } else {
                        0.0
                    }
                }),
        }
    }

    /// Retrieve the item's minimum contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn minimum_contribution_cached(
        &mut self,
        tree: &mut impl LayoutPartialTree,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: Size<Option<f32>>,
        inner_node_size: LogicalSize<Option<f32>>,
    ) -> f32 {
        self.minimum_contribution_cache.get(axis).unwrap_or_else(|| {
            let size = self.minimum_contribution(tree, axis, axis_tracks, grid_area_size, inner_node_size);
            self.minimum_contribution_cache.set(axis, Some(size));
            size
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style_helpers::{auto, percent};
    use crate::{Direction, Point, Style, WritingMode};

    #[test]
    fn overflow_axes_follow_the_parent_writing_mode() {
        let style: Style = Style { overflow: Point { x: Overflow::Hidden, y: Overflow::Scroll }, ..Style::default() };
        let item = GridItem::new_with_placement_style_and_order(
            NodeId::new(0),
            WritingDirection::new(WritingMode::VerticalLr, Direction::Ltr),
            InBothAbstractAxis {
                inline: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
                block: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
            },
            style,
            InBothAbstractAxis { inline: AlignItems::STRETCH, block: AlignItems::STRETCH },
            0,
        );

        assert_eq!(item.overflow, LogicalSize { inline_size: Overflow::Scroll, block_size: Overflow::Hidden });
    }

    #[test]
    fn supplied_baseline_avoids_the_intrinsic_track_cycle_fallback() {
        let style: Style = Style {
            size: Size { width: auto(), height: percent(0.5) },
            align_self: Some(AlignSelf::BASELINE),
            ..Style::default()
        };
        let mut item = GridItem::new_with_placement_style_and_order(
            NodeId::new(0),
            WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
            InBothAbstractAxis {
                inline: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
                block: Line { start: OriginZeroLine(0), end: OriginZeroLine(1) },
            },
            style,
            InBothAbstractAxis { inline: AlignItems::STRETCH, block: AlignItems::STRETCH },
            0,
        );
        item.crosses_intrinsic_row = true;

        item.resolve_baseline_fallback(AbstractAxis::Block, WritingMode::HorizontalTb, false);
        assert_eq!(item.used_alignment(AbstractAxis::Block), AlignSelf::BASELINE);

        item.resolve_baseline_fallback(AbstractAxis::Block, WritingMode::HorizontalTb, true);
        assert_eq!(item.used_alignment(AbstractAxis::Block), AlignSelf::START);
    }
}
