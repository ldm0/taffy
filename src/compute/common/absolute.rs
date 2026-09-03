use crate::compute::common::aspect_ratio::ResolvedAxisConstraints;
use crate::compute::common::intrinsic_size::{
    resolve_content_based_block_size_constraints, BlockSizeProperties, ContentBasedBlockSize,
    RatioDependentAutomaticMinimum,
};
#[cfg(any(feature = "block_layout", feature = "flexbox", feature = "grid"))]
use crate::geometry::AbsoluteAxis;
#[cfg(any(feature = "block_layout", feature = "flexbox", feature = "grid"))]
use crate::geometry::Rect;
use crate::geometry::{Size, WritingMode};
#[cfg(any(feature = "block_layout", feature = "flexbox", feature = "grid"))]
use crate::style::AvailableSpace;
use crate::style::{Dimension, ResolvedAspectRatio};
#[cfg(any(feature = "block_layout", feature = "flexbox"))]
use crate::tree::LayoutPartialTreeExt;
use crate::tree::{ChildLayoutInput, LayoutPartialTree, NodeId};
use crate::util::MaybeMath;
use crate::AutoSizeBehavior;

/// Definite sizing opportunities established by an absolute containing block.
///
/// CSS Position first derives an inset-modified containing block (IMCB). An
/// authored `stretch` size fits its margin box into that IMCB in every inset
/// configuration, while an automatic size only stretches when neither inset
/// in the axis is `auto`. Keeping both results in one value prevents Block,
/// Flex, and Grid from assigning different meanings to the same insets.
#[cfg(any(feature = "block_layout", feature = "flexbox", feature = "grid"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InsetModifiedContainingBlock {
    /// IMCB size before the positioned box's margins are removed.
    margin_box_opportunity: Size<f32>,
    /// Border-box opportunity used by authored `stretch` properties.
    stretch_border_box_opportunity: Size<f32>,
    /// Whether either inset in each physical axis was authored as `auto`.
    has_auto_inset: Size<bool>,
}

#[cfg(any(feature = "block_layout", feature = "flexbox", feature = "grid"))]
impl InsetModifiedContainingBlock {
    /// Build the physical IMCB sizing opportunities for a positioned box.
    ///
    /// When both insets are `auto`, the containing formatting context supplies
    /// the opportunity established by its static-position/alignment rules. If
    /// either inset is specified, an `auto` opposite inset contributes zero.
    /// Negative IMCB sizes clamp to zero before margins, matching CSS Position.
    pub fn new(
        containing_size: Size<f32>,
        insets: Rect<Option<f32>>,
        both_auto_opportunity: Size<f32>,
        margins: Rect<Option<f32>>,
    ) -> Self {
        let resolve_axis = |containing: f32,
                            start: Option<f32>,
                            end: Option<f32>,
                            both_auto: f32,
                            margin_start: Option<f32>,
                            margin_end: Option<f32>| {
            let both_insets_auto = start.is_none() && end.is_none();
            let margin_box_opportunity =
                if both_insets_auto { both_auto } else { containing - start.unwrap_or(0.0) - end.unwrap_or(0.0) }
                    .max(0.0);
            let stretch_border_box_opportunity =
                (margin_box_opportunity - margin_start.unwrap_or(0.0) - margin_end.unwrap_or(0.0)).max(0.0);
            (margin_box_opportunity, stretch_border_box_opportunity, start.is_none() || end.is_none())
        };

        let horizontal = resolve_axis(
            containing_size.width,
            insets.left,
            insets.right,
            both_auto_opportunity.width,
            margins.left,
            margins.right,
        );
        let vertical = resolve_axis(
            containing_size.height,
            insets.top,
            insets.bottom,
            both_auto_opportunity.height,
            margins.top,
            margins.bottom,
        );

        Self {
            margin_box_opportunity: Size { width: horizontal.0, height: vertical.0 },
            stretch_border_box_opportunity: Size { width: horizontal.1, height: vertical.1 },
            has_auto_inset: Size { width: horizontal.2, height: vertical.2 },
        }
    }

    /// Definite IMCB size supplied to a child before its own margins are removed.
    pub const fn margin_box_opportunity(self) -> Size<f32> {
        self.margin_box_opportunity
    }

    /// Definite border-box opportunity for preferred/minimum/maximum `stretch`.
    pub(crate) fn authored_stretch_available_space(self) -> Size<AvailableSpace> {
        self.stretch_border_box_opportunity.map(AvailableSpace::Definite)
    }

