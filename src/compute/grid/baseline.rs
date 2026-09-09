//! Baseline participation, intrinsic shims and final fragment alignment.

use super::alignment::GridItemLayout;
use super::types::GridItem;
use crate::compute::common::baseline::{BaselineAlignment, BaselineGroups, BaselineMetrics, BaselineSide};
use crate::geometry::{AbsoluteAxis, AbstractAxis, InBothLogicalAxes, Line, LogicalSize, Rect, Size, WritingDirection};
use crate::tree::{ChildLayoutInput, LayoutOutput, LayoutPartialTree, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::sys::Vec;
use crate::util::ResolveOrZero;
use crate::{AvailableSpace, BaselineType, CoreStyle};

/// Both intrinsic measurements and final fragments use the container's
/// writing direction and dominant font baseline, not the item's defaults.
#[derive(Clone, Copy)]
pub(super) struct GridBaselineContext {
    /// The alignment container's writing mode and inline progression.
    writing_direction: WritingDirection,
    /// Dominant baseline used to synthesize absent or incompatible baselines.
    baseline_type: BaselineType,
}

impl GridBaselineContext {
    /// Read the container's baseline context once for this grid layout.
    pub(super) fn new(tree: &impl LayoutPartialTree, node: NodeId) -> Self {
        Self {
            writing_direction: WritingDirection::new(
                tree.get_writing_mode(node),
                tree.get_core_container_style(node).direction(),
            ),
            baseline_type: tree.get_baseline_type(node),
        }
    }

    /// Resolve the item's baseline context unless auto margins take precedence.
    fn alignment(
        self,
        tree: &impl LayoutPartialTree,
        item: &GridItem,
        axis: AbstractAxis,
    ) -> Option<BaselineAlignment> {
        let style = match axis {
            AbstractAxis::Inline => item.justify_self,
            AbstractAxis::Block => item.align_self,
        };
        let preference = style.baseline_preference()?;
        let physical_axis = self.writing_direction.mode.physical_axis(axis);
        let margin = axis_edges(item.margin, physical_axis);
        if margin.start.is_auto() || margin.end.is_auto() {
            return None;
        }
        Some(BaselineAlignment::new(
            self.writing_direction,
            tree.get_writing_mode(item.node),
            physical_axis,
            self.baseline_type,
            preference,
            false,
        ))
    }
}

/// A participating item's baseline in physical border-box coordinates and the
/// corresponding distances used by its sharing group.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GridItemBaseline {
    /// Physical coordinate relative to the item's border-box origin.
    pub(crate) position: f32,
    /// Signed distances to the margin-box edges in its sharing group.
    pub(crate) metrics: BaselineMetrics,
}

/// Select min/max physical edges at the fragment boundary.
fn axis_edges<T: Copy>(margin: Rect<T>, axis: AbsoluteAxis) -> Line<T> {
    match axis {
        AbsoluteAxis::Horizontal => margin.horizontal_components(),
        AbsoluteAxis::Vertical => margin.vertical_components(),
    }
}

/// Index of the first or last occupied logical track for this physical group.
fn baseline_track(item: &GridItem, axis: AbstractAxis, side: BaselineSide, flow: WritingDirection) -> usize {
    let indexes = item.placement_indexes(axis);
    let reversed = flow.mode.is_axis_flow_reversed(flow.mode.physical_axis(axis), flow.direction);
    if (side == BaselineSide::Max) != reversed {
        // Track vectors alternate line/gutter and track slots.
        indexes.end.saturating_sub(2) as usize
    } else {
        indexes.start as usize
    }
}

/// Collect independent opposing groups at each participating track edge.
fn collect_groups(items: &[GridItem], axis: AbstractAxis, flow: WritingDirection) -> Vec<BaselineGroups> {
    let mut groups = Vec::new();
    for item in items {
        let Some(baseline) = item.alignment_baselines.get(axis) else { continue };
        let index = baseline_track(item, axis, baseline.metrics.side, flow);
        if groups.len() <= index {
            groups.resize(index + 1, BaselineGroups::default());
        }
        groups[index].add(baseline.metrics);
    }
    groups
}

