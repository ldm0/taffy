//! Conversion between physical baseline sets and a formatting context's
//! logical block axis.

use crate::geometry::{Point, Size, WritingDirection};

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
pub(crate) fn synthesized_logical_baseline(block_size: f32, writing_direction: WritingDirection) -> f32 {
    if writing_direction.mode.is_line_direction_flipped() {
        0.0
    } else {
        block_size
    }
}

/// Project a fragment baseline into logical block coordinates, synthesizing
/// an alphabetic baseline when the fragment cannot supply one on that axis.
pub(crate) fn logical_block_baseline_or_synthesize(
    baseline: Point<Option<f32>>,
    child_size: Size<f32>,
    writing_direction: WritingDirection,
) -> f32 {
    logical_block_baseline(baseline, child_size, writing_direction).unwrap_or_else(|| {
        synthesized_logical_baseline(writing_direction.mode.to_logical(child_size).block_size, writing_direction)
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
    use crate::{Direction, WritingMode};

    #[test]
    fn baseline_round_trips_through_reversed_vertical_block_flow() {
        let writing_direction = WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr);
        let size = Size { width: 100.0, height: 40.0 };
        let physical = physical_baseline(Some(30.0), size, writing_direction);
        assert_eq!(physical, Point { x: Some(70.0), y: None });
        assert_eq!(logical_block_baseline(physical, size, writing_direction), Some(30.0));
    }

    #[test]
    fn synthesized_baseline_uses_the_line_under_edge() {
        assert_eq!(
            synthesized_logical_baseline(40.0, WritingDirection::new(WritingMode::HorizontalTb, Direction::Ltr),),
            40.0,
        );
        assert_eq!(
            synthesized_logical_baseline(40.0, WritingDirection::new(WritingMode::VerticalLr, Direction::Ltr)),
            0.0,
        );
        assert_eq!(
            synthesized_logical_baseline(40.0, WritingDirection::new(WritingMode::VerticalRl, Direction::Ltr)),
            40.0,
        );
    }
}
