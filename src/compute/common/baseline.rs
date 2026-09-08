//! Flow-relative baseline selection, synthesis and sharing groups.

use crate::{AbsoluteAxis, BaselinePreference, BaselineType, Line, Point, Size, WritingDirection, WritingMode};

/// Physical edge against which a baseline-sharing group is packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BaselineSide {
    /// Top or left.
    Min,
    /// Bottom or right.
    Max,
}

/// Writing mode and dominant baseline required by one alignment context.
#[derive(Clone, Copy, Debug)]
pub struct BaselineContext {
    /// Axis-compatible writing mode in which the baseline is requested.
    pub writing_mode: WritingMode,
    /// Font baseline to synthesize when the fragment has no compatible set.
    pub baseline_type: BaselineType,
}

impl BaselineContext {
    /// Select a real baseline only when the fragment's set is compatible with
    /// this line context. A perpendicular baseline is not interchangeable.
    pub fn real_baseline(self, baselines: Point<Option<f32>>, child_mode: WritingMode) -> Option<f32> {
        (self.writing_mode == child_mode)
            .then(|| match self.writing_mode.block_axis() {
                AbsoluteAxis::Horizontal => baselines.x,
                AbsoluteAxis::Vertical => baselines.y,
            })
            .flatten()
    }

    /// Resolve a baseline in physical border-box coordinates. An orthogonal
    /// fragment's other-axis baseline is not a baseline in this context.
    pub(crate) fn resolve(
        self,
        baselines: Point<Option<f32>>,
        child_mode: WritingMode,
        size: Size<f32>,
        is_scroll_container: bool,
    ) -> f32 {
        let extent = size.get_abs(self.writing_mode.block_axis());
        let baseline = self.real_baseline(baselines, child_mode).unwrap_or_else(|| match self.baseline_type {
            BaselineType::Central => extent / 2.0,
            BaselineType::Alphabetic => match self.writing_mode {
                WritingMode::HorizontalTb | WritingMode::SidewaysLr => extent,
                WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl => 0.0,
            },
        });
        if is_scroll_container {
            baseline.clamp(0.0, extent)
        } else {
            baseline
        }
    }
}

/// One item's compatible baseline context and baseline-sharing group.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BaselineAlignment {
    /// Context selecting the item's real or synthesized baseline.
    pub(crate) context: BaselineContext,
    /// Physical packing edge of the item's baseline-sharing group.
    pub(crate) side: BaselineSide,
    /// Convert physical baseline coordinates to the baseline context's ascent.
    /// This is distinct from the sharing group's packing edge (e.g. sideways-lr
    /// column flex uses a horizontal baseline in a bottom-to-top cross flow).
    pub(crate) ascent_reversed: bool,
}

impl BaselineAlignment {
    /// CSS Align's axis-compatible writing mode and major/minor grouping,
    /// expressed on the physical alignment axis. Only flex reverses line flow.
    pub(crate) fn new(
        container: WritingDirection,
        child: WritingMode,
        axis: AbsoluteAxis,
        baseline_type: BaselineType,
        preference: BaselinePreference,
        wrap_reverse: bool,
    ) -> Self {
        let main_axis_is_inline = axis == container.mode.block_axis();
        let cross_start_reversed = container.mode.is_axis_flow_reversed(axis, container.direction);
        let parallel = !container.mode.is_orthogonal_to(child);
        let writing_mode = if main_axis_is_inline {
            if parallel {
                child
            } else {
                container.mode
            }
        } else if !parallel {
            child
        } else if child.is_horizontal() {
            if container.direction.is_rtl() {
                WritingMode::VerticalRl
            } else {
                WritingMode::VerticalLr
            }
        } else {
            WritingMode::HorizontalTb
        };
        let major = if main_axis_is_inline {
            writing_mode == container.mode
        } else {
            container.direction.is_rtl() == writing_mode.is_block_flow_reversed()
        } ^ wrap_reverse
            ^ (preference == BaselinePreference::Last);
        let side = if major == cross_start_reversed { BaselineSide::Max } else { BaselineSide::Min };
        Self {
            context: BaselineContext { writing_mode, baseline_type },
            side,
            ascent_reversed: writing_mode.is_block_flow_reversed()
                ^ wrap_reverse
                ^ (preference == BaselinePreference::Last),
        }
    }

    /// Measure the distances to both physical margin-box edges in the
    /// baseline context. Negative author margins remain signed.
    pub(crate) fn metrics(self, baseline: f32, extent: f32, margin: Line<f32>) -> BaselineMetrics {
        let ascent = if self.ascent_reversed { extent - baseline } else { baseline }
            + match self.side {
                BaselineSide::Min => margin.start,
                BaselineSide::Max => margin.end,
            };
        BaselineMetrics { side: self.side, ascent, descent: extent + margin.sum() - ascent }
    }
}

/// Distances from an item's shared baseline to its outer cross edges.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BaselineMetrics {
    /// Packing edge of the compatible sharing group.
    pub(crate) side: BaselineSide,
    /// Distance from the packing-side margin edge to the baseline.
    pub(crate) ascent: f32,
    /// Distance from the baseline to the opposite margin edge.
    pub(crate) descent: f32,
}

/// Opposing baseline groups must size independently: unrelated ascents and
/// descents must never be combined into one fictitious baseline-sharing group.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BaselineGroups {
    /// Group packed toward physical left/top.
    min: Option<BaselineMetrics>,
    /// Group packed toward physical right/bottom.
    max: Option<BaselineMetrics>,
}

