//! Shared preferred-size and min/max transfer rules for `aspect-ratio`.

use crate::{
    AbsoluteAxis, AutoSizeBehavior, AvailableSpace, BoxSizing, LogicalSize, MaybeMath, ResolvedAspectRatio, Size,
    WritingMode,
};

/// Preferred and limiting sizes after applying a preferred aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedSizeConstraints {
    /// Preferred border-box size after applying the ratio transfer.
    pub size: Size<Option<f32>>,
    /// Axes whose preferred size was synthesized from the opposite axis by the
    /// preferred aspect ratio.
    pub aspect_ratio_applied: Size<bool>,
    /// Used minimum constraint for the requested transfer mode.
    pub min_size: Size<Option<f32>>,
    /// Used maximum constraint for the requested transfer mode.
    pub max_size: Size<Option<f32>>,
    /// Authored and transferred sources before they become a used min/max pair.
    constraint_sources: Size<ResolvedAxisConstraints>,
}

/// Resolved min/max sources for one physical axis.
///
/// An authored maximum caps the ratio-dependent automatic minimum; a maximum
/// transferred from the opposite axis does not. Keeping those sources separate
/// preserves that observable CSS sizing order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ResolvedAxisConstraints {
    /// Minimum resolved directly from the target axis.
    authored_min: Option<f32>,
    /// Maximum resolved directly from the target axis.
    authored_max: Option<f32>,
    /// Minimum transferred from the opposite axis through the preferred ratio.
    transferred_min: Option<f32>,
    /// Maximum transferred from the opposite axis through the preferred ratio.
    transferred_max: Option<f32>,
}

impl ResolvedAxisConstraints {
    /// No authored or transferred constraints.
    const NONE: Self = Self { authored_min: None, authored_max: None, transferred_min: None, transferred_max: None };

    /// Merge late-resolved authored constraints and an automatic minimum.
    pub(crate) fn resolve(
        self,
        late_authored_min: Option<f32>,
        late_authored_max: Option<f32>,
        automatic_min: Option<f32>,
    ) -> (Option<f32>, Option<f32>) {
        let authored_min = maximum_constraint(self.authored_min, late_authored_min);
        let authored_max = minimum_constraint(self.authored_max, late_authored_max);
        let automatic_min = automatic_min.map(|minimum| minimum.min(authored_max.unwrap_or(f32::INFINITY)));
        let authored_and_automatic_min = maximum_constraint(authored_min, automatic_min);
        let used_min = merge_minimum(authored_and_automatic_min, self.transferred_min, authored_max);
        let used_max = merge_maximum(authored_max, self.transferred_max, used_min);
        (used_min, used_max)
    }

    /// Add late-resolved values of authored intrinsic sizing keywords.
    fn with_late_authored_constraints(mut self, min: Option<f32>, max: Option<f32>) -> Self {
        self.authored_min = maximum_constraint(self.authored_min, min);
        self.authored_max = minimum_constraint(self.authored_max, max);
        self
    }
}

impl ResolvedSizeConstraints {
    /// Empty constraint state used by content-only sizing operations.
    pub(crate) const NONE: Self = Self {
        size: Size::NONE,
        aspect_ratio_applied: Size { width: false, height: false },
        min_size: Size::NONE,
        max_size: Size::NONE,
        constraint_sources: Size { width: ResolvedAxisConstraints::NONE, height: ResolvedAxisConstraints::NONE },
    };

    /// Merge values of authored intrinsic min/max keywords that became known
    /// after the initial length/percentage resolution.
    pub(crate) fn apply_late_authored_constraints(&mut self, min_size: Size<Option<f32>>, max_size: Size<Option<f32>>) {
        self.constraint_sources.width =
            self.constraint_sources.width.with_late_authored_constraints(min_size.width, max_size.width);
        self.constraint_sources.height =
            self.constraint_sources.height.with_late_authored_constraints(min_size.height, max_size.height);
        let (min_width, max_width) = self.constraint_sources.width.resolve(None, None, None);
        let (min_height, max_height) = self.constraint_sources.height.resolve(None, None, None);
        self.min_size = Size { width: min_width, height: min_height };
        self.max_size = Size { width: max_width, height: max_height };
    }

    /// Return source-preserving constraints for one physical axis.
    #[inline(always)]
    pub(crate) fn axis_constraints(self, axis: AbsoluteAxis) -> ResolvedAxisConstraints {
        self.constraint_sources.get_abs(axis)
    }

