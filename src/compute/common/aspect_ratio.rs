//! Shared preferred-size and min/max transfer rules for `aspect-ratio`.

use crate::{AbsoluteAxis, AutoSizeBehavior, BoxSizing, MaybeMath, ResolvedAspectRatio, Size, WritingMode};

use super::box_sizing::floor_border_box_size;

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
    /// Authored and transferred constraint sources before the used min/max
    /// pair is collapsed.
    constraint_sources: Size<ResolvedAxisConstraints>,
    /// Content-based automatic minimums retained so a later intrinsic
    /// authored constraint can recompute the source-ordered used pair.
    automatic_minimums: Size<Option<f32>>,
}

/// Resolved min/max sources for one physical axis.
///
/// CSS Sizing applies an automatic content-based minimum before it merges
/// constraints transferred through `aspect-ratio`. Retaining both sources is
/// therefore observable: an authored maximum caps the automatic minimum, but
/// a transferred maximum does not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ResolvedAxisConstraints {
    /// Minimum authored directly in this axis.
    authored_min: Option<f32>,
    /// Maximum authored directly in this axis.
    authored_max: Option<f32>,
    /// Minimum projected from the opposite axis through the preferred ratio.
    transferred_min: Option<f32>,
    /// Maximum projected from the opposite axis through the preferred ratio.
    transferred_max: Option<f32>,
}

impl ResolvedAxisConstraints {
    /// No authored or transferred constraints.
    pub(crate) const NONE: Self =
        Self { authored_min: None, authored_max: None, transferred_min: None, transferred_max: None };

    /// Merge late-resolved authored constraints and an automatic minimum into
    /// the used min/max pair for this axis.
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
    fn with_late_authored_constraints(mut self, min: Option<f32>, max: Option<f32>) -> ResolvedAxisConstraints {
        self.authored_min = maximum_constraint(self.authored_min, min);
        self.authored_max = minimum_constraint(self.authored_max, max);
        self
    }

    /// Resolve only constraints authored in this axis.
    ///
    /// Initial fragment geometry is computed before preferred-ratio transfers
    /// and content callbacks. Keeping that operation source-aware prevents a
    /// ratio-derived opposite axis from feeding back into the same intrinsic
    /// contribution.
    #[inline(always)]
    fn resolve_authored(self) -> (Option<f32>, Option<f32>) {
        Self { transferred_min: None, transferred_max: None, ..self }.resolve(None, None, None)
    }
}

impl ResolvedSizeConstraints {
    /// Return the preferred border-box size after applying the source-ordered
    /// minimum and maximum constraints.
    ///
    /// The unclamped [`Self::size`] remains available to algorithms that need
    /// sizing provenance; layout and contribution boundaries consume this
    /// value when they need the actual preferred used size.
    #[inline(always)]
    pub(crate) fn used_preferred_size(self) -> Size<Option<f32>> {
        self.size.maybe_clamp(self.min_size, self.max_size)
    }

    /// Return initial geometry available without measuring content.
    ///
    /// A direct preferred size is constrained by authored min/max values. When
    /// an otherwise missing axis has equal authored minimum and maximum
    /// constraints, that pair also fixes its initial geometry. Preferred-ratio
    /// and transferred-constraint results are deliberately excluded: using
    /// either as the opposite-axis input would feed the ratio back into its own
    /// intrinsic content callback.
    #[inline(always)]
    pub(crate) fn initial_geometry(self) -> Size<Option<f32>> {
        let (min_width, max_width) = self.constraint_sources.width.resolve_authored();
        let (min_height, max_height) = self.constraint_sources.height.resolve_authored();
        let authored_min = Size { width: min_width, height: min_height };
        let authored_max = Size { width: max_width, height: max_height };
        let direct_size = Size {
            width: if self.aspect_ratio_applied.width { None } else { self.size.width },
            height: if self.aspect_ratio_applied.height { None } else { self.size.height },
        }
        .maybe_clamp(authored_min, authored_max);
        let fixed_by_constraints = authored_min.zip_map(authored_max, |min, max| match (min, max) {
            (Some(min), Some(max)) if max <= min => Some(min),
            _ => None,
        });
        fixed_by_constraints.or(direct_size)
    }

