//! The sizing contract shared by grid track measurement and final placement.

use crate::compute::common::aspect_ratio::{resolve_size_constraints, ResolvedSizeConstraints, SizeConstraintInput};
use crate::{AlignItemsKeyword, AlignSelf, AutoSizeBehavior, MaybeMath, Size};

/// Alignment chooses automatic sizing independently of the position edge.
/// Auto margins disable stretching; normal replaced items use their content.
pub(super) fn auto_size_behavior(alignment: AlignSelf, has_auto_margin: bool, is_replaced: bool) -> AutoSizeBehavior {
    if has_auto_margin {
        return AutoSizeBehavior::FitContent;
    }
    match alignment.keyword() {
        AlignItemsKeyword::Stretch => AutoSizeBehavior::StretchExplicit,
        AlignItemsKeyword::Normal if !is_replaced => AutoSizeBehavior::StretchImplicit,
        _ => AutoSizeBehavior::FitContent,
    }
}

/// A grid area's sizing proposal, before the child produces its final fragment.
pub(super) struct GridItemSizing {
    /// Ratio and limit sources retained for content-dependent minimums.
    pub(super) constraints: ResolvedSizeConstraints,
    /// Only independent block constraints cross the formatter boundary as fixed.
    pub(super) known_dimensions: Size<Option<f32>>,
    /// Child-logical block sizing selected by its physical grid-area axis.
    pub(super) block_auto_behavior: AutoSizeBehavior,
}

/// Resolve independent constraints before deriving the dependent ratio axis.
/// Explicit stretch is not a candidate for opposite-axis transfers; implicit
/// normal sizing still respects constraints transferred through the ratio.
/// A content-derived block size remains owned by the actual child formatter.
pub(super) fn resolve_item_sizing(
    mut input: SizeConstraintInput,
    available: Size<Option<f32>>,
    behavior: Size<AutoSizeBehavior>,
) -> GridItemSizing {
    let mode = input.writing_mode;
    let behavior = mode.to_logical(behavior);
    let available = mode.to_logical(available);
    let authored_auto = mode.to_logical(input.size_is_auto);
    let mut size = mode.to_logical(input.size);
    let mut can_transfer = authored_auto;
    let block_stretches = behavior.block_size == AutoSizeBehavior::StretchExplicit
        || (behavior.block_size == AutoSizeBehavior::StretchImplicit && input.aspect_ratio.is_none());
    if authored_auto.block_size && size.block_size.is_none() && block_stretches {
        size.block_size = available.block_size;
        can_transfer.block_size = size.block_size.is_none() || behavior.block_size != AutoSizeBehavior::StretchExplicit;
    }
    let ratio_from_block = input.aspect_ratio.is_some()
        && size.block_size.is_some()
        && behavior.inline_size != AutoSizeBehavior::StretchExplicit;
    let inline_stretches = behavior.inline_size == AutoSizeBehavior::StretchExplicit
        || (behavior.inline_size == AutoSizeBehavior::StretchImplicit && !ratio_from_block);
    if authored_auto.inline_size && size.inline_size.is_none() && inline_stretches {
        size.inline_size = available.inline_size;
        can_transfer.inline_size =
            size.inline_size.is_none() || behavior.inline_size != AutoSizeBehavior::StretchExplicit;
    }
    input.size = mode.to_physical(size).maybe_clamp(input.min_size, input.max_size);
    input.size_is_auto = mode.to_physical(can_transfer);
    input.block_auto_behavior = behavior.block_size;
    let independent_block = mode.to_logical(input.size).block_size;
    let constraints = resolve_size_constraints(input);
    let mut known = mode.to_logical(constraints.size.maybe_clamp(constraints.min_size, constraints.max_size));
    if independent_block.is_none() {
        known.block_size = None;
    }
    GridItemSizing { constraints, known_dimensions: mode.to_physical(known), block_auto_behavior: behavior.block_size }
}