impl BaselineGroups {
    /// Export the preferred sharing group's baseline, falling back to the
    /// opposite group only when the preferred one is absent.
    pub(crate) fn baseline(self, cross_size: f32, prefer_max: bool) -> Option<f32> {
        let min = self.min.map(|group| group.ascent);
        let max = self.max.map(|group| cross_size - group.ascent);
        if prefer_max {
            max.or(min)
        } else {
            min.or(max)
        }
    }

    /// Expand the item's own sharing group, preserving signed margins.
    pub(crate) fn add(&mut self, metrics: BaselineMetrics) {
        let group = match metrics.side {
            BaselineSide::Min => &mut self.min,
            BaselineSide::Max => &mut self.max,
        };
        if let Some(group) = group {
            group.ascent = group.ascent.max(metrics.ascent);
            group.descent = group.descent.max(metrics.descent);
        } else {
            *group = Some(metrics);
        }
    }

    /// Cross size needed to accommodate both groups independently.
    pub(crate) fn cross_size(self) -> f32 {
        [self.min, self.max].into_iter().flatten().map(|group| group.ascent + group.descent).fold(0.0, f32::max)
    }

    /// Physical margin-box offset aligning the item with its sharing group.
    pub(crate) fn alignment_offset(self, metrics: BaselineMetrics, free_space: f32) -> f32 {
        let delta = self.shim(metrics);
        match metrics.side {
            BaselineSide::Min => delta,
            BaselineSide::Max => free_space - delta,
        }
    }

    /// Extra space on the packing side needed to reach the shared baseline.
    /// Grid adds this to intrinsic contributions, not to used CSS margins.
    pub(crate) fn shim(self, metrics: BaselineMetrics) -> f32 {
        let group = match metrics.side {
            BaselineSide::Min => self.min,
            BaselineSide::Max => self.max,
        }
        .expect("a participating baseline must have been collected into its sharing group");
        group.ascent - metrics.ascent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesis_uses_line_under_or_central_baseline_in_each_writing_mode() {
        let size = Size { width: 40.0, height: 60.0 };
        for (writing_mode, alphabetic, central) in [
            (WritingMode::HorizontalTb, 60.0, 30.0),
            (WritingMode::VerticalRl, 0.0, 20.0),
            (WritingMode::VerticalLr, 0.0, 20.0),
            (WritingMode::SidewaysRl, 0.0, 20.0),
            (WritingMode::SidewaysLr, 40.0, 20.0),
        ] {
            for (baseline_type, expected) in [(BaselineType::Alphabetic, alphabetic), (BaselineType::Central, central)]
            {
                let context = BaselineContext { writing_mode, baseline_type };
                assert_eq!(context.resolve(Point::NONE, writing_mode, size, false), expected, "{context:?}");
            }
        }
    }

    #[test]
    fn actual_baselines_are_axis_specific_and_require_compatible_writing_modes() {
        let size = Size { width: 40.0, height: 60.0 };
        let baselines = Point { x: Some(13.0), y: Some(29.0) };
        for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl, WritingMode::VerticalLr] {
            let context = BaselineContext { writing_mode, baseline_type: BaselineType::Central };
            let expected = if writing_mode.is_horizontal() { 29.0 } else { 13.0 };
            assert_eq!(context.real_baseline(baselines, writing_mode), Some(expected));
            assert_eq!(context.resolve(baselines, writing_mode, size, false), expected);
            let other_mode =
                if writing_mode == WritingMode::VerticalRl { WritingMode::VerticalLr } else { WritingMode::VerticalRl };
            assert_eq!(context.real_baseline(baselines, other_mode), None);
            assert_eq!(
                context.resolve(baselines, other_mode, size, false),
                if writing_mode.is_horizontal() { 30.0 } else { 20.0 }
            );
        }
    }

    #[test]
    fn only_scroll_containers_clamp_real_baselines_to_border_edges() {
        for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
            let context = BaselineContext { writing_mode, baseline_type: BaselineType::Alphabetic };
            let size = Size { width: 40.0, height: 60.0 };
            for value in [-15.0, 90.0] {
                let baselines = Point { x: Some(value), y: Some(value) };
                assert_eq!(context.resolve(baselines, writing_mode, size, false), value);
                assert_eq!(
                    context.resolve(baselines, writing_mode, size, true),
                    value.clamp(0.0, size.get_abs(writing_mode.block_axis()))
                );
            }
        }
    }

    #[test]
    fn opposite_groups_never_combine_unrelated_ascents_and_descents() {
        let mut groups = BaselineGroups::default();
        let min = BaselineMetrics { side: BaselineSide::Min, ascent: 70.0, descent: 10.0 };
        let max = BaselineMetrics { side: BaselineSide::Max, ascent: 10.0, descent: 70.0 };
        groups.add(min);
        groups.add(max);
        assert_eq!(groups.cross_size(), 80.0);
        assert_eq!(groups.alignment_offset(min, 20.0), 0.0);
        assert_eq!(groups.alignment_offset(max, 20.0), 20.0);
        assert_eq!(groups.baseline(100.0, false), Some(70.0));
        assert_eq!(groups.baseline(100.0, true), Some(90.0));
    }

    #[test]
    fn signed_margin_metrics_are_not_clamped_before_group_collection() {
        let mut groups = BaselineGroups::default();
        let item = BaselineMetrics { side: BaselineSide::Min, ascent: -10.0, descent: 40.0 };
        groups.add(item);
        assert_eq!(groups.cross_size(), 30.0);
        assert_eq!(groups.alignment_offset(item, 20.0), 0.0);
        groups.add(BaselineMetrics { side: BaselineSide::Min, ascent: -5.0, descent: 20.0 });
        assert_eq!(groups.cross_size(), 35.0);
        assert_eq!(groups.alignment_offset(item, 20.0), 5.0);
    }
}