    /// Return source-preserving constraints for the logical inline axis.
    #[inline(always)]
    pub(crate) fn inline_axis_constraints(self, writing_mode: WritingMode) -> ResolvedAxisConstraints {
        self.axis_constraints(writing_mode.inline_axis())
    }

    /// Return source-preserving constraints for the logical block axis.
    #[inline(always)]
    pub(crate) fn block_axis_constraints(self, writing_mode: WritingMode) -> ResolvedAxisConstraints {
        self.axis_constraints(writing_mode.block_axis())
    }
}

/// Controls whether min/max constraints from the opposite axis participate in
/// the current sizing operation.
///
/// Formatting contexts such as flexbox deliberately ignore transferred sizes
/// while resolving flexible lengths, then apply them while computing the
/// hypothetical size. Keeping that choice at the call site preserves the
/// sizing operation's semantics instead of storing parallel constraint sets in
/// the shared resolver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferredSizesMode {
    /// Apply min/max constraints transferred through the preferred ratio.
    Normal,
    /// Retain only constraints explicitly authored in the requested axis.
    Ignore,
}

/// Inputs to preferred-size and ratio constraint resolution.
///
/// Naming the constraint-space state at call sites keeps sizing order visible
/// and prevents formatting-context flags from growing a positional argument
/// list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SizeConstraintInput {
    /// Resolved preferred border-box sizes before ratio transfer.
    pub size: Size<Option<f32>>,
    /// Resolved authored minimum border-box sizes.
    pub min_size: Size<Option<f32>>,
    /// Resolved authored maximum border-box sizes.
    pub max_size: Size<Option<f32>>,
    /// Whether each preferred size was authored as `auto`.
    pub size_is_auto: Size<bool>,
    /// Writing mode that defines the logical block axis.
    pub writing_mode: WritingMode,
    /// How an authored logical inline-size of `auto` resolves in this space.
    pub inline_auto_behavior: AutoSizeBehavior,
    /// How an authored logical block-size of `auto` resolves in this space.
    pub block_auto_behavior: AutoSizeBehavior,
    /// Available border-box space for formatting-context-owned automatic
    /// sizing. An explicit stretch precedes ratio transfer only when its axis
    /// is definite; an intrinsic constraint falls back to content sizing.
    pub auto_size_available_space: Size<AvailableSpace>,
    /// Whether opposite-axis min/max constraints transfer through the ratio.
    pub transferred_sizes_mode: TransferredSizesMode,
    /// Used preferred aspect ratio and its sizing box.
    pub aspect_ratio: Option<ResolvedAspectRatio>,
    /// Physical padding-and-border sums for ratio box conversion.
    pub padding_border: Size<f32>,
}

/// Apply a preferred aspect ratio to resolved border-box sizes and merge
/// min/max sizes transferred from the opposite axis with explicitly specified
/// constraints.
///
/// A constraint is transferred into an axis only while the preferred size in
/// that axis is `auto`. An explicit maximum caps a transferred minimum, and an
/// explicit minimum floors a transferred maximum. This retains the provenance
/// that would be lost by independently applying the ratio to `min_size` and
/// `max_size` and then using the generic "minimum wins" clamp.
pub(crate) fn resolve_size_constraints(input: SizeConstraintInput) -> ResolvedSizeConstraints {
    let SizeConstraintInput {
        mut size,
        mut min_size,
        mut max_size,
        size_is_auto,
        writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        auto_size_available_space,
        transferred_sizes_mode,
        aspect_ratio,
        padding_border,
    } = input;

    // Resolved CSS lengths denote a border box at this boundary. Like
    // Blink's Resolve*Length helpers, normalize every present value before
    // ratio transfer so an undersized border-box declaration cannot squash
    // padding or border. The structural border-box minimum also participates
    // as the source of a transferred minimum while preserving `None` as the
    // authored min-size provenance in the target axis.
    size = size.maybe_max(padding_border);
    min_size = min_size.maybe_max(padding_border);
    max_size = max_size.maybe_max(padding_border);
    let transfer_min_size = min_size.or(padding_border.map(Some)).maybe_max(padding_border);
    let (transferred_min, transferred_max) = match transferred_sizes_mode {
        TransferredSizesMode::Normal => (
            transferred_constraints(transfer_min_size, size_is_auto, aspect_ratio, padding_border),
            transferred_constraints(max_size, size_is_auto, aspect_ratio, padding_border),
        ),
        TransferredSizesMode::Ignore => (Size::NONE, Size::NONE),
    };

    let constraint_sources = Size {
        width: ResolvedAxisConstraints {
            authored_min: min_size.width,
            authored_max: max_size.width,
            transferred_min: transferred_min.width,
            transferred_max: transferred_max.width,
        },
        height: ResolvedAxisConstraints {
            authored_min: min_size.height,
            authored_max: max_size.height,
            transferred_min: transferred_min.height,
            transferred_max: transferred_max.height,
        },
    };
    let (min_width, max_width) = constraint_sources.width.resolve(None, None, None);
    let (min_height, max_height) = constraint_sources.height.resolve(None, None, None);
    let min_size = Size { width: min_width, height: min_height };
    let max_size = Size { width: max_width, height: max_height };

    let resolved_size = apply_preferred_aspect_ratio(PreferredAspectRatioInput {
        size,
        authored_auto: size_is_auto,
        writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        auto_size_available_space,
        aspect_ratio,
        padding_border,
    });
    let aspect_ratio_applied = Size {
        width: size.width.is_none() && resolved_size.width.is_some(),
        height: size.height.is_none() && resolved_size.height.is_some(),
    };

    ResolvedSizeConstraints { size: resolved_size, aspect_ratio_applied, min_size, max_size, constraint_sources }
}