    /// Return the source-preserving constraints for the logical block axis.
    pub(crate) fn block_axis_constraints(self, writing_mode: WritingMode) -> ResolvedAxisConstraints {
        writing_mode.to_logical(self.constraint_sources).block_size
    }

    /// Return the source-preserving constraints for one physical axis.
    pub(crate) const fn axis_constraints(self, axis: AbsoluteAxis) -> ResolvedAxisConstraints {
        match axis {
            AbsoluteAxis::Horizontal => self.constraint_sources.width,
            AbsoluteAxis::Vertical => self.constraint_sources.height,
        }
    }

    /// Merge values of authored intrinsic min/max keywords that became known
    /// after the initial length/percentage resolution.
    pub(crate) fn apply_late_authored_constraints(&mut self, min_size: Size<Option<f32>>, max_size: Size<Option<f32>>) {
        self.constraint_sources.width =
            self.constraint_sources.width.with_late_authored_constraints(min_size.width, max_size.width);
        self.constraint_sources.height =
            self.constraint_sources.height.with_late_authored_constraints(min_size.height, max_size.height);
        let (min_width, max_width) = self.constraint_sources.width.resolve(None, None, self.automatic_minimums.width);
        let (min_height, max_height) =
            self.constraint_sources.height.resolve(None, None, self.automatic_minimums.height);
        self.min_size = Size { width: min_width, height: min_height };
        self.max_size = Size { width: max_width, height: max_height };
    }

    /// Merge an intrinsic preferred/minimum/maximum triplet after its content
    /// callback has completed.
    ///
    /// Direct constraint resolution may provisionally synthesize the queried
    /// preferred axis through `aspect-ratio` while an authored intrinsic
    /// keyword is still unresolved. The authored keyword owns that axis once
    /// measured, so it replaces only that provisional ratio result. A direct
    /// authored length, percentage, or parent-fixed size retains precedence.
    pub(crate) fn apply_late_intrinsic_axis(
        &mut self,
        axis: AbsoluteAxis,
        preferred: Option<f32>,
        preferred_aspect_ratio_applied: bool,
        minimum: Option<f32>,
        maximum: Option<f32>,
    ) {
        if let Some(preferred) = preferred {
            match axis {
                AbsoluteAxis::Horizontal if self.size.width.is_none() || self.aspect_ratio_applied.width => {
                    self.size.width = Some(preferred);
                    self.aspect_ratio_applied.width = preferred_aspect_ratio_applied;
                }
                AbsoluteAxis::Vertical if self.size.height.is_none() || self.aspect_ratio_applied.height => {
                    self.size.height = Some(preferred);
                    self.aspect_ratio_applied.height = preferred_aspect_ratio_applied;
                }
                AbsoluteAxis::Horizontal | AbsoluteAxis::Vertical => {}
            }
        }

        let (minimum, maximum) = match axis {
            AbsoluteAxis::Horizontal => (Size { width: minimum, height: None }, Size { width: maximum, height: None }),
            AbsoluteAxis::Vertical => (Size { width: None, height: minimum }, Size { width: None, height: maximum }),
        };
        self.apply_late_authored_constraints(minimum, maximum);
    }