    /// Border-box fill used only by `auto` with two specified insets.
    pub(crate) fn implicit_auto_stretch_size(self) -> Size<Option<f32>> {
        Size {
            width: (!self.has_auto_inset.width).then_some(self.stretch_border_box_opportunity.width),
            height: (!self.has_auto_inset.height).then_some(self.stretch_border_box_opportunity.height),
        }
    }

    /// Inset- and margin-adjusted space used by fit-content measurements.
    pub(crate) const fn stretch_border_box_opportunity(self) -> Size<f32> {
        self.stretch_border_box_opportunity
    }
}

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

impl AbsoluteBoxSizing {
    /// Resolve the content-based automatic minimum activated when a preferred
    /// size in `axis` came from the opposite axis through `aspect-ratio`.
    ///
    /// Out-of-flow boxes do not contribute this measurement dependency to the
    /// containing block, but Block, Flex, and Grid must still use the same
    /// source-ordered clamp for the positioned fragment itself.
    pub(crate) fn resolve_ratio_automatic_minimum(
        mut self,
        tree: &mut impl LayoutPartialTree,
        node: NodeId,
        inputs: ChildLayoutInput,
        axis: AbsoluteAxis,
        automatic_minimum: Option<RatioDependentAutomaticMinimum>,
    ) -> Self {
        let Some(automatic_minimum) = automatic_minimum else {
            return self;
        };
        let resolved = automatic_minimum.resolve_for_node(tree, node, inputs, axis);

        match axis {
            AbsoluteAxis::Horizontal => {
                self.min_size.width = resolved.min_size;
                self.max_size.width = resolved.max_size;
                self.size.width = self.size.width.maybe_clamp(resolved.min_size, resolved.max_size);
            }
            AbsoluteAxis::Vertical => {
                self.min_size.height = resolved.min_size;
                self.max_size.height = resolved.max_size;
                self.size.height = self.size.height.maybe_clamp(resolved.min_size, resolved.max_size);
            }
        }
        self
    }
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
            block_auto_behavior,
            AvailableSpace::MaxContent,
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
        let intrinsic = resolve_content_based_block_size_constraints(tree, node, child_input, self.content_size);
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

#[cfg(all(test, any(feature = "block_layout", feature = "flexbox", feature = "grid")))]
mod tests {
    use super::*;

    #[test]
    fn imcb_separates_authored_stretch_from_implicit_auto_stretch() {
        let one_sided = InsetModifiedContainingBlock::new(
            Size { width: 400.0, height: 300.0 },
            Rect { left: None, right: None, top: Some(10.0), bottom: None },
            Size { width: 180.0, height: 90.0 },
            Rect { left: Some(7.0), right: Some(11.0), top: Some(7.0), bottom: Some(11.0) },
        );

        assert_eq!(one_sided.margin_box_opportunity(), Size { width: 180.0, height: 290.0 });
        assert_eq!(one_sided.stretch_border_box_opportunity(), Size { width: 162.0, height: 272.0 });
        assert_eq!(one_sided.implicit_auto_stretch_size(), Size::NONE);
        assert_eq!(
            one_sided.authored_stretch_available_space(),
            Size { width: AvailableSpace::Definite(162.0), height: AvailableSpace::Definite(272.0) }
        );

        let fully_inset = InsetModifiedContainingBlock::new(
            Size { width: 400.0, height: 300.0 },
            Rect { left: Some(10.0), right: Some(20.0), top: Some(10.0), bottom: Some(20.0) },
            Size::ZERO,
            Rect { left: Some(7.0), right: Some(11.0), top: Some(7.0), bottom: Some(11.0) },
        );
        assert_eq!(fully_inset.margin_box_opportunity(), Size { width: 370.0, height: 270.0 });
        assert_eq!(fully_inset.implicit_auto_stretch_size(), Size { width: Some(352.0), height: Some(252.0) });
    }

    #[test]
    fn imcb_clamps_before_negative_margins_expand_stretch() {
        let imcb = InsetModifiedContainingBlock::new(
            Size { width: 100.0, height: 100.0 },
            Rect { left: Some(80.0), right: Some(80.0), top: None, bottom: None },
            Size { width: 100.0, height: 100.0 },
            Rect { left: Some(-10.0), right: Some(-10.0), top: None, bottom: None },
        );

        assert_eq!(imcb.margin_box_opportunity().width, 0.0);
        assert_eq!(imcb.stretch_border_box_opportunity().width, 20.0);
    }
}