/// Constraint-space state needed to order a preferred ratio against automatic
/// sizing owned by the containing formatting context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreferredAspectRatioInput {
    /// Resolved preferred border-box size before ratio transfer.
    pub size: Size<Option<f32>>,
    /// Axes whose preferred size was authored as `auto`.
    pub authored_auto: Size<bool>,
    /// Writing mode that defines the logical sizing axes.
    pub writing_mode: WritingMode,
    /// Automatic inline-size behavior selected by the formatting context.
    pub inline_auto_behavior: AutoSizeBehavior,
    /// Automatic block-size behavior selected by the formatting context.
    pub block_auto_behavior: AutoSizeBehavior,
    /// Available border-box space for automatic sizing in each physical axis.
    pub auto_size_available_space: Size<AvailableSpace>,
    /// Used preferred aspect ratio and its sizing box.
    pub aspect_ratio: Option<ResolvedAspectRatio>,
    /// Physical padding-and-border sums for ratio box conversion.
    pub padding_border: Size<f32>,
}

/// Apply a preferred ratio while preserving the constraint space's auto-size
/// ordering in both logical axes.
pub(crate) fn apply_preferred_aspect_ratio(input: PreferredAspectRatioInput) -> Size<Option<f32>> {
    let PreferredAspectRatioInput {
        size,
        authored_auto,
        writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        auto_size_available_space,
        aspect_ratio,
        padding_border,
    } = input;
    let ratio_resolved_size =
        size.maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border);
    let source = writing_mode.to_logical(size);
    let authored_auto = writing_mode.to_logical(authored_auto);
    let available_space = writing_mode.to_logical(auto_size_available_space);
    let mut resolved = writing_mode.to_logical(ratio_resolved_size);
    if inline_auto_behavior == AutoSizeBehavior::StretchExplicit
        && available_space.inline_size.is_definite()
        && authored_auto.inline_size
        && source.inline_size.is_none()
        && resolved.inline_size.is_some()
    {
        // A definite explicit stretch resolves `auto` before preferred-ratio
        // transfer. With an intrinsic constraint there is no stretch size to
        // resolve, so the ratio remains the content-sized fallback.
        resolved.inline_size = None;
    }
    if authored_auto.block_size
        && source.block_size.is_none()
        && resolved.block_size.is_some()
        && block_auto_behavior == AutoSizeBehavior::StretchExplicit
        && available_space.block_size.is_definite()
    {
        resolved.block_size = None;
    }
    writing_mode.to_physical(resolved)
}

