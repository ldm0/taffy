//! Shared conversion between authored CSS sizes and layout sizing boxes.

use crate::{BoxSizing, MaybeMath, Size};

/// Floor already-resolved border-box sizes at their padding-and-border inset.
///
/// CSS used border-box dimensions cannot expose a negative content box. Blink
/// establishes the same invariant in `ResolveInlineLengthInternal` and
/// `ResolveBlockLengthInternal`, before preferred sizes or min/max constraints
/// participate in aspect-ratio transfer.
#[inline(always)]
pub(crate) fn floor_border_box_size(size: Size<Option<f32>>, padding_border: Size<f32>) -> Size<Option<f32>> {
    size.maybe_max(padding_border)
}

/// Convert resolved authored dimensions to content-box dimensions.
#[inline(always)]
pub(crate) fn authored_size_to_content_box(
    size: Size<Option<f32>>,
    box_sizing: BoxSizing,
    padding_border: Size<f32>,
) -> Size<Option<f32>> {
    match box_sizing {
        BoxSizing::BorderBox => floor_border_box_size(size, padding_border).maybe_sub(padding_border),
        BoxSizing::ContentBox => size.maybe_max(Size::ZERO),
    }
}

/// Convert a parent-owned border-box constraint to content-box dimensions.
#[inline(always)]
pub(crate) fn border_box_to_content_box(size: Size<Option<f32>>, padding_border: Size<f32>) -> Size<Option<f32>> {
    floor_border_box_size(size, padding_border).maybe_sub(padding_border)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_box_conversion_never_exposes_negative_content_space() {
        let padding_border = Size { width: 40.0, height: 30.0 };
        let authored = Size { width: Some(20.0), height: Some(50.0) };

        assert_eq!(
            authored_size_to_content_box(authored, BoxSizing::BorderBox, padding_border),
            Size { width: Some(0.0), height: Some(20.0) }
        );
    }

    #[test]
    fn content_box_conversion_clamps_negative_resolved_sizes() {
        let padding_border = Size { width: 40.0, height: 30.0 };
        let authored = Size { width: Some(-10.0), height: Some(20.0) };

        assert_eq!(
            authored_size_to_content_box(authored, BoxSizing::ContentBox, padding_border),
            Size { width: Some(0.0), height: Some(20.0) }
        );
    }

    #[test]
    fn parent_border_box_conversion_uses_the_padding_border_floor() {
        let padding_border = Size { width: 40.0, height: 30.0 };
        let known_border_box = Size { width: Some(20.0), height: Some(50.0) };

        assert_eq!(
            border_box_to_content_box(known_border_box, padding_border),
            Size { width: Some(0.0), height: Some(20.0) }
        );
    }
}