/// Resolve the fragment's actual or synthesized baseline, excluding cycles.
fn item_baselines(
    tree: &impl LayoutPartialTree,
    context: GridBaselineContext,
    item: &GridItem,
    fragment: LayoutOutput,
    margin: Rect<f32>,
) -> InBothLogicalAxes<Option<GridItemBaseline>> {
    let child_mode = tree.get_writing_mode(item.node);
    let resolve = |axis: AbstractAxis| {
        let physical_axis = context.writing_direction.mode.physical_axis(axis);
        let alignment_style = match axis {
            AbstractAxis::Inline => item.justify_self,
            AbstractAxis::Block => item.align_self,
        };
        let preference = alignment_style.baseline_preference()?;
        let alignment = context.alignment(tree, item, axis)?;
        let baselines = preference.select(fragment.first_baselines, fragment.last_baselines);
        let has_real_baseline = alignment.context.real_baseline(baselines, child_mode).is_some();
        // Like Blink's GridItemData::SetAlignmentFallback, exclude a
        // track-dependent synthesized baseline across intrinsic or flexible
        // tracks. Blink also does this for fr tracks in a definite container;
        // do not infer eligibility from a final resolved track/fragment size.
        let cyclic_track = item.crosses_intrinsic_track(axis) || item.crosses_flexible_track(axis);
        if !has_real_baseline
            && cyclic_track
            && [
                item.size.get_abs(physical_axis),
                item.min_size.get_abs(physical_axis),
                item.max_size.get_abs(physical_axis),
            ]
            .iter()
            .any(|size| size.may_have_percentage_dependence() || size.is_stretch())
        {
            return None;
        }
        let position = alignment.context.resolve(
            baselines,
            child_mode,
            fragment.size,
            item.overflow.x.is_scroll_container() || item.overflow.y.is_scroll_container(),
        );
        Some(GridItemBaseline {
            position,
            metrics: alignment.metrics(
                position,
                fragment.size.get_abs(physical_axis),
                axis_edges(margin, physical_axis),
            ),
        })
    };
    InBothLogicalAxes { inline: resolve(AbstractAxis::Inline), block: resolve(AbstractAxis::Block) }
}

/// Intrinsic shims are measured and consumed only in this sizing pass. Both
/// axes matter: a block-axis contribution can also affect intrinsic width.
pub(super) fn measure_intrinsic_baselines<Tree: LayoutPartialTree>(
    tree: &mut Tree,
    context: GridBaselineContext,
    items: &mut [GridItem],
    grid_area_size: impl Fn(&GridItem, &Tree) -> LogicalSize<Option<f32>>,
) {
    for item in items.iter_mut() {
        item.baseline_shims = Rect::ZERO;
        // A new set of shims changes intrinsic contributions even when a
        // child's own measured size is cached from an earlier sizing pass.
        item.min_content_contribution_cache = LogicalSize::NONE;
        item.max_content_contribution_cache = LogicalSize::NONE;
        item.minimum_contribution_cache = LogicalSize::NONE;
        item.alignment_baselines = InBothLogicalAxes { inline: None, block: None };
        if item.align_self.baseline_preference().is_none() && item.justify_self.baseline_preference().is_none() {
            continue;
        }
        let area_size = grid_area_size(item, tree);
        let known_dimensions = item.known_dimensions(tree, area_size);
        let output = tree.perform_child_layout(
            item.node,
            ChildLayoutInput::new(
                known_dimensions,
                item.parent_writing_mode.to_physical(area_size),
                item.parent_writing_mode,
                item.parent_writing_mode
                    .to_physical(area_size)
                    .map(|size| size.map_or(AvailableSpace::MinContent, AvailableSpace::Definite)),
                SizingMode::InherentSize,
                Line::FALSE,
            ),
        );
        let basis = area_size.inline_size;
        let margin = item.margin.map(|value| value.resolve_or_zero(basis, |value, basis| tree.calc(value, basis)));
        item.alignment_baselines = item_baselines(tree, context, item, output, margin);
    }
    for axis in [AbstractAxis::Inline, AbstractAxis::Block] {
        let groups = collect_groups(items, axis, context.writing_direction);
        for item in items.iter_mut() {
            let Some(baseline) = item.alignment_baselines.get(axis) else { continue };
            let shim = groups[baseline_track(item, axis, baseline.metrics.side, context.writing_direction)]
                .shim(baseline.metrics);
            match (context.writing_direction.mode.physical_axis(axis), baseline.metrics.side) {
                (AbsoluteAxis::Horizontal, BaselineSide::Min) => item.baseline_shims.left = shim,
                (AbsoluteAxis::Horizontal, BaselineSide::Max) => item.baseline_shims.right = shim,
                (AbsoluteAxis::Vertical, BaselineSide::Min) => item.baseline_shims.top = shim,
                (AbsoluteAxis::Vertical, BaselineSide::Max) => item.baseline_shims.bottom = shim,
            }
        }
    }
}