/// Inputs for resolving formatting-context-owned automatic sizes after
/// authored preferred sizes and ratio transfers have been resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormattingContextSizeInput {
    /// Resolved preferred border-box size. Explicit-stretch axes may still be
    /// unresolved because their available size belongs to the formatting
    /// context.
    pub size: Size<Option<f32>>,
    /// Whether each physical preferred size was authored as `auto`.
    pub size_is_auto: Size<bool>,
    /// Writing mode that defines inline-before-block sizing order.
    pub writing_mode: WritingMode,
    /// Automatic inline-size behavior selected by the formatting context.
    pub inline_auto_behavior: AutoSizeBehavior,
    /// Automatic block-size behavior selected by the formatting context.
    pub block_auto_behavior: AutoSizeBehavior,
    /// Definite stretch fit supplied by the formatting context in each
    /// physical axis. `None` means that stretch cannot resolve in that axis.
    pub stretch_size: Size<Option<f32>>,
    /// Used preferred aspect ratio and its sizing box.
    pub aspect_ratio: Option<ResolvedAspectRatio>,
    /// Physical padding-and-border sums for ratio box conversion.
    pub padding_border: Size<f32>,
}

/// Automatic-size resolution together with the axes supplied through the
/// preferred aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormattingContextSizeResult {
    /// Resolved physical border-box size.
    pub size: Size<Option<f32>>,
    /// Physical axes whose size was synthesized from the opposite axis by the
    /// preferred aspect ratio.
    pub aspect_ratio_applied: Size<bool>,
}

/// Resolve stretch and preferred-ratio sizing in CSS logical-axis order while
/// retaining ratio provenance for intrinsic-size caches and parent formatting
/// algorithms.
///
/// A definite explicit stretch owns an automatic axis before ratio transfer;
/// without a definite opportunity it falls back to ratio/content sizing. An
/// implicit inline stretch happens only after a ratio has had a chance to use
/// an authored opposite-axis size, and then supplies the inline basis from
/// which an automatic block size may be transferred. Implicit block stretch
/// is the final fallback. Keeping this ordering in one resolver prevents
/// individual formatting contexts from replaying the ratio over an
/// already-stretched axis.
pub fn resolve_formatting_context_size(input: FormattingContextSizeInput) -> FormattingContextSizeResult {
    let FormattingContextSizeInput {
        size,
        size_is_auto,
        writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        stretch_size,
        aspect_ratio,
        padding_border,
    } = input;
    let mut resolved = writing_mode.to_logical(size);
    let authored_auto = writing_mode.to_logical(size_is_auto);
    let stretch = writing_mode.to_logical(stretch_size);

    if authored_auto.inline_size && inline_auto_behavior == AutoSizeBehavior::StretchExplicit {
        if let Some(size) = stretch.inline_size {
            resolved.inline_size = Some(size);
        }
    }
    if authored_auto.block_size && block_auto_behavior == AutoSizeBehavior::StretchExplicit {
        if let Some(size) = stretch.block_size {
            resolved.block_size = Some(size);
        }
    }

    let mut ratio_applied = LogicalSize { inline_size: false, block_size: false };
    let mut apply_ratio = |logical_size: LogicalSize<Option<f32>>| {
        let source = logical_size;
        let physical_size = writing_mode.to_physical(logical_size);
        let ratio_size =
            physical_size.maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border);
        let logical_size = writing_mode.to_logical(ratio_size);
        ratio_applied.inline_size |= source.inline_size.is_none() && logical_size.inline_size.is_some();
        ratio_applied.block_size |= source.block_size.is_none() && logical_size.block_size.is_some();
        logical_size
    };

    resolved = apply_ratio(resolved);
    if authored_auto.inline_size
        && inline_auto_behavior == AutoSizeBehavior::StretchImplicit
        && resolved.inline_size.is_none()
    {
        resolved.inline_size = stretch.inline_size;
    }
    resolved = apply_ratio(resolved);
    if authored_auto.block_size
        && block_auto_behavior == AutoSizeBehavior::StretchImplicit
        && resolved.block_size.is_none()
    {
        resolved.block_size = stretch.block_size;
    }
    // An implicit block-axis fill can be the first definite axis when the
    // inline axis is fit-content. Give that source the same ratio transfer
    // opportunity as an authored or explicitly stretched block size.
    resolved = apply_ratio(resolved);

    FormattingContextSizeResult {
        size: writing_mode.to_physical(resolved),
        aspect_ratio_applied: writing_mode.to_physical(ratio_applied),
    }
}

/// Transfer one pair of axis constraints through the preferred ratio.
fn transferred_constraints(
    constraints: Size<Option<f32>>,
    target_is_auto: Size<bool>,
    aspect_ratio: Option<ResolvedAspectRatio>,
    padding_border: Size<f32>,
) -> Size<Option<f32>> {
    Size {
        width: target_is_auto
            .width
            .then(|| {
                Size { width: None, height: constraints.height }
                    .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
                    .width
            })
            .flatten(),
        height: target_is_auto
            .height
            .then(|| {
                Size { width: constraints.width, height: None }
                    .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
                    .height
            })
            .flatten(),
    }
}