    /// Apply CSS Sizing's aspect-ratio automatic minimum in one physical
    /// axis while preserving the authored/transferred ordering.
    pub(crate) fn apply_automatic_minimum(&mut self, axis: AbsoluteAxis, automatic_minimum: Option<f32>) {
        match axis {
            AbsoluteAxis::Horizontal => self.automatic_minimums.width = automatic_minimum,
            AbsoluteAxis::Vertical => self.automatic_minimums.height = automatic_minimum,
        }
        let (minimum, maximum) = self.axis_constraints(axis).resolve(None, None, automatic_minimum);
        match axis {
            AbsoluteAxis::Horizontal => {
                self.min_size.width = minimum;
                self.max_size.width = maximum;
            }
            AbsoluteAxis::Vertical => {
                self.min_size.height = minimum;
                self.max_size.height = maximum;
            }
        }
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
/// Keeping the constraint-space state named at call sites makes the sizing
/// order explicit and prevents formatting-context-specific flags from growing
/// a positional parameter list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SizeConstraintInput {
    /// Resolved preferred border-box sizes before ratio transfer.
    pub size: Size<Option<f32>>,
    /// Axes whose preferred size was still indefinite before an intrinsic
    /// contribution was materialized.
    ///
    /// This is intentionally distinct from [`Self::size_is_auto`]. An
    /// intrinsic keyword such as `max-content` can already have a numeric
    /// value in `size`, while opposite-axis min/max constraints must still be
    /// transferred through the preferred ratio to bound that contribution.
    pub preferred_size_is_indefinite: Size<bool>,
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
    /// Whether opposite-axis min/max constraints transfer through the ratio.
    pub transferred_sizes_mode: TransferredSizesMode,
    /// Used preferred aspect ratio and its sizing box.
    pub aspect_ratio: ResolvedAspectRatio,
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
        preferred_size_is_indefinite,
        mut min_size,
        mut max_size,
        size_is_auto,
        writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        transferred_sizes_mode,
        aspect_ratio,
        padding_border,
    } = input;

    // This constraint space stores border-box dimensions. Establish their
    // minimum physical box before ratio transfer so an authored preferred or
    // limiting size smaller than padding+border cannot become a negative
    // ratio basis. Blink performs this normalization while resolving each CSS
    // length, before its aspect-ratio sizing helpers consume the value.
    size = floor_border_box_size(size, padding_border);
    min_size = floor_border_box_size(min_size, padding_border);
    max_size = floor_border_box_size(max_size, padding_border);
    let (transferred_min, transferred_max) = match transferred_sizes_mode {
        TransferredSizesMode::Normal => (
            transferred_constraints(min_size, preferred_size_is_indefinite, aspect_ratio, padding_border),
            transferred_constraints(max_size, preferred_size_is_indefinite, aspect_ratio, padding_border),
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

    let resolved_size = apply_preferred_aspect_ratio(
        size,
        size_is_auto,
        writing_mode,
        inline_auto_behavior,
        block_auto_behavior,
        aspect_ratio,
        padding_border,
    );
    let aspect_ratio_applied = Size {
        width: size.width.is_none() && resolved_size.width.is_some(),
        height: size.height.is_none() && resolved_size.height.is_some(),
    };

    ResolvedSizeConstraints {
        size: resolved_size,
        aspect_ratio_applied,
        min_size,
        max_size,
        constraint_sources,
        automatic_minimums: Size::NONE,
    }
}

/// Apply a preferred ratio while preserving the constraint space's auto-size
/// ordering in both logical axes.
pub(crate) fn apply_preferred_aspect_ratio(
    size: Size<Option<f32>>,
    size_is_auto: Size<bool>,
    writing_mode: WritingMode,
    inline_auto_behavior: AutoSizeBehavior,
    block_auto_behavior: AutoSizeBehavior,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<Option<f32>> {
    let ratio_resolved_size =
        size.maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border);
    let source = writing_mode.to_logical(size);
    let authored_auto = writing_mode.to_logical(size_is_auto);
    let mut resolved = writing_mode.to_logical(ratio_resolved_size);
    if inline_auto_behavior == AutoSizeBehavior::StretchExplicit
        && authored_auto.inline_size
        && source.inline_size.is_none()
        && resolved.inline_size.is_some()
    {
        // Explicit stretch resolves `auto` before preferred-ratio transfer.
        // Keep the opposite-axis result and transferred min/max constraints,
        // but leave the preferred inline size for the formatting context.
        resolved.inline_size = None;
    }
    if block_auto_behavior == AutoSizeBehavior::StretchExplicit
        && authored_auto.block_size
        && source.block_size.is_none()
        && resolved.block_size.is_some()
    {
        resolved.block_size = None;
    }
    writing_mode.to_physical(resolved)
}

/// Transfer one pair of axis constraints through the preferred ratio.
fn transferred_constraints(
    constraints: Size<Option<f32>>,
    target_was_indefinite: Size<bool>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<Option<f32>> {
    Size {
        width: target_was_indefinite
            .width
            .then(|| {
                Size { width: None, height: constraints.height }
                    .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::BorderBox, padding_border)
                    .width
            })
            .flatten(),
        height: target_was_indefinite
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

    #[test]
    fn initial_geometry_excludes_preferred_ratio_feedback() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: None, height: Some(25.0) },
            preferred_size_is_indefinite: Size { width: true, height: false },
            min_size: Size { width: None, height: Some(25.0) },
            max_size: Size { width: None, height: Some(25.0) },
            size_is_auto: Size { width: false, height: false },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(4.0), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.size, Size { width: Some(100.0), height: Some(25.0) });
        assert_eq!(resolved.initial_geometry(), Size { width: None, height: Some(25.0) });
    }

