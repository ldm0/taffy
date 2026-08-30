use crate::compute::common::aspect_ratio::ResolvedAxisConstraints;
use crate::compute::common::intrinsic_size::{
    measure_content_based_block_size, BlockSizeProperties, ContentBasedBlockSize,
};
#[cfg(any(feature = "block_layout", feature = "flexbox"))]
use crate::geometry::AbsoluteAxis;
use crate::geometry::{Size, WritingMode};
#[cfg(any(feature = "block_layout", feature = "flexbox"))]
use crate::style::AvailableSpace;
use crate::style::{Dimension, ResolvedAspectRatio};
#[cfg(any(feature = "block_layout", feature = "flexbox"))]
use crate::tree::LayoutPartialTreeExt;
use crate::tree::{ChildLayoutInput, LayoutPartialTree, NodeId};
use crate::util::MaybeMath;
use crate::AutoSizeBehavior;

/// Preferred and limiting border-box dimensions while resolving an
/// absolutely positioned box.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AbsoluteBoxSizing {
    /// Preferred dimensions after inset, fit-content, and ratio resolution.
    pub(crate) size: Size<Option<f32>>,
    /// Resolved minimum dimensions.
    pub(crate) min_size: Size<Option<f32>>,
    /// Resolved maximum dimensions.
    pub(crate) max_size: Size<Option<f32>>,
}

/// Authored and already-resolved state required to finish one absolute
/// logical block axis.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AbsoluteBlockSizeInput {
    /// Writing mode that defines the box's logical block axis.
    pub(crate) writing_mode: WritingMode,
    /// Authored preferred physical dimensions.
    pub(crate) size: Size<Dimension>,
    /// Authored minimum physical dimensions.
    pub(crate) min_size: Size<Dimension>,
    /// Authored maximum physical dimensions.
    pub(crate) max_size: Size<Dimension>,
    /// Used preferred aspect ratio, including its sizing box.
    pub(crate) aspect_ratio: Option<ResolvedAspectRatio>,
    /// Physical padding-and-border sums.
    pub(crate) padding_border: Size<f32>,
    /// Resolution behavior for an authored logical `block-size: auto`.
    pub(crate) block_auto_behavior: AutoSizeBehavior,
    /// Whether overflow suppresses the ratio-dependent automatic minimum.
    pub(crate) is_scroll_container: bool,
    /// Whether replaced sizing bypasses the non-replaced automatic minimum.
    pub(crate) is_replaced: bool,
    /// Authored and ratio-transferred block constraints with provenance.
    pub(crate) constraint_sources: ResolvedAxisConstraints,
}

/// Content-dependent logical block constraints shared by every absolute
/// containing-block implementation.
///
/// Block, Flex, and Grid determine the available inline size differently, but
/// CSS Sizing resolves `block-size`, `min-block-size`, and `max-block-size`
/// identically once that inline size is known. Keeping that state here avoids
/// formatting-context-specific intrinsic-keyword paths.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AbsoluteBlockSizeResolver {
    /// Writing mode that maps the shared logical result back to physical axes.
    writing_mode: WritingMode,
    /// Content-dependent preferred and limiting block-size state.
    content_size: ContentBasedBlockSize,
    /// Authored and ratio-transferred constraints retained before late resolution.
    constraint_sources: ResolvedAxisConstraints,
    /// Physical padding-and-border sums that floor the used border box.
    padding_border: Size<f32>,
}

impl AbsoluteBlockSizeResolver {
    /// Capture authored and ratio-transferred block-axis constraints before
    /// the parent formatting algorithm resolves insets and fit-content sizing.
    pub(crate) fn new(input: AbsoluteBlockSizeInput) -> Self {
        let AbsoluteBlockSizeInput {
            writing_mode,
            size,
            min_size,
            max_size,
            aspect_ratio,
            padding_border,
            block_auto_behavior,
            is_scroll_container,
            is_replaced,
            constraint_sources,
        } = input;
        let size = writing_mode.to_logical(size);
        let min_size = writing_mode.to_logical(min_size);
        let max_size = writing_mode.to_logical(max_size);
        let properties = BlockSizeProperties::new(size.block_size, min_size.block_size, max_size.block_size);
        let content_size = ContentBasedBlockSize::new(
            properties,
            aspect_ratio,
            padding_border,
            block_auto_behavior.is_content_based(aspect_ratio.is_some()),
            is_scroll_container,
            is_replaced,
        );
        Self { writing_mode, content_size, constraint_sources, padding_border }
    }

    /// Measure and merge late content-derived block constraints after the
    /// absolute parent has completed inline-axis sizing.
    pub(crate) fn resolve(
        self,
        tree: &mut impl LayoutPartialTree,
        node: NodeId,
        child_input: ChildLayoutInput,
        sizing: AbsoluteBoxSizing,
    ) -> AbsoluteBoxSizing {
        let intrinsic = measure_content_based_block_size(tree, node, child_input, self.content_size);
        let mut logical_size = self.writing_mode.to_logical(sizing.size);
        let mut logical_min_size = self.writing_mode.to_logical(sizing.min_size);
        let mut logical_max_size = self.writing_mode.to_logical(sizing.max_size);
        let resolved = intrinsic.resolve_against(logical_size.block_size, self.constraint_sources);
        let minimum_border_box_size = self.writing_mode.to_logical(self.padding_border).block_size;

        logical_min_size.block_size =
            resolved.min.or(logical_min_size.block_size).map(|size| size.max(minimum_border_box_size));
        logical_max_size.block_size = resolved.max.or(logical_max_size.block_size);
        logical_size.block_size = logical_size
            .block_size
            .or(resolved.preferred)
            .maybe_clamp(logical_min_size.block_size, logical_max_size.block_size)
            .map(|size| size.max(minimum_border_box_size));

        AbsoluteBoxSizing {
            size: self.writing_mode.to_physical(logical_size),
            min_size: self.writing_mode.to_physical(logical_min_size),
            max_size: self.writing_mode.to_physical(logical_max_size),
        }
    }
}

/// Resolves the fit-content width used by an auto-width absolutely positioned box.
///
/// CSS 2 defines this as `min(max(min-content, available), max-content)`. A single
/// measurement with definite available space is insufficient: nested block and flex
/// containers may return their max-content contribution while they are being measured.
#[cfg(any(feature = "block_layout", feature = "flexbox"))]
#[inline]
pub(crate) fn fit_content_width(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    mut inputs: ChildLayoutInput,
    available_width: f32,
) -> f32 {
    inputs.available_space.width = AvailableSpace::MinContent;
    let min_content = tree.measure_child_size(node, inputs, AbsoluteAxis::Horizontal);
    inputs.available_space.width = AvailableSpace::MaxContent;
    let max_content = tree.measure_child_size(node, inputs, AbsoluteAxis::Horizontal);

    available_width.max(0.0).max(min_content).min(max_content)
}