/// Resolve sharing groups from final fragments before any item position is
/// published. The intrinsic shims are deliberately not reused as margins or
/// as baseline measurements here.
pub(super) fn align_final_baselines(
    tree: &impl LayoutPartialTree,
    context: GridBaselineContext,
    items: &mut [GridItem],
    layouts: &mut [GridItemLayout],
) {
    for (item, fragment) in items.iter_mut().zip(layouts.iter()) {
        item.alignment_baselines = item_baselines(
            tree,
            context,
            item,
            LayoutOutput::from_sizes_and_baseline_sets(
                fragment.layout.size,
                Size::ZERO,
                fragment.first_baselines,
                fragment.last_baselines,
            ),
            fragment.layout.margin,
        );
    }
    for axis in [AbstractAxis::Inline, AbstractAxis::Block] {
        let groups = collect_groups(items, axis, context.writing_direction);
        for (item, fragment) in items.iter().zip(layouts.iter_mut()) {
            let Some(alignment) = context.alignment(tree, item, axis) else { continue };
            let physical_axis = context.writing_direction.mode.physical_axis(axis);
            let area = axis_edges(fragment.grid_area, physical_axis);
            let margin = axis_edges(fragment.layout.margin, physical_axis);
            let free_space = area.end - area.start - fragment.layout.size.get_abs(physical_axis) - margin.sum();
            let offset = if let Some(baseline) = item.alignment_baselines.get(axis) {
                groups[baseline_track(item, axis, baseline.metrics.side, context.writing_direction)]
                    .alignment_offset(baseline.metrics, free_space)
            } else {
                // A cyclic synthesized baseline uses its group's fallback
                // edge, which need not be the container's own start/end.
                match alignment.side {
                    BaselineSide::Min => 0.0,
                    BaselineSide::Max => free_space,
                }
            };
            let origin = area.start + margin.start + offset;
            match physical_axis {
                AbsoluteAxis::Horizontal => fragment.layout.location.x = origin + fragment.relative_offset.x,
                AbsoluteAxis::Vertical => fragment.layout.location.y = origin + fragment.relative_offset.y,
            }
        }
    }
}

/// Baselines from the edge rows prefer the corresponding sharing group, then
/// the opposite group, then a real child baseline in grid order. A spanning
/// item participates at its group's own start/end track.
pub(super) fn container_baselines(items: &[GridItem], flow: WritingDirection) -> (f32, f32) {
    debug_assert!(!items.is_empty());
    let first_row = items.iter().map(|item| item.row_indexes.start).min().unwrap() as usize;
    let last_row = items.iter().map(|item| item.row_indexes.end.saturating_sub(2)).max().unwrap() as usize;
    let resolve = |last: bool| {
        let row = if last { last_row } else { first_row };
        let prefer_max = last != flow.is_block_flow_reversed();
        let sides =
            if prefer_max { [BaselineSide::Max, BaselineSide::Min] } else { [BaselineSide::Min, BaselineSide::Max] };
        for side in sides {
            let candidates = items.iter().filter_map(|item| {
                let baseline = item.alignment_baselines.block?;
                (baseline.metrics.side == side && baseline_track(item, AbstractAxis::Block, side, flow) == row)
                    .then_some((item, baseline.position))
            });
            let selected = if last {
                candidates.max_by_key(|(item, _)| (item.column_indexes.end, item.source_order))
            } else {
                candidates.min_by_key(|(item, _)| (item.column_indexes.start, item.source_order))
            };
            if let Some((item, position)) = selected {
                return item.block_axis_origin + position;
            }
        }
        let candidates = items.iter().filter(|item| {
            if last {
                item.row_indexes.end.saturating_sub(2) as usize == row
            } else {
                item.row_indexes.start as usize == row
            }
        });
        let selected = if last {
            candidates.max_by_key(|item| (item.last_baseline.is_some(), item.column_indexes.end, item.source_order))
        } else {
            candidates.min_by_key(|item| (item.first_baseline.is_none(), item.column_indexes.start, item.source_order))
        }
        .unwrap();
        selected.block_axis_origin
            + if last { selected.last_baseline } else { selected.first_baseline }
                .unwrap_or(selected.synthesized_baseline)
    };
    (resolve(false), resolve(true))
}
