//! Contains GridItem used to represent a single grid item during layout
use super::GridTrack;
use crate::compute::common::aspect_ratio::{resolve_size_constraints, SizeConstraintInput, TransferredSizesMode};
use crate::compute::grid::baseline::GridItemBaseline;
use crate::compute::grid::item_sizing::{auto_size_behavior, resolve_item_sizing, GridItemSizing};
use crate::compute::grid::OriginZeroLine;
use crate::geometry::AbstractAxis;
use crate::geometry::{InBothLogicalAxes, Line, LogicalSize, Point, Rect, Size};
use crate::style::{
    AlignItems, AlignSelf, AvailableSpace, Dimension, LengthPercentageAuto, Overflow, ResolvedAspectRatio,
};
use crate::tree::{ChildLayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{AutoSizeBehavior, BoxSizing, GridItemStyle, LengthPercentage, WritingMode};
use core::ops::Range;

/// Represents a single grid item
#[derive(Debug)]
pub(in super::super) struct GridItem {
    /// The id of the node that this item represents
    pub node: NodeId,

    /// Writing mode of the grid container that establishes this item's
    /// containing block.
    pub parent_writing_mode: WritingMode,

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
    pub overflow: Point<Overflow>,
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
    /// Baseline participation on each logical axis; metrics remain physical
    /// distances within the fragment's compatible baseline context.
    /// Intrinsic measurement and final layout collect independent sets.
    pub alignment_baselines: InBothLogicalAxes<Option<GridItemBaseline>>,
    /// Additional intrinsic contribution on each group's packing side.
    /// These are not author margins and must never enter `Layout::margin`.
    pub baseline_shims: Rect<f32>,

    /// The item's definite row-start and row-end (same as `row` field, except in a different coordinate system)
    /// (as indexes into the grid's logical column track vector)
    pub row_indexes: Line<u16>,
    /// The items definite column-start and column-end (same as `column` field, except in a different coordinate system)
    /// (as indexes into the grid's logical row track vector)
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
    pub grid_area_size_cache: Option<LogicalSize<Option<f32>>>,
    /// Cache for the min-content size
    pub min_content_contribution_cache: LogicalSize<Option<f32>>,
    /// Cache for the minimum contribution
    pub minimum_contribution_cache: LogicalSize<Option<f32>>,
    /// Cache for the max-content size
    pub max_content_contribution_cache: LogicalSize<Option<f32>>,
    /// Whether an intrinsic item contribution observed a dependency on the
    /// grid area's block-size.
    pub depends_on_block_constraints: bool,

    /// Physical border-box origin along the container's block axis, before
    /// relative positioning. Used to propagate the container's baselines.
    pub block_axis_origin: f32,
    /// Baseline synthesized in the container's writing mode and font context.
    pub synthesized_baseline: f32,
    /// First baseline from the item's final layout, relative to its border box.
    pub first_baseline: Option<f32>,
    /// Last baseline from the item's final layout, relative to its border box.
    pub last_baseline: Option<f32>,
}

impl GridItem {
    /// The requested alignment in one of the container's logical axes.
    pub fn alignment(&self, axis: AbstractAxis) -> AlignSelf {
        match axis {
            AbstractAxis::Inline => self.justify_self,
            AbstractAxis::Block => self.align_self,
        }
    }

    /// Physical min/max margin edges along the requested logical axis.
    pub fn physical_axis_margins(&self, axis: AbstractAxis) -> Line<LengthPercentageAuto> {
        match self.parent_writing_mode.physical_axis(axis) {
            crate::AbsoluteAxis::Horizontal => self.margin.horizontal_components(),
            crate::AbsoluteAxis::Vertical => self.margin.vertical_components(),
        }
    }

    /// Physical overflow projected onto a container-relative logical axis.
    pub fn overflow_in_axis(&self, axis: AbstractAxis) -> Overflow {
        match self.parent_writing_mode.physical_axis(axis) {
            crate::AbsoluteAxis::Horizontal => self.overflow.x,
            crate::AbsoluteAxis::Vertical => self.overflow.y,
        }
    }

    /// Create a new item given a concrete placement in both axes
    pub fn new_with_placement_style_and_order<S: GridItemStyle>(
        node: NodeId,
        parent_writing_mode: WritingMode,
        placement: InBothLogicalAxes<Line<OriginZeroLine>>,
        style: S,
        parent_alignment: InBothLogicalAxes<AlignItems>,
        source_order: u16,
    ) -> Self {
        GridItem {
            node,
            parent_writing_mode,
            source_order,
            row: placement.block,
            column: placement.inline,
            is_compressible_replaced: style.is_compressible_replaced(),
            overflow: style.overflow(),
            box_sizing: style.box_sizing(),
            size: style.size(),
            min_size: style.min_size(),
            max_size: style.max_size(),
            aspect_ratio: style.aspect_ratio().and_then(|ratio| ResolvedAspectRatio::new(ratio, style.box_sizing())),
            padding: style.padding(),
            border: style.border(),
            margin: style.margin(),
            align_self: style.align_self().unwrap_or(parent_alignment.block),
            justify_self: style.justify_self().unwrap_or(parent_alignment.inline),
            alignment_baselines: InBothLogicalAxes { inline: None, block: None },
            baseline_shims: Rect::ZERO,
            row_indexes: Line { start: 0, end: 0 }, // Properly initialised later
            column_indexes: Line { start: 0, end: 0 }, // Properly initialised later
            crosses_flexible_row: false,            // Properly initialised later
            crosses_flexible_column: false,         // Properly initialised later
            crosses_intrinsic_row: false,           // Properly initialised later
            crosses_intrinsic_column: false,        // Properly initialised later
            grid_area_size_cache: None,
            min_content_contribution_cache: LogicalSize::NONE,
            max_content_contribution_cache: LogicalSize::NONE,
            minimum_contribution_cache: LogicalSize::NONE,
            depends_on_block_constraints: false,
            block_axis_origin: 0.0,
            synthesized_baseline: 0.0,
            first_baseline: None,
            last_baseline: None,
        }
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

    /// Compute the known_dimensions to be passed to the child sizing functions
    /// The key thing that is being done here is applying stretch alignment, which is necessary to
    /// allow percentage sizes further down the tree to resolve properly in some cases
    pub(in crate::compute::grid) fn sizing_constraints(
        &self,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: LogicalSize<Option<f32>>,
    ) -> GridItemSizing {
        let percentage_basis = grid_area_size.inline_size;
        let physical_area_size = self.parent_writing_mode.to_physical(grid_area_size);
        let margins =
            self.parent_writing_mode.to_physical(self.margins_axis_sums_with_baseline_shims(percentage_basis, tree));

        let aspect_ratio = self.aspect_ratio;
        // CSS resolves percentage padding and border against the inline size
        // of the containing block.
        // Spec:
        // https://www.w3.org/TR/css-grid-1/#item-margins
        // https://www.w3.org/TR/CSS22/box.html#padding-properties
        let padding = self.padding.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let border = self.border.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment =
            if self.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
        let input = SizeConstraintInput {
            size: self
                .size
                .maybe_resolve(physical_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            min_size: self
                .min_size
                .maybe_resolve(physical_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            max_size: self
                .max_size
                .maybe_resolve(physical_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            size_is_auto: self.size.map(|dimension| dimension.is_auto()),
            writing_mode: tree.get_writing_mode(self.node),
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio,
            padding_border: padding_border_size,
        };

        let grid_area_minus_item_margins_size = physical_area_size.maybe_sub(margins);

        let behavior = |axis| {
            let margin = self.physical_axis_margins(axis);
            auto_size_behavior(
                self.alignment(axis),
                margin.start.is_auto() || margin.end.is_auto(),
                self.is_compressible_replaced,
            )
        };
        resolve_item_sizing(
            input,
            grid_area_minus_item_margins_size,
            self.parent_writing_mode.to_physical(LogicalSize {
                inline_size: behavior(AbstractAxis::Inline),
                block_size: behavior(AbstractAxis::Block),
            }),
        )
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
    ) -> LogicalSize<Option<f32>> {
        let mut size = LogicalSize::NONE;
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

        size
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
    ) -> LogicalSize<Option<f32>> {
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

    /// Resolve intrinsic margin contributions in the grid's logical axes.
    /// Inline-axis percentage margins are cyclic in intrinsic inline sizing;
    /// block-axis percentage margins use the containing inline size.
    #[inline(always)]
    pub fn margins_axis_sums_with_baseline_shims(
        &self,
        inner_inline_size: Option<f32>,
        tree: &impl LayoutPartialTree,
    ) -> LogicalSize<f32> {
        let sums = |axis, basis| {
            let margin = self.physical_axis_margins(axis);
            margin.start.resolve_or_zero(basis, |value, basis| tree.calc(value, basis))
                + margin.end.resolve_or_zero(basis, |value, basis| tree.calc(value, basis))
        };
        LogicalSize {
            inline_size: sums(AbstractAxis::Inline, Some(0.0)),
            block_size: sums(AbstractAxis::Block, inner_inline_size),
        } + self.parent_writing_mode.to_logical(self.baseline_shims.sum_axes())
    }

    /// Compute the item's min content contribution from the provided parameters
    pub fn min_content_contribution(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: LogicalSize<Option<f32>>,
        available_space: LogicalSize<Option<f32>>,
    ) -> f32 {
        let sizing = self.sizing_constraints(tree, grid_area_size);
        // The child sees the grid area as its containing block during intrinsic measurement, so
        // percentage box properties resolve against the grid area when that size is definite.
        // Spec:
        // https://www.w3.org/TR/css-grid-1/#grid-item-sizing
        // https://www.w3.org/TR/css-grid-1/#algo-overview
        let measured = tree.measure_child_size_with_metadata(
            self.node,
            ChildLayoutInput::new(
                sizing.known_dimensions,
                self.parent_writing_mode.to_physical(grid_area_size),
                self.parent_writing_mode,
                self.parent_writing_mode.to_physical(available_space.map(|opt| match opt {
                    Some(size) => AvailableSpace::Definite(size),
                    None => AvailableSpace::MinContent,
                })),
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .with_block_auto_behavior(sizing.block_auto_behavior),
            self.parent_writing_mode.physical_axis(axis).into(),
        );
        self.depends_on_block_constraints |= measured.depends_on_block_constraints;
        measured.size.get_abs(self.parent_writing_mode.physical_axis(axis))
    }

    /// Retrieve the item's min content contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn min_content_contribution_cached(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: LogicalSize<Option<f32>>,
        available_space: LogicalSize<Option<f32>>,
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
        grid_area_size: LogicalSize<Option<f32>>,
        available_space: LogicalSize<Option<f32>>,
    ) -> f32 {
        let sizing = self.sizing_constraints(tree, grid_area_size);
        // See the min-content path above. Max-content measurement uses the same containing-block
        // basis so percentage-dependent item geometry is measured from the grid area rather than
        // from the container.
        let measured = tree.measure_child_size_with_metadata(
            self.node,
            ChildLayoutInput::new(
                sizing.known_dimensions,
                self.parent_writing_mode.to_physical(grid_area_size),
                self.parent_writing_mode,
                self.parent_writing_mode.to_physical(available_space.map(|opt| match opt {
                    Some(size) => AvailableSpace::Definite(size),
                    None => AvailableSpace::MaxContent,
                })),
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .with_block_auto_behavior(sizing.block_auto_behavior),
            self.parent_writing_mode.physical_axis(axis).into(),
        );
        self.depends_on_block_constraints |= measured.depends_on_block_constraints;
        measured.size.get_abs(self.parent_writing_mode.physical_axis(axis))
    }

    /// Retrieve the item's max content contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn max_content_contribution_cached(
        &mut self,
        axis: AbstractAxis,
        tree: &mut impl LayoutPartialTree,
        grid_area_size: LogicalSize<Option<f32>>,
        available_space: LogicalSize<Option<f32>>,
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
        grid_area_size: LogicalSize<Option<f32>>,
        inner_node_size: LogicalSize<Option<f32>>,
    ) -> f32 {
        let percentage_basis = grid_area_size.inline_size;
        let physical_area_size = self.parent_writing_mode.to_physical(grid_area_size);
        let padding = self.padding.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let border = self.border.resolve_or_zero(percentage_basis, |val, basis| tree.calc(val, basis));
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment =
            if self.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: self
                .size
                .maybe_resolve(physical_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            min_size: self
                .min_size
                .maybe_resolve(physical_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            max_size: self
                .max_size
                .maybe_resolve(physical_area_size, |val, basis| tree.calc(val, basis))
                .maybe_add(box_sizing_adjustment),
            size_is_auto: self.size.map(|dimension| dimension.is_auto()),
            writing_mode: tree.get_writing_mode(self.node),
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: self.aspect_ratio,
            padding_border: padding_border_size,
        });
        resolved
            .size
            .get_abs(self.parent_writing_mode.physical_axis(axis))
            .or_else(|| resolved.min_size.get_abs(self.parent_writing_mode.physical_axis(axis)))
            .or_else(|| self.overflow_in_axis(axis).maybe_into_automatic_min_size())
            .unwrap_or_else(|| {
                // Automatic minimum size. See https://www.w3.org/TR/css-grid-1/#min-size-auto

                // To provide a more reasonable default minimum size for grid items, the used value of its automatic minimum size
                // in a given axis is the content-based minimum size if all of the following are true:
                let item_axis_tracks = &axis_tracks[self.track_range_excluding_lines(axis)];

                // it is not a scroll container
                // TODO: support overflow property

                // it spans at least one track in that axis whose min track sizing function is auto
                let spans_auto_min_track = axis_tracks
                    .iter()
                    // TODO: should this be 'behaves as auto' rather than just literal auto?
                    .any(|track| track.min_track_sizing_function.is_auto());

                // if it spans more than one track in that axis, none of those tracks are flexible
                let only_span_one_track = item_axis_tracks.len() == 1;
                let spans_a_flexible_track = axis_tracks.iter().any(|track| track.max_track_sizing_function.is_fr());

                let use_content_based_minimum =
                    spans_auto_min_track && (only_span_one_track || !spans_a_flexible_track);

                // Otherwise, the automatic minimum size is zero, as usual.
                if use_content_based_minimum {
                    let mut minimum_contribution =
                        self.min_content_contribution_cached(axis, tree, grid_area_size, grid_area_size);

                    // If the item is a compressible replaced element, and has a definite preferred size or maximum size in the
                    // relevant axis, the size suggestion is capped by those sizes; for this purpose, any indefinite percentages
                    // in these sizes are resolved against zero (and considered definite).
                    if self.is_compressible_replaced {
                        let size = self
                            .size
                            .get_abs(self.parent_writing_mode.physical_axis(axis))
                            .maybe_resolve(Some(0.0), |val, basis| tree.calc(val, basis));
                        let max_size = self
                            .max_size
                            .get_abs(self.parent_writing_mode.physical_axis(axis))
                            .maybe_resolve(Some(0.0), |val, basis| tree.calc(val, basis));
                        minimum_contribution = minimum_contribution.maybe_min(size).maybe_min(max_size);
                    }

                    // The content-based minimum size is additionally clamped by the sum of any fixed max track sizing
                    // functions of the tracks the item spans. Note that this clamp does not apply to explicitly specified
                    // preferred or minimum sizes, and that the argument to fit-content() does not clamp the content-based
                    // minimum size in the same way as a fixed max track sizing function.
                    let limit =
                        self.spanned_fixed_track_limit(axis, axis_tracks, inner_node_size.get(axis), &|val, basis| {
                            tree.resolve_calc_value(val, basis)
                        });
                    minimum_contribution.maybe_min(limit)
                } else {
                    0.0
                }
            })
    }

    /// Retrieve the item's minimum contribution from the cache or compute it using the provided parameters
    #[inline(always)]
    pub fn minimum_contribution_cached(
        &mut self,
        tree: &mut impl LayoutPartialTree,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: LogicalSize<Option<f32>>,
        inner_node_size: LogicalSize<Option<f32>>,
    ) -> f32 {
        self.minimum_contribution_cache.get(axis).unwrap_or_else(|| {
            let size = self.minimum_contribution(tree, axis, axis_tracks, grid_area_size, inner_node_size);
            self.minimum_contribution_cache.set(axis, Some(size));
            size
        })
    }
}
