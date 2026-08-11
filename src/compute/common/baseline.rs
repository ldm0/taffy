//! Baseline projection, synthesis, and baseline-sharing group selection.

use crate::geometry::{Point, Size, WritingDirection};
use crate::{Direction, WritingMode};

/// One of the two baseline-sharing groups that may occupy an alignment axis.
/// The major group is placed toward the axis start and the minor group toward
/// its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BaselineGroup {
    /// Baselines aligned toward the alignment axis start.
    Major,
    /// Baselines aligned toward the alignment axis end.
    Minor,
}

/// Font baseline used when an alignment subject cannot expose a compatible
/// fragment baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FontBaseline {
    /// Alphabetic baseline synthesized on the line-under edge.
    Alphabetic,
    /// Central baseline synthesized halfway through the alignment axis.
    Central,
}

impl FontBaseline {
    /// Resolve the CSS initial baseline for a writing mode. Upright vertical
    /// text uses the central baseline; horizontal and sideways text use the
    /// alphabetic baseline.
    pub(crate) fn for_writing_mode(writing_mode: WritingMode) -> Self {
        match writing_mode {
            WritingMode::VerticalRl | WritingMode::VerticalLr => Self::Central,
            WritingMode::HorizontalTb | WritingMode::SidewaysRl | WritingMode::SidewaysLr => Self::Alphabetic,
        }
    }
}

/// Select the writing mode in which a child's baseline participates in an
/// alignment context.
///
/// This implements CSS Box Alignment's baseline-generation rules. A flex row
/// establishes a parallel alignment context; a flex column establishes a
/// perpendicular one.
pub(crate) fn determine_baseline_writing_mode(
    container: WritingDirection,
    child: WritingMode,
    is_parallel_context: bool,
) -> WritingMode {
    let orthogonal_mode = if is_parallel_context {
        container.mode
    } else if child.is_horizontal() {
        if container.direction == Direction::Ltr {
            WritingMode::VerticalLr
        } else {
            WritingMode::VerticalRl
        }
    } else {
        WritingMode::HorizontalTb
    };
    let child_is_parallel = !container.mode.is_orthogonal_to(child);

    match (is_parallel_context, child_is_parallel) {
        (true, true) | (false, false) => child,
        (true, false) | (false, true) => orthogonal_mode,
    }
}

/// Select the baseline-sharing group for a baseline writing mode.
pub(crate) fn determine_baseline_group(
    container: WritingDirection,
    baseline_writing_mode: WritingMode,
    is_parallel_context: bool,
    is_last_baseline: bool,
    is_flipped: bool,
) -> BaselineGroup {
    let mut start_group = BaselineGroup::Major;
    let mut end_group = BaselineGroup::Minor;
    if is_last_baseline {
        core::mem::swap(&mut start_group, &mut end_group);
    }
    if is_flipped {
        core::mem::swap(&mut start_group, &mut end_group);
    }

    if is_parallel_context {
        debug_assert!(!container.mode.is_orthogonal_to(baseline_writing_mode));
        return if baseline_writing_mode == container.mode { start_group } else { end_group };
    }

    match baseline_writing_mode {
        WritingMode::HorizontalTb | WritingMode::VerticalLr | WritingMode::SidewaysLr => {
            if container.direction == Direction::Ltr {
                start_group
            } else {
                end_group
            }
        }
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            if container.direction == Direction::Ltr {
                end_group
            } else {
                start_group
            }
        }
    }
}

/// Project a child's physical baseline into its parent's logical block axis.
pub(crate) fn logical_block_baseline(
    baseline: Point<Option<f32>>,
    child_size: Size<f32>,
    writing_direction: WritingDirection,
) -> Option<f32> {
    if writing_direction.mode.is_horizontal() {
        baseline.y
    } else {
        baseline.x.map(
            |offset| {
                if writing_direction.is_block_flow_reversed() {
                    child_size.width - offset
                } else {
                    offset
                }
            },
        )
    }
}

/// Synthesize the alphabetic baseline used when a child fragment has no
/// baseline in the formatting context's writing mode.
///
/// CSS synthesizes this baseline on the line-under edge. In Taffy's logical
/// block coordinates that is block-end for normal line direction and
/// block-start for flipped-line writing modes such as `vertical-lr`.
pub(crate) fn synthesized_logical_baseline(
    block_size: f32,
    writing_direction: WritingDirection,
    font_baseline: FontBaseline,
) -> f32 {
    match font_baseline {
        FontBaseline::Central => block_size / 2.0,
        FontBaseline::Alphabetic if writing_direction.mode.is_line_direction_flipped() => 0.0,
        FontBaseline::Alphabetic => block_size,
    }
}

