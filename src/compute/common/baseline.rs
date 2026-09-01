//! Conversion between physical baseline sets and a formatting context's
//! logical block axis.

use crate::geometry::{Point, Size, WritingDirection};

/// Project a physical baseline into a formatting context's logical block axis.
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
}