/// Merge explicit and transferred minimums while honoring an explicit maximum.
fn merge_minimum(explicit_min: Option<f32>, transferred_min: Option<f32>, explicit_max: Option<f32>) -> Option<f32> {
    let transferred_min = match (transferred_min, explicit_max) {
        (Some(transferred), Some(maximum)) => Some(transferred.min(maximum)),
        (transferred, _) => transferred,
    };
    match (explicit_min, transferred_min) {
        (Some(explicit), Some(transferred)) => Some(explicit.max(transferred)),
        (Some(explicit), None) => Some(explicit),
        (None, transferred) => transferred,
    }
}

/// Merge explicit and transferred maximums while preserving the used minimum.
fn merge_maximum(explicit_max: Option<f32>, transferred_max: Option<f32>, used_min: Option<f32>) -> Option<f32> {
    let maximum = match (explicit_max, transferred_max) {
        (Some(explicit), Some(transferred)) => Some(explicit.min(transferred)),
        (Some(explicit), None) => Some(explicit),
        (None, transferred) => transferred,
    };
    match (maximum, used_min) {
        (Some(maximum), Some(minimum)) => Some(maximum.max(minimum)),
        (maximum, _) => maximum,
    }
}

/// Combine lower bounds, where an absent constraint has no effect.
fn maximum_constraint(lhs: Option<f32>, rhs: Option<f32>) -> Option<f32> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.max(rhs)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