/// Project a fragment baseline into logical block coordinates, synthesizing
/// an alphabetic baseline when the fragment cannot supply one on that axis.
pub(crate) fn logical_block_baseline_or_synthesize(
    baseline: Point<Option<f32>>,
    child_size: Size<f32>,
    writing_direction: WritingDirection,
    font_baseline: FontBaseline,
) -> f32 {
    logical_block_baseline(baseline, child_size, writing_direction).unwrap_or_else(|| {
        synthesized_logical_baseline(
            writing_direction.mode.to_logical(child_size).block_size,
            writing_direction,
            font_baseline,
        )
    })
}

/// Materialize one logical block-axis baseline in physical coordinates.
pub(crate) fn physical_baseline(
    baseline: Option<f32>,
    container_size: Size<f32>,
    writing_direction: WritingDirection,
) -> Point<Option<f32>> {
    if writing_direction.mode.is_horizontal() {
        Point { x: None, y: baseline }
    } else {
        Point {
            x: baseline.map(|offset| {
                if writing_direction.is_block_flow_reversed() {
                    container_size.width - offset
                } else {
                    offset
                }
            }),
            y: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_round_trips_through_reversed_vertical_block_flow() {
        let writing_direction = WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr);
        let size = Size { width: 100.0, height: 40.0 };
        let physical = physical_baseline(Some(30.0), size, writing_direction);
        assert_eq!(physical, Point { x: Some(70.0), y: None });
        assert_eq!(logical_block_baseline(physical, size, writing_direction), Some(30.0));
    }

    #[test]
    fn alphabetic_synthesis_uses_the_line_under_edge() {
        assert_eq!(
            synthesized_logical_baseline(
                40.0,
                WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
                FontBaseline::Alphabetic,
            ),
            40.0,
        );
        assert_eq!(
            synthesized_logical_baseline(
                40.0,
                WritingDirection::new(WritingMode::VerticalLr, Direction::Ltr),
                FontBaseline::Alphabetic,
            ),
            0.0,
        );
        assert_eq!(
            synthesized_logical_baseline(
                40.0,
                WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr),
                FontBaseline::Alphabetic,
            ),
            40.0,
        );
    }

    #[test]
    fn central_synthesis_uses_the_axis_midpoint() {
        for writing_mode in [WritingMode::HorizontalTb, WritingMode::VerticalLr, WritingMode::VerticalRl] {
            assert_eq!(
                synthesized_logical_baseline(
                    40.0,
                    WritingDirection::new(writing_mode, Direction::Ltr),
                    FontBaseline::Central,
                ),
                20.0,
            );
        }
    }

    #[test]
    fn perpendicular_baseline_modes_follow_the_container_inline_direction() {
        assert_eq!(
            determine_baseline_writing_mode(
                WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
                WritingMode::HorizontalTb,
                false,
            ),
            WritingMode::VerticalLr,
        );
        assert_eq!(
            determine_baseline_writing_mode(
                WritingDirection::new(WritingMode::HorizontalTb, Direction::Rtl),
                WritingMode::HorizontalTb,
                false,
            ),
            WritingMode::VerticalRl,
        );
    }

    #[test]
    fn baseline_groups_keep_perpendicular_ltr_and_rtl_items_in_the_major_group() {
        assert_eq!(
            determine_baseline_group(
                WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),
                WritingMode::VerticalLr,
                false,
                false,
                false,
            ),
            BaselineGroup::Major,
        );
        assert_eq!(
            determine_baseline_group(
                WritingDirection::new(WritingMode::HorizontalTb, Direction::Rtl),
                WritingMode::VerticalRl,
                false,
                false,
                false,
            ),
            BaselineGroup::Major,
        );
    }

    #[test]
    fn last_baselines_swap_the_first_baseline_groups() {
        let container = WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr);
        assert_eq!(
            determine_baseline_group(container, WritingMode::VerticalRl, true, false, false),
            BaselineGroup::Major,
        );
        assert_eq!(
            determine_baseline_group(container, WritingMode::VerticalRl, true, true, false),
            BaselineGroup::Minor,
        );
    }
}
