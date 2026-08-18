//! Generic CSS alignment code that is shared between both the Flexbox and CSS Grid algorithms.
use crate::style::{AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword, AlignmentSafety};
use crate::tree::AutoSizeBehavior;

/// A self-alignment value after a formatting context has supplied the meaning
/// of `normal`, together with the corresponding behavior for an authored
/// `auto` size.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct ResolvedSelfAlignment {
    /// Positional alignment used after sizing.
    pub position: AlignItems,
    /// Ordering between automatic sizing, stretch, and preferred ratios.
    pub auto_size: AutoSizeBehavior,
}

/// Resolve the context-dependent sizing semantics of one self-alignment value.
///
/// The formatting model supplies both the positional and auto-size behavior
/// of `normal`. Explicit `stretch` is a strong stretch; all positional values
/// remain content-sized.
#[inline]
pub(crate) const fn resolve_self_alignment(
    alignment: AlignItems,
    normal_position: AlignItems,
    normal_auto_size: AutoSizeBehavior,
) -> ResolvedSelfAlignment {
    match alignment.keyword() {
        AlignItemsKeyword::Normal => ResolvedSelfAlignment { position: normal_position, auto_size: normal_auto_size },
        AlignItemsKeyword::Stretch => {
            ResolvedSelfAlignment { position: alignment, auto_size: AutoSizeBehavior::StretchExplicit }
        }
        _ => ResolvedSelfAlignment { position: alignment, auto_size: AutoSizeBehavior::FitContent },
    }
}

/// A content-alignment keyword after context-dependent fallbacks have been
/// resolved. Baseline preferences deliberately cannot reach numeric offset
/// code: a layout context either performs baseline sharing before this point
/// or resolves them to their spec-defined safe start/end fallback.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ResolvedAlignContentKeyword {
    /// Logical start-edge positioning.
    Start,
    /// Logical end-edge positioning.
    End,
    /// Flex-relative start-edge positioning.
    FlexStart,
    /// Flex-relative end-edge positioning.
    FlexEnd,
    /// Center positioning.
    Center,
    /// Distributed stretching remains applicable.
    Stretch,
    /// Distribute free space between subjects.
    SpaceBetween,
    /// Distribute equal free space around and between subjects.
    SpaceEvenly,
    /// Distribute half-sized free space at the outer edges.
    SpaceAround,
}

impl ResolvedAlignContentKeyword {
    /// Reverse logical/flex-relative edges for a reversed alignment axis.
    pub(crate) fn reversed(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
            Self::FlexStart => Self::FlexEnd,
            Self::FlexEnd => Self::FlexStart,
            Self::Stretch => Self::End,
            Self::Center | Self::SpaceBetween | Self::SpaceEvenly | Self::SpaceAround => self,
        }
    }

    /// Reverse only logical `start`/`end` edges while retaining flex-relative
    /// and distributed positions.
    ///
    /// Flex main-axis placement uses a physical low-to-high offset on a
    /// vertical axis. When logical main-start is the physical high edge, only
    /// logical positions need projection: `flex-start` already follows the
    /// independently normalized flex direction.
    pub(crate) fn logical_edges_reversed(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
            other => other,
        }
    }
}

/// Resolve the `safe`/`unsafe` overflow-position fallback for a self-level alignment value
/// (used by `align-self` / `justify-self`-style sites and by absolutely-positioned items in
/// flex/grid). If the alignment subject overflows its alignment container and the requested
/// alignment is `safe`, fall back to logical `Start` per CSS Box Alignment
/// <https://www.w3.org/TR/css-align-3/#overflow-values>. Otherwise drop the safety modifier
/// and return the bare keyword.
#[inline]
pub(crate) fn resolve_self_alignment_safety(alignment: AlignItems, overflows: bool) -> AlignItemsKeyword {
    if matches!(alignment.safety, AlignmentSafety::Safe) && overflows {
        AlignItemsKeyword::Start
    } else {
        alignment.keyword
    }
}