    #[test]
    fn explicit_and_implicit_auto_stretch_preserve_ratio_order_per_logical_axis() {
        let ratio = ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::BorderBox };
        let auto_width = Size { width: true, height: false };

        let implicit_inline = apply_preferred_aspect_ratio(
            Size { width: None, height: Some(50.0) },
            auto_width,
            WritingMode::HorizontalTb,
            AutoSizeBehavior::StretchImplicit,
            AutoSizeBehavior::FitContent,
            ratio,
            Size::ZERO,
        );
        let explicit_inline = apply_preferred_aspect_ratio(
            Size { width: None, height: Some(50.0) },
            auto_width,
            WritingMode::HorizontalTb,
            AutoSizeBehavior::StretchExplicit,
            AutoSizeBehavior::FitContent,
            ratio,
            Size::ZERO,
        );

        assert_eq!(implicit_inline, Size { width: Some(100.0), height: Some(50.0) });
        assert_eq!(explicit_inline, Size { width: None, height: Some(50.0) });

        let auto_height = Size { width: false, height: true };
        let implicit_block = apply_preferred_aspect_ratio(
            Size { width: Some(100.0), height: None },
            auto_height,
            WritingMode::HorizontalTb,
            AutoSizeBehavior::FitContent,
            AutoSizeBehavior::StretchImplicit,
            ratio,
            Size::ZERO,
        );
        let explicit_block = apply_preferred_aspect_ratio(
            Size { width: Some(100.0), height: None },
            auto_height,
            WritingMode::HorizontalTb,
            AutoSizeBehavior::FitContent,
            AutoSizeBehavior::StretchExplicit,
            ratio,
            Size::ZERO,
        );

