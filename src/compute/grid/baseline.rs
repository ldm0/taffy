//! Baseline participation, intrinsic shims and final fragment alignment.

use super::alignment::GridItemLayout;
use super::types::GridItem;
use crate::compute::common::baseline::{BaselineAlignment, BaselineGroups, BaselineMetrics, BaselineSide};
use crate::geometry::{AbstractAxis, InBothAbsAxis, Line, Rect, Size, WritingDirection};
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
        let margin = axis_edges(item.margin, axis);
        if margin.start.is_auto() || margin.end.is_auto() {
            return None;
        }
        Some(BaselineAlignment::new(
            self.writing_direction,
            tree.get_writing_mode(item.node),
            axis.as_abs_naive(),
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

/// Select physical edges using grid's existing row/column storage convention.
fn axis_edges<T: Copy>(margin: Rect<T>, axis: AbstractAxis) -> Line<T> {
    match axis {
        AbstractAxis::Inline => margin.horizontal_components(),
        AbstractAxis::Block => margin.vertical_components(),
    }
}

/// Index of the item's first or last occupied physical track for this group.
fn baseline_track(item: &GridItem, axis: AbstractAxis, side: BaselineSide) -> usize {
    let indexes = item.placement_indexes(axis);
    match side {
        BaselineSide::Min => indexes.start as usize,
        // Track vectors alternate line/gutter and track slots. A spanning
        // last-baseline item belongs to its last occupied track, not its first.
        BaselineSide::Max => indexes.end.saturating_sub(2) as usize,
    }
}

/// Collect independent opposing groups at each participating track edge.
fn collect_groups(items: &[GridItem], axis: AbstractAxis) -> Vec<BaselineGroups> {
    let mut groups = Vec::new();
    for item in items {
        let Some(baseline) = item.alignment_baselines.get(axis.as_abs_naive()) else { continue };
        let index = baseline_track(item, axis, baseline.metrics.side);
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
) -> InBothAbsAxis<Option<GridItemBaseline>> {
    let child_mode = tree.get_writing_mode(item.node);
    let resolve = |axis: AbstractAxis| {
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
            && [item.size.get(axis), item.min_size.get(axis), item.max_size.get(axis)]
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
            metrics: alignment.metrics(position, fragment.size.get(axis), axis_edges(margin, axis)),
        })
    };
    InBothAbsAxis { horizontal: resolve(AbstractAxis::Inline), vertical: resolve(AbstractAxis::Block) }
}

/// Intrinsic shims are measured and consumed only in this sizing pass. Both
/// axes matter: a block-axis contribution can also affect intrinsic width.
pub(super) fn measure_intrinsic_baselines<Tree: LayoutPartialTree>(
    tree: &mut Tree,
    context: GridBaselineContext,
    items: &mut [GridItem],
    grid_area_size: impl Fn(&GridItem, &Tree) -> Size<Option<f32>>,
) {
    for item in items.iter_mut() {
        item.baseline_shims = Rect::ZERO;
        // A new set of shims changes intrinsic contributions even when a
        // child's own measured size is cached from an earlier sizing pass.
        item.min_content_contribution_cache = Size::NONE;
        item.max_content_contribution_cache = Size::NONE;
        item.minimum_contribution_cache = Size::NONE;
        item.alignment_baselines = InBothAbsAxis { horizontal: None, vertical: None };
        if item.align_self.baseline_preference().is_none() && item.justify_self.baseline_preference().is_none() {
            continue;
        }
        let area_size = grid_area_size(item, tree);
        let known_dimensions = item.known_dimensions(tree, area_size);
        let output = tree.perform_child_layout(
            item.node,
            ChildLayoutInput::new(
                known_dimensions,
                area_size,
                item.parent_writing_mode,
                area_size.map(|size| size.map_or(AvailableSpace::MinContent, AvailableSpace::Definite)),
                SizingMode::InherentSize,
                Line::FALSE,
            ),
        );
        let basis = item.parent_writing_mode.to_logical(area_size).inline_size;
        let margin = item.margin.map(|value| value.resolve_or_zero(basis, |value, basis| tree.calc(value, basis)));
        item.alignment_baselines = item_baselines(tree, context, item, output, margin);
    }
    for axis in [AbstractAxis::Inline, AbstractAxis::Block] {
        let groups = collect_groups(items, axis);
        for item in items.iter_mut() {
            let Some(baseline) = item.alignment_baselines.get(axis.as_abs_naive()) else { continue };
            let shim = groups[baseline_track(item, axis, baseline.metrics.side)].shim(baseline.metrics);
            match (axis, baseline.metrics.side) {
                (AbstractAxis::Inline, BaselineSide::Min) => item.baseline_shims.left = shim,
                (AbstractAxis::Inline, BaselineSide::Max) => item.baseline_shims.right = shim,
                (AbstractAxis::Block, BaselineSide::Min) => item.baseline_shims.top = shim,
                (AbstractAxis::Block, BaselineSide::Max) => item.baseline_shims.bottom = shim,
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
        let groups = collect_groups(items, axis);
        for (item, fragment) in items.iter().zip(layouts.iter_mut()) {
            let Some(alignment) = context.alignment(tree, item, axis) else { continue };
            let area = axis_edges(fragment.grid_area, axis);
            let margin = axis_edges(fragment.layout.margin, axis);
            let free_space = area.end - area.start - fragment.layout.size.get(axis) - margin.sum();
            let offset = if let Some(baseline) = item.alignment_baselines.get(axis.as_abs_naive()) {
                groups[baseline_track(item, axis, baseline.metrics.side)].alignment_offset(baseline.metrics, free_space)
            } else {
                // A cyclic synthesized baseline uses its group's fallback
                // edge, which need not be the container's own start/end.
                match alignment.side {
                    BaselineSide::Min => 0.0,
                    BaselineSide::Max => free_space,
                }
            };
            let origin = area.start + margin.start + offset;
            match axis {
                AbstractAxis::Inline => fragment.layout.location.x = origin + fragment.relative_offset.x,
                AbstractAxis::Block => fragment.layout.location.y = origin + fragment.relative_offset.y,
            }
        }
    }
}

/// Baselines from the edge rows prefer the corresponding sharing group, then
/// the opposite group, then a real child baseline in grid order. A spanning
/// item participates at its group's own start/end track.
pub(super) fn container_baselines(items: &[GridItem]) -> (f32, f32) {
    debug_assert!(!items.is_empty());
    let first_row = items.iter().map(|item| item.row_indexes.start).min().unwrap() as usize;
    let last_row = items.iter().map(|item| item.row_indexes.end.saturating_sub(2)).max().unwrap() as usize;
    let resolve = |last: bool| {
        let row = if last { last_row } else { first_row };
        let sides = if last { [BaselineSide::Max, BaselineSide::Min] } else { [BaselineSide::Min, BaselineSide::Max] };
        for side in sides {
            let candidates = items.iter().filter_map(|item| {
                let baseline = item.alignment_baselines.vertical?;
                (baseline.metrics.side == side && baseline_track(item, AbstractAxis::Block, side) == row)
                    .then_some((item, baseline.position))
            });
            let selected = if last {
                candidates.max_by_key(|(item, _)| (item.column_indexes.end, item.source_order))
            } else {
                candidates.min_by_key(|(item, _)| (item.column_indexes.start, item.source_order))
            };
            if let Some((item, position)) = selected {
                return item.y_position + position;
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
        selected.y_position
            + if last { selected.last_baseline } else { selected.first_baseline }.unwrap_or(selected.height)
    };
    (resolve(false), resolve(true))
}