/// Resolve any spec-defined fallbacks for the given [`AlignContent`] value, returning the
/// bare position keyword the alignment math should use.
///
/// In addition to the spec at <https://www.w3.org/TR/css-align-3/> this implementation follows
/// the resolution of <https://github.com/w3c/csswg-drafts/issues/10154>.
pub fn apply_alignment_fallback(
    free_space: f32,
    num_items: usize,
    alignment_mode: AlignContent,
) -> ResolvedAlignContentKeyword {
    let mut is_safe = matches!(alignment_mode.safety, AlignmentSafety::Safe);

    // Baseline content alignment only operates inside a baseline-sharing
    // context. These generic content-distribution sites have no such group,
    // so use the normative positional fallback while retaining its implicit
    // overflow safety.
    let mut keyword = match alignment_mode.keyword {
        AlignContentKeyword::Start => ResolvedAlignContentKeyword::Start,
        AlignContentKeyword::End => ResolvedAlignContentKeyword::End,
        AlignContentKeyword::FlexStart => ResolvedAlignContentKeyword::FlexStart,
        AlignContentKeyword::FlexEnd => ResolvedAlignContentKeyword::FlexEnd,
        AlignContentKeyword::Left | AlignContentKeyword::Right => {
            unreachable!("physical content alignment must be resolved at the formatting-context boundary")
        }
        AlignContentKeyword::Center => ResolvedAlignContentKeyword::Center,
        AlignContentKeyword::Baseline => {
            is_safe = true;
            ResolvedAlignContentKeyword::Start
        }
        AlignContentKeyword::LastBaseline => {
            is_safe = true;
            ResolvedAlignContentKeyword::End
        }
        AlignContentKeyword::Stretch => ResolvedAlignContentKeyword::Stretch,
        AlignContentKeyword::SpaceBetween => ResolvedAlignContentKeyword::SpaceBetween,
        AlignContentKeyword::SpaceEvenly => ResolvedAlignContentKeyword::SpaceEvenly,
        AlignContentKeyword::SpaceAround => ResolvedAlignContentKeyword::SpaceAround,
    };

    // 1. If there is only a single item being aligned or the items overflow the container, the
    //    distributed alignment keywords (`stretch`, `space-*`) fall back to a positional keyword
    //    and gain implicit `safe` semantics so step 2 can flip them to `Start` on overflow.
    //    https://www.w3.org/TR/css-align-3/#distribution-values
    if num_items <= 1 || free_space <= 0.0 {
        (keyword, is_safe) = match keyword {
            ResolvedAlignContentKeyword::Stretch | ResolvedAlignContentKeyword::SpaceBetween => {
                (ResolvedAlignContentKeyword::FlexStart, true)
            }
            ResolvedAlignContentKeyword::SpaceAround | ResolvedAlignContentKeyword::SpaceEvenly => {
                (ResolvedAlignContentKeyword::Center, true)
            }
            other => (other, is_safe),
        };
    }

    // 2. Safe alignment falls back to `Start` whenever the alignment subject would overflow the
    //    alignment container.
    if free_space <= 0.0 && is_safe {
        keyword = ResolvedAlignContentKeyword::Start;
    }

    keyword
}

/// Generic alignment function that is used:
///   - For both align-content and justify-content alignment
///   - For both the Flexbox and CSS Grid algorithms
///
/// CSS Grid does not apply gaps as part of alignment, so the gap parameter should
/// always be set to zero for CSS Grid.
pub fn compute_alignment_offset(
    free_space: f32,
    num_items: usize,
    gap: f32,
    alignment_mode: ResolvedAlignContentKeyword,
    layout_is_flex_reversed: bool,
    is_first: bool,
) -> f32 {
    if is_first {
        match alignment_mode {
            ResolvedAlignContentKeyword::Start => 0.0,
            ResolvedAlignContentKeyword::FlexStart => {
                if layout_is_flex_reversed {
                    free_space
                } else {
                    0.0
                }
            }
            ResolvedAlignContentKeyword::End => free_space,
            ResolvedAlignContentKeyword::FlexEnd => {
                if layout_is_flex_reversed {
                    0.0
                } else {
                    free_space
                }
            }
            ResolvedAlignContentKeyword::Center => free_space / 2.0,
            ResolvedAlignContentKeyword::Stretch => 0.0,
            ResolvedAlignContentKeyword::SpaceBetween => 0.0,
            ResolvedAlignContentKeyword::SpaceAround => {
                if free_space >= 0.0 {
                    (free_space / num_items as f32) / 2.0
                } else {
                    free_space / 2.0
                }
            }
            ResolvedAlignContentKeyword::SpaceEvenly => {
                if free_space >= 0.0 {
                    free_space / (num_items + 1) as f32
                } else {
                    free_space / 2.0
                }
            }
        }
    } else {
        let free_space = free_space.max(0.0);
        gap + match alignment_mode {
            ResolvedAlignContentKeyword::Start
            | ResolvedAlignContentKeyword::FlexStart
            | ResolvedAlignContentKeyword::End
            | ResolvedAlignContentKeyword::FlexEnd
            | ResolvedAlignContentKeyword::Center
            | ResolvedAlignContentKeyword::Stretch => 0.0,
            ResolvedAlignContentKeyword::SpaceBetween => free_space / (num_items - 1) as f32,
            ResolvedAlignContentKeyword::SpaceAround => free_space / num_items as f32,
            ResolvedAlignContentKeyword::SpaceEvenly => free_space / (num_items + 1) as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_content_fallbacks_preserve_preference_and_safety() {
        assert_eq!(apply_alignment_fallback(40.0, 2, AlignContent::BASELINE), ResolvedAlignContentKeyword::Start);
        assert_eq!(apply_alignment_fallback(40.0, 2, AlignContent::LAST_BASELINE), ResolvedAlignContentKeyword::End);
        assert_eq!(apply_alignment_fallback(-10.0, 2, AlignContent::LAST_BASELINE), ResolvedAlignContentKeyword::Start);
    }

    #[test]
    fn resolved_content_alignment_reverses_only_directional_edges() {
        assert_eq!(ResolvedAlignContentKeyword::Start.reversed(), ResolvedAlignContentKeyword::End);
        assert_eq!(ResolvedAlignContentKeyword::FlexStart.reversed(), ResolvedAlignContentKeyword::FlexEnd);
        assert_eq!(ResolvedAlignContentKeyword::Stretch.reversed(), ResolvedAlignContentKeyword::End);
        assert_eq!(ResolvedAlignContentKeyword::Center.reversed(), ResolvedAlignContentKeyword::Center);
        assert_eq!(ResolvedAlignContentKeyword::SpaceBetween.reversed(), ResolvedAlignContentKeyword::SpaceBetween);
        assert_eq!(ResolvedAlignContentKeyword::Start.logical_edges_reversed(), ResolvedAlignContentKeyword::End);
        assert_eq!(
            ResolvedAlignContentKeyword::FlexStart.logical_edges_reversed(),
            ResolvedAlignContentKeyword::FlexStart
        );
    }
}