/// Combine upper bounds, where an absent constraint represents infinity.
fn minimum_constraint(lhs: Option<f32>, rhs: Option<f32>) -> Option<f32> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.min(rhs)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style_helpers::TaffyMaxContent;

    #[test]
    fn border_box_floor_precedes_preferred_ratio_transfer() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: None, height: Some(20.0) },
            min_size: Size::NONE,
            max_size: Size::NONE,
            size_is_auto: Size { width: true, height: false },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size::MAX_CONTENT,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox),
            padding_border: Size { width: 40.0, height: 40.0 },
        });

        assert_eq!(resolved.size, Size { width: Some(80.0), height: Some(40.0) });
    }

    #[test]
    fn structural_border_box_minimum_transfers_through_the_ratio() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size::NONE,
            min_size: Size::NONE,
            max_size: Size { width: None, height: Some(20.0) },
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size::MAX_CONTENT,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox),
            padding_border: Size { width: 40.0, height: 40.0 },
        });

        assert_eq!(resolved.min_size.width, Some(80.0));
        assert_eq!(resolved.max_size, Size { width: Some(80.0), height: Some(40.0) });
    }

    #[test]
    fn explicit_and_implicit_inline_stretch_preserve_ratio_order() {
        let ratio = ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox);
        let auto_width = Size { width: true, height: false };
        let source = Size { width: None, height: Some(50.0) };

        let implicit = apply_preferred_aspect_ratio(PreferredAspectRatioInput {
            size: source,
            authored_auto: auto_width,
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::StretchImplicit,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size {
                width: AvailableSpace::Definite(300.0),
                height: AvailableSpace::MaxContent,
            },
            aspect_ratio: ratio,
            padding_border: Size::ZERO,
        });
        let explicit = apply_preferred_aspect_ratio(PreferredAspectRatioInput {
            size: source,
            authored_auto: auto_width,
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::StretchExplicit,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size {
                width: AvailableSpace::Definite(300.0),
                height: AvailableSpace::MaxContent,
            },
            aspect_ratio: ratio,
            padding_border: Size::ZERO,
        });
        let indefinite_explicit = apply_preferred_aspect_ratio(PreferredAspectRatioInput {
            size: source,
            authored_auto: auto_width,
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::StretchExplicit,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size::MAX_CONTENT,
            aspect_ratio: ratio,
            padding_border: Size::ZERO,
        });

        assert_eq!(implicit, Size { width: Some(100.0), height: Some(50.0) });
        assert_eq!(explicit, Size { width: None, height: Some(50.0) });
        assert_eq!(indefinite_explicit, Size { width: Some(100.0), height: Some(50.0) });
    }

    #[test]
    fn implicit_block_stretch_can_supply_the_ratio_source() {
        let resolved = resolve_formatting_context_size(FormattingContextSizeInput {
            size: Size::NONE,
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::StretchImplicit,
            stretch_size: Size { width: None, height: Some(60.0) },
            aspect_ratio: ResolvedAspectRatio::new(3.0, BoxSizing::BorderBox),
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.size, Size { width: Some(180.0), height: Some(60.0) });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: true, height: false });
    }

    #[test]
    fn explicit_block_stretch_reports_the_ratio_derived_inline_axis() {
        let resolved = resolve_formatting_context_size(FormattingContextSizeInput {
            size: Size::NONE,
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::StretchExplicit,
            stretch_size: Size { width: None, height: Some(60.0) },
            aspect_ratio: ResolvedAspectRatio::new(3.0, BoxSizing::BorderBox),
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.size, Size { width: Some(180.0), height: Some(60.0) });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: true, height: false });
    }

    #[test]
    fn explicit_stretch_precedes_ratio_only_with_a_definite_opportunity() {
        let resolve = |stretch_width| {
            resolve_formatting_context_size(FormattingContextSizeInput {
                size: Size { width: None, height: Some(50.0) },
                size_is_auto: Size { width: true, height: false },
                writing_mode: WritingMode::HorizontalTb,
                inline_auto_behavior: AutoSizeBehavior::StretchExplicit,
                block_auto_behavior: AutoSizeBehavior::FitContent,
                stretch_size: Size { width: stretch_width, height: None },
                aspect_ratio: ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox),
                padding_border: Size::ZERO,
            })
        };

        let definite = resolve(Some(300.0));
        assert_eq!(definite.size, Size { width: Some(300.0), height: Some(50.0) });
        assert_eq!(definite.aspect_ratio_applied, Size { width: false, height: false });

        let indefinite = resolve(None);
        assert_eq!(indefinite.size, Size { width: Some(100.0), height: Some(50.0) });
        assert_eq!(indefinite.aspect_ratio_applied, Size { width: true, height: false });
    }

    #[test]
    fn explicit_constraints_win_over_conflicting_transferred_constraints() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: Some(50.0), height: None },
            min_size: Size { width: Some(100.0), height: None },
            max_size: Size { width: None, height: Some(100.0) },
            size_is_auto: Size { width: false, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size::MAX_CONTENT,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio::new(0.5, BoxSizing::BorderBox),
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.size, Size { width: Some(50.0), height: Some(100.0) });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: false, height: true });
        assert_eq!(resolved.min_size, Size { width: Some(100.0), height: Some(100.0) });
        assert_eq!(resolved.max_size, Size { width: None, height: Some(100.0) });
    }

    #[test]
    fn minimum_wins_when_the_transferred_pair_conflicts() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size::NONE,
            min_size: Size { width: None, height: Some(150.0) },
            max_size: Size { width: None, height: Some(100.0) },
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size::MAX_CONTENT,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox),
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.min_size.width, Some(300.0));
        assert_eq!(resolved.max_size.width, Some(300.0));
    }

    #[test]
    fn ignore_mode_retains_only_explicit_constraints() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size::NONE,
            min_size: Size { width: Some(10.0), height: Some(150.0) },
            max_size: Size { width: Some(20.0), height: Some(100.0) },
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            auto_size_available_space: Size::MAX_CONTENT,
            transferred_sizes_mode: TransferredSizesMode::Ignore,
            aspect_ratio: ResolvedAspectRatio::new(2.0, BoxSizing::BorderBox),
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.min_size, Size { width: Some(10.0), height: Some(150.0) });
        assert_eq!(resolved.max_size, Size { width: Some(20.0), height: Some(150.0) });
    }

    #[test]
    fn authored_maximum_caps_automatic_minimum_before_transfer() {
        let constraints = ResolvedAxisConstraints {
            authored_min: None,
            authored_max: Some(80.0),
            transferred_min: None,
            transferred_max: Some(50.0),
        };

        assert_eq!(constraints.resolve(None, None, Some(100.0)), (Some(80.0), Some(80.0)));
    }

    #[test]
    fn transferred_maximum_does_not_cap_automatic_minimum() {
        let constraints = ResolvedAxisConstraints {
            authored_min: None,
            authored_max: None,
            transferred_min: None,
            transferred_max: Some(50.0),
        };

        assert_eq!(constraints.resolve(None, None, Some(100.0)), (Some(100.0), Some(100.0)));
    }
}