        assert_eq!(implicit_block, Size { width: Some(100.0), height: Some(50.0) });
        assert_eq!(explicit_block, Size { width: Some(100.0), height: None });
    }

    #[test]
    fn ratio_transfer_uses_the_minimum_border_box_as_its_source() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: None, height: Some(20.0) },
            preferred_size_is_indefinite: Size { width: true, height: false },
            min_size: Size::NONE,
            max_size: Size::NONE,
            size_is_auto: Size { width: true, height: false },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::BorderBox },
            padding_border: Size { width: 40.0, height: 40.0 },
        });

        assert_eq!(resolved.size, Size { width: Some(80.0), height: Some(40.0) });
    }

    #[test]
    fn explicit_constraints_win_over_conflicting_transferred_constraints() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: Some(50.0), height: None },
            preferred_size_is_indefinite: Size { width: false, height: true },
            min_size: Size { width: Some(100.0), height: None },
            max_size: Size { width: None, height: Some(100.0) },
            size_is_auto: Size { width: false, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(0.5), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.size, Size { width: Some(50.0), height: Some(100.0) });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: false, height: true });
        assert_eq!(resolved.min_size, Size { width: Some(100.0), height: Some(100.0) });
        assert_eq!(resolved.max_size, Size { width: None, height: Some(100.0) });
        assert_eq!(resolved.used_preferred_size(), Size { width: Some(100.0), height: Some(100.0) });
    }

    #[test]
    fn minimum_wins_when_the_transferred_pair_conflicts() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size::NONE,
            preferred_size_is_indefinite: Size { width: true, height: true },
            min_size: Size { width: None, height: Some(150.0) },
            max_size: Size { width: None, height: Some(100.0) },
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.min_size.width, Some(300.0));
        assert_eq!(resolved.max_size.width, Some(300.0));
    }

    #[test]
    fn transferred_constraints_bound_a_materialized_intrinsic_preferred_size() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            // The max-content contribution has already been measured, but it
            // was indefinite when the initial constraint space was built.
            size: Size { width: Some(200.0), height: None },
            preferred_size_is_indefinite: Size { width: true, height: true },
            min_size: Size::NONE,
            max_size: Size { width: None, height: Some(100.0) },
            size_is_auto: Size { width: false, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(1.0), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.max_size, Size { width: Some(100.0), height: Some(100.0) });
        assert_eq!(resolved.used_preferred_size(), Size { width: Some(100.0), height: Some(100.0) });
    }

    #[test]
    fn authored_intrinsic_preferred_replaces_a_provisional_ratio_axis() {
        let mut resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: None, height: Some(100.0) },
            preferred_size_is_indefinite: Size { width: true, height: false },
            min_size: Size::NONE,
            max_size: Size::NONE,
            size_is_auto: Size { width: false, height: false },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(1.0), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        assert_eq!(resolved.size, Size { width: Some(100.0), height: Some(100.0) });
        resolved.apply_late_intrinsic_axis(AbsoluteAxis::Horizontal, Some(50.0), false, None, None);
        assert_eq!(resolved.size, Size { width: Some(50.0), height: Some(100.0) });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: false, height: false });
    }

    #[test]
    fn ratio_backed_intrinsic_preferred_retains_ratio_provenance() {
        let mut resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: None, height: Some(100.0) },
            preferred_size_is_indefinite: Size { width: true, height: false },
            min_size: Size::NONE,
            max_size: Size::NONE,
            size_is_auto: Size { width: false, height: false },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(1.0), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        resolved.apply_late_intrinsic_axis(AbsoluteAxis::Horizontal, Some(100.0), true, None, None);
        assert_eq!(resolved.size, Size { width: Some(100.0), height: Some(100.0) });
        assert_eq!(resolved.aspect_ratio_applied, Size { width: true, height: false });
    }

    #[test]
    fn ignore_mode_retains_only_explicit_constraints() {
        let resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size::NONE,
            preferred_size_is_indefinite: Size { width: true, height: true },
            min_size: Size { width: Some(10.0), height: Some(150.0) },
            max_size: Size { width: Some(20.0), height: Some(100.0) },
            size_is_auto: Size { width: true, height: true },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Ignore,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::BorderBox },
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

    #[test]
    fn late_authored_constraints_retain_the_automatic_minimum() {
        let mut resolved = resolve_size_constraints(SizeConstraintInput {
            size: Size { width: None, height: Some(200.0) },
            preferred_size_is_indefinite: Size { width: true, height: false },
            min_size: Size::NONE,
            max_size: Size { width: None, height: Some(100.0) },
            size_is_auto: Size { width: true, height: false },
            writing_mode: WritingMode::HorizontalTb,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            transferred_sizes_mode: TransferredSizesMode::Normal,
            aspect_ratio: ResolvedAspectRatio { ratio: Some(0.5), box_sizing: BoxSizing::BorderBox },
            padding_border: Size::ZERO,
        });

        resolved.apply_automatic_minimum(AbsoluteAxis::Horizontal, Some(100.0));
        resolved.apply_late_authored_constraints(Size::NONE, Size { width: Some(120.0), height: None });

        assert_eq!(resolved.min_size.width, Some(100.0));
        assert_eq!(resolved.max_size.width, Some(100.0));
    }
}
