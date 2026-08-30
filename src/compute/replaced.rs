//! CSS sizing for replaced content such as images, canvas and form controls.
//!
//! Replaced content differs from an ordinary measured leaf in two important
//! ways: its natural dimensions participate in preferred/min/max sizing, and
//! constraints transferred through its preferred aspect ratio must be applied
//! as part of the same sizing operation. Keeping this algorithm in Taffy means
//! embedding engines provide content metrics rather than reimplementing the
//! CSS box model in a measurement callback.

use crate::geometry::{AbsoluteAxis, Size};
use crate::style::{AvailableSpace, BoxSizing, CoreStyle, ResolvedAspectRatio, SizeContainment};
use crate::tree::{LayoutInput, LayoutOutput, RequestedAxis, RunMode, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::WritingMode;

use super::common::box_sizing::{authored_size_to_content_box, border_box_to_content_box};
use super::common::intrinsic_size::replaced_min_content_contribution_is_cyclic;

/// Natural content metrics supplied by the embedding engine.
///
/// Natural dimensions remain optional even after the resource has a concrete
/// object size. This distinction is observable when an intrinsic sizing
/// keyword combines one natural axis with a preferred aspect ratio. The
/// default object size supplies missing axes only when no ratio participates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplacedNaturalSizing {
    /// Actual natural content-box dimensions. A missing dimension must remain
    /// `None`; it is not the corresponding default-object-size dimension.
    pub dimensions: Size<Option<f32>>,
    /// Category/resource-specific default content-box size used when natural
    /// dimensions and a preferred ratio do not determine the box.
    pub default_object_size: Size<f32>,
}

impl ReplacedNaturalSizing {
    /// Construct natural sizing information from independently optional axes.
    pub const fn new(dimensions: Size<Option<f32>>, default_object_size: Size<f32>) -> Self {
        Self { dimensions, default_object_size }
    }

    /// Construct natural sizing information for content with two fixed axes.
    pub const fn fixed(size: Size<f32>) -> Self {
        Self { dimensions: Size::new(size.width, size.height), default_object_size: size }
    }
}

/// Which layout-object model supplies a replaced sizing operation's
/// min-content contribution.
///
/// Blink's actual `LayoutReplaced` objects (images, SVG, canvas, frames, ...)
/// drop a cyclic preferred/max inline size to their used minimum. HTML form
/// controls such as text and range inputs are only treated as replaced for
/// the compressible-percentage rule: their percentage resolves against zero,
/// while an absolute `calc()` term remains in the contribution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReplacedMinContentKind {
    /// Natural/external replaced content represented by a replaced layout
    /// object.
    #[default]
    NaturalObject,
    /// A content-bearing control treated as replaced by CSS Sizing's
    /// compressible-percentage rule.
    CompressibleControl,
}

/// Node-level content metrics and used values for replaced sizing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplacedSizingContext {
    /// Used writing mode for the replaced box.
    pub writing_mode: WritingMode,
    /// Used preferred aspect ratio and the box whose dimensions it constrains.
    pub aspect_ratio: ResolvedAspectRatio,
    /// Used size-containment state for the generated box.
    pub size_containment: SizeContainment,
    /// Natural dimensions and the embedding category's default object size.
    pub natural_sizing: ReplacedNaturalSizing,
    /// Layout-object model used for cyclic min-content contributions.
    pub min_content_kind: ReplacedMinContentKind,
}

impl ReplacedSizingContext {
    /// Construct the used values for a replaced sizing operation.
    pub const fn new(
        writing_mode: WritingMode,
        aspect_ratio: ResolvedAspectRatio,
        size_containment: SizeContainment,
        natural_sizing: ReplacedNaturalSizing,
    ) -> Self {
        Self {
            writing_mode,
            aspect_ratio,
            size_containment,
            natural_sizing,
            min_content_kind: ReplacedMinContentKind::NaturalObject,
        }
    }

    /// Select the layout-object model used for min-content contributions.
    #[inline(always)]
    pub const fn with_min_content_kind(mut self, min_content_kind: ReplacedMinContentKind) -> Self {
        self.min_content_kind = min_content_kind;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Whether one natural content-box dimension violates its used constraints.
enum ConstraintViolation {
    /// The dimension lies inside its used min/max range.
    None,
    /// The dimension is smaller than its used minimum.
    Minimum,
    /// The dimension is larger than its used maximum.
    Maximum,
}

/// Compute the border-box layout size of replaced content.
///
/// The embedding engine supplies natural content metrics through
/// ReplacedSizingContext. Taffy owns CSS preferred/min/max resolution,
/// intrinsic percentage behavior, aspect-ratio transfer, box-sizing
/// conversion and size containment. This is intentionally separate from
/// compute_leaf_layout, whose measurement callback returns content size and
/// must not duplicate style constraint resolution.
///
/// As with every child layout algorithm, definite available space is the
/// margin-excluded border-box space offered by the parent. It controls
/// intrinsic contribution semantics but is not an implicit maximum size.
pub fn compute_replaced_layout(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    context: ReplacedSizingContext,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
) -> LayoutOutput {
    let constraint_space = inputs.constraint_space(context.writing_mode);
    let percentage_basis = constraint_space.margin_padding_percentage_basis();
    let LayoutInput {
        run_mode, known_dimensions, parent_size, available_space, sizing_mode, axis: requested_axis, ..
    } = inputs;

    let padding = style.padding().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let border = style.border().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let padding_border = padding + border;
    let padding_border_sum = padding_border.sum_axes();

    let contained_content_size = Size {
        width: context
            .size_containment
            .axes
            .width
            .then_some(context.size_containment.intrinsic_content_size.width.unwrap_or(0.0)),
        height: context
            .size_containment
            .axes
            .height
            .then_some(context.size_containment.intrinsic_content_size.height.unwrap_or(0.0)),
    };
    let natural_dimensions = Size {
        width: contained_content_size.width.or(context.natural_sizing.dimensions.width),
        height: contained_content_size.height.or(context.natural_sizing.dimensions.height),
    };
    let default_object_size = Size {
        width: contained_content_size.width.unwrap_or(context.natural_sizing.default_object_size.width),
        height: contained_content_size.height.unwrap_or(context.natural_sizing.default_object_size.height),
    };
    let (natural_size, natural_ratio_derived_axes) = normalized_natural_size(
        natural_dimensions,
        default_object_size,
        context.aspect_ratio,
        context.writing_mode,
        padding_border_sum,
    );
    let content_known = border_box_to_content_box(known_dimensions, padding_border_sum);

    // During a min-content contribution, percentage preferred and maximum
    // sizes are cyclic and resolve against zero. Minimum sizes retain the
    // normal percentage basis per CSS Sizing 3 replaced contributions.
    let preferred_percentage_basis = Size {
        width: if available_space.width == AvailableSpace::MinContent { Some(0.0) } else { parent_size.width },
        height: if available_space.height == AvailableSpace::MinContent { Some(0.0) } else { parent_size.height },
    };

    let raw_size = style.size();
    let raw_min_size = style.min_size();
    let raw_max_size = style.max_size();
    let cyclic_min_content_axis =
        replaced_min_content_contribution_is_cyclic(inputs, context.writing_mode, raw_size, raw_max_size)
            .then(|| context.writing_mode.inline_axis());
    let logical_raw_size = context.writing_mode.to_logical(raw_size);
    let logical_default_object_size = context.writing_mode.to_logical(default_object_size);
    let ratio_only_max_content_inline_size = logical_raw_size
        .inline_size
        .may_have_percentage_dependence()
        .then_some(logical_default_object_size.inline_size);
    let box_sizing = style.box_sizing();
    let mut preferred_size = authored_size_to_content_box(
        raw_size.maybe_resolve(preferred_percentage_basis, &resolve_calc_value),
        box_sizing,
        padding_border_sum,
    );
    let mut min_size = authored_size_to_content_box(
        raw_min_size.maybe_resolve(parent_size, &resolve_calc_value),
        box_sizing,
        padding_border_sum,
    );
    let mut max_size = authored_size_to_content_box(
        raw_max_size.maybe_resolve(preferred_percentage_basis, &resolve_calc_value),
        box_sizing,
        padding_border_sum,
    );

    // Blink's replaced min-content contribution is the used minimum when a
    // preferred or maximum inline size contains a cyclic percentage. This is
    // deliberately distinct from Flexbox's specified-size suggestion, which
    // resolves the percentage against zero while retaining a calc() length.
    if context.min_content_kind == ReplacedMinContentKind::NaturalObject
        && replaced_min_content_contribution_is_cyclic(inputs, context.writing_mode, raw_size, raw_max_size)
    {
        let mut logical_preferred_size = context.writing_mode.to_logical(preferred_size);
        logical_preferred_size.inline_size = Some(0.0);
        preferred_size = context.writing_mode.to_physical(logical_preferred_size);
    }

    for (raw, resolved, contained) in [
        (raw_size, &mut preferred_size, contained_content_size),
        (raw_min_size, &mut min_size, contained_content_size),
        (raw_max_size, &mut max_size, contained_content_size),
    ] {
        if raw.width.is_intrinsic() {
            resolved.width = resolved.width.or(contained.width);
        }
        if raw.height.is_intrinsic() {
            resolved.height = resolved.height.or(contained.height);
        }
    }

    // Resolve intrinsic min/max constraints by sizing the replaced content
    // while ignoring preferred lengths in the queried axis. The opposite
    // fixed/preferred axis can determine the result through the used ratio;
    // otherwise the normalized natural size supplies the intrinsic value.
    // This is the same boundary as Blink's ReplacedSizeMode axis modes.
    let intrinsic_basis = content_known.or(preferred_size);
    let intrinsic_fallback = natural_size.or_else(|| {
        context.aspect_ratio.ratio().map(|_| {
            ratio_only_stretch_size(
                available_space,
                context.writing_mode,
                context.aspect_ratio,
                padding_border_sum,
                ratio_only_max_content_inline_size,
            )
        })
    });
    let intrinsic_size = Size {
        width: intrinsic_constraint_size(
            AbsoluteAxis::Horizontal,
            intrinsic_basis,
            intrinsic_fallback,
            context.aspect_ratio,
            padding_border_sum,
        ),
        height: intrinsic_constraint_size(
            AbsoluteAxis::Vertical,
            intrinsic_basis,
            intrinsic_fallback,
            context.aspect_ratio,
            padding_border_sum,
        ),
    };
    for (raw, resolved) in [(raw_min_size, &mut min_size), (raw_max_size, &mut max_size)] {
        if raw.width.is_intrinsic() {
            resolved.width = intrinsic_size.width;
        }
        if raw.height.is_intrinsic() {
            resolved.height = intrinsic_size.height;
        }
    }

    // Intrinsic min/max keywords resolve after the directly resolvable
    // constraints because their value may transfer from the opposite
    // preferred axis. Establish the CSS min-over-max precedence only after
    // those late constraints are present.
    max_size = max_size.maybe_max(min_size);

    // A content-size probe ignores preferred and minimum constraints in the
    // requested axis. Opposite-axis constraints remain available for ratio
    // transfer.
    if sizing_mode == SizingMode::ContentSize {
        match requested_axis {
            RequestedAxis::Horizontal => {
                if cyclic_min_content_axis != Some(AbsoluteAxis::Horizontal) {
                    preferred_size.width = None;
                }
                min_size.width = None;
            }
            RequestedAxis::Vertical => {
                if cyclic_min_content_axis != Some(AbsoluteAxis::Vertical) {
                    preferred_size.height = None;
                }
                min_size.height = None;
            }
            RequestedAxis::Both => {}
        }
    }

    if known_dimensions.width.is_some() || known_dimensions.height.is_some() {
        let style_max_size = authored_size_to_content_box(
            raw_max_size.maybe_resolve(preferred_percentage_basis, &resolve_calc_value),
            box_sizing,
            padding_border_sum,
        )
        .maybe_max(min_size);
        let known_or_cyclic_preferred =
            if cyclic_min_content_axis.is_some() { content_known.or(preferred_size) } else { content_known };
        let transfer_basis = known_or_cyclic_preferred.maybe_clamp(min_size, style_max_size);
        let ratio_derived_axes = if transfer_basis.width.is_none() && transfer_basis.height.is_none() {
            natural_ratio_derived_axes
        } else {
            ratio_derived_axes_from_basis(transfer_basis, context.aspect_ratio)
        };
        let transferred = complete_replaced_size(
            apply_aspect_ratio_to_content_size(transfer_basis, context.aspect_ratio, padding_border_sum),
            natural_size,
        )
        .expect("known replaced dimensions or natural sizing determine both axes");
        let size = content_known.unwrap_or(transferred.maybe_clamp(min_size, style_max_size));
        let ratio_derived_axes = Size {
            width: content_known.width.is_none() && ratio_derived_axes.width,
            height: content_known.height.is_none() && ratio_derived_axes.height,
        };
        return replaced_output(
            size.map(|value| value.max(0.0)) + padding_border_sum,
            run_mode,
            requested_axis,
            ratio_derived_axes,
        );
    }

    let direct_preferred_axes = Size { width: preferred_size.width.is_some(), height: preferred_size.height.is_some() };
    let (unclamped, ratio_derived_axes) = if direct_preferred_axes.width || direct_preferred_axes.height {
        (
            complete_replaced_size(
                apply_aspect_ratio_to_content_size(preferred_size, context.aspect_ratio, padding_border_sum),
                natural_size,
            )
            .expect("preferred replaced dimensions or natural sizing determine both axes"),
            ratio_derived_axes_from_basis(preferred_size, context.aspect_ratio),
        )
    } else {
        match natural_size {
            Some(natural_size) => (natural_size, natural_ratio_derived_axes),
            None => {
                let ratio_basis = ratio_only_stretch_basis(
                    available_space,
                    context.writing_mode,
                    padding_border_sum,
                    ratio_only_max_content_inline_size,
                );
                (
                    complete_replaced_size(
                        apply_aspect_ratio_to_content_size(ratio_basis, context.aspect_ratio, padding_border_sum),
                        None,
                    )
                    .expect("a valid preferred ratio resolves the stretch-fit axis"),
                    ratio_derived_axes_from_basis(ratio_basis, context.aspect_ratio),
                )
            }
        }
    };
    let size = unclamped.map(|value| value.max(0.0));

    if !context.aspect_ratio.has_ratio() {
        return replaced_output(
            size.maybe_clamp(min_size, max_size) + padding_border_sum,
            run_mode,
            requested_axis,
            ratio_derived_axes,
        );
    }

    let size = constrain_replaced_size(
        size,
        direct_preferred_axes,
        min_size,
        max_size,
        context.aspect_ratio,
        padding_border_sum,
    );
    replaced_output(size + padding_border_sum, run_mode, requested_axis, ratio_derived_axes)
}

/// Apply used min/max constraints without losing which preferred axes were
/// resolved directly.
///
/// Blink's replaced sizing keeps independently resolved inline and block
/// sizes as two optionals. When one is present, it owns that axis: constraints
/// first clamp the direct value, the ratio derives only the missing axis, and
/// constraints on that derived axis do not feed back. A natural-size
/// candidate has no direct axis, so both dimensions remain coupled by the
/// preferred ratio during constraint reconciliation.
fn constrain_replaced_size(
    size: Size<f32>,
    direct_axes: Size<bool>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<f32> {
    match (direct_axes.width, direct_axes.height) {
        (true, true) => size.maybe_clamp(min_size, max_size),
        (true, false) => {
            let width = size.width.maybe_clamp(min_size.width, max_size.width);
            let height = content_height_from_width(width, aspect_ratio, padding_border)
                .expect("a resolved ratio transfers a direct width to height")
                .maybe_clamp(min_size.height, max_size.height);
            Size { width, height }
        }
        (false, true) => {
            let height = size.height.maybe_clamp(min_size.height, max_size.height);
            let width = content_width_from_height(height, aspect_ratio, padding_border)
                .expect("a resolved ratio transfers a direct height to width")
                .maybe_clamp(min_size.width, max_size.width);
            Size { width, height }
        }
        (false, false) => constrain_natural_replaced_size(size, min_size, max_size, aspect_ratio, padding_border),
    }
}

/// Reconcile constraints for a natural replaced size while preserving its
/// preferred ratio where the min/max pair permits it.
fn constrain_natural_replaced_size(
    size: Size<f32>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<f32> {
    let width_violation = constraint_violation(size.width, min_size.width, max_size.width);
    let height_violation = constraint_violation(size.height, min_size.height, max_size.height);
    match (width_violation, height_violation) {
        (ConstraintViolation::None, ConstraintViolation::None) => size,
        (ConstraintViolation::Maximum, ConstraintViolation::None) => {
            let width = max_size.width.expect("maximum width violation has a bound");
            Size {
                width,
                height: content_height_from_width(width, aspect_ratio, padding_border)
                    .expect("a resolved ratio transfers width to height")
                    .maybe_max(min_size.height),
            }
        }
        (ConstraintViolation::Minimum, ConstraintViolation::None) => {
            let width = min_size.width.expect("minimum width violation has a bound");
            Size {
                width,
                height: content_height_from_width(width, aspect_ratio, padding_border)
                    .expect("a resolved ratio transfers width to height")
                    .maybe_min(max_size.height),
            }
        }
        (ConstraintViolation::None, ConstraintViolation::Maximum) => {
            let height = max_size.height.expect("maximum height violation has a bound");
            Size {
                width: content_width_from_height(height, aspect_ratio, padding_border)
                    .expect("a resolved ratio transfers height to width")
                    .maybe_max(min_size.width),
                height,
            }
        }
        (ConstraintViolation::None, ConstraintViolation::Minimum) => {
            let height = min_size.height.expect("minimum height violation has a bound");
            Size {
                width: content_width_from_height(height, aspect_ratio, padding_border)
                    .expect("a resolved ratio transfers height to width")
                    .maybe_min(max_size.width),
                height,
            }
        }
        (ConstraintViolation::Maximum, ConstraintViolation::Maximum) => {
            let width = max_size.width.expect("maximum width violation has a bound");
            let height = max_size.height.expect("maximum height violation has a bound");
            if ratio_basis_scale(width, size.width, padding_border.width, aspect_ratio.sizing_box())
                <= ratio_basis_scale(height, size.height, padding_border.height, aspect_ratio.sizing_box())
            {
                Size {
                    width,
                    height: content_height_from_width(width, aspect_ratio, padding_border)
                        .expect("a resolved ratio transfers width to height")
                        .maybe_max(min_size.height),
                }
            } else {
                Size {
                    width: content_width_from_height(height, aspect_ratio, padding_border)
                        .expect("a resolved ratio transfers height to width")
                        .maybe_max(min_size.width),
                    height,
                }
            }
        }
        (ConstraintViolation::Minimum, ConstraintViolation::Minimum) => {
            let width = min_size.width.expect("minimum width violation has a bound");
            let height = min_size.height.expect("minimum height violation has a bound");
            if ratio_basis_scale(width, size.width, padding_border.width, aspect_ratio.sizing_box())
                <= ratio_basis_scale(height, size.height, padding_border.height, aspect_ratio.sizing_box())
            {
                Size {
                    width: content_width_from_height(height, aspect_ratio, padding_border)
                        .expect("a resolved ratio transfers height to width")
                        .maybe_min(max_size.width),
                    height,
                }
            } else {
                Size {
                    width,
                    height: content_height_from_width(width, aspect_ratio, padding_border)
                        .expect("a resolved ratio transfers width to height")
                        .maybe_min(max_size.height),
                }
            }
        }
        (ConstraintViolation::Minimum, ConstraintViolation::Maximum) => Size {
            width: min_size.width.expect("minimum width violation has a bound"),
            height: max_size.height.expect("maximum height violation has a bound"),
        },
        (ConstraintViolation::Maximum, ConstraintViolation::Minimum) => Size {
            width: max_size.width.expect("maximum width violation has a bound"),
            height: min_size.height.expect("minimum height violation has a bound"),
        },
    }
}

/// Normalize independently optional natural axes in the node's logical
/// sizing order. A preferred ratio always reconciles the natural block axis
/// from the natural inline axis when both exist, matching replaced layout's
/// inline-first sizing algorithm. The default object size is used only when
/// there is no ratio to perform that transfer.
fn normalized_natural_size(
    dimensions: Size<Option<f32>>,
    default_object_size: Size<f32>,
    aspect_ratio: ResolvedAspectRatio,
    writing_mode: WritingMode,
    padding_border: Size<f32>,
) -> (Option<Size<f32>>, Size<bool>) {
    if !aspect_ratio.has_ratio() {
        return (Some(dimensions.unwrap_or(default_object_size)), Size { width: false, height: false });
    }

    let ratio_basis = if writing_mode.is_horizontal() {
        if let Some(width) = dimensions.width {
            Size { width: Some(width), height: None }
        } else if let Some(height) = dimensions.height {
            Size { width: None, height: Some(height) }
        } else {
            return (None, Size { width: false, height: false });
        }
    } else if let Some(height) = dimensions.height {
        Size { width: None, height: Some(height) }
    } else if let Some(width) = dimensions.width {
        Size { width: Some(width), height: None }
    } else {
        return (None, Size { width: false, height: false });
    };

    (
        complete_replaced_size(apply_aspect_ratio_to_content_size(ratio_basis, aspect_ratio, padding_border), None),
        ratio_derived_axes_from_basis(ratio_basis, aspect_ratio),
    )
}

/// Resolve the stretch-fit fallback for replaced content that has only a
/// preferred ratio. Replaced layout stretches in its logical inline axis and
/// derives the orthogonal axis through that ratio. An indefinite max-content
/// query uses the category's default logical inline size when the preferred
/// inline size contains a percentage; the corresponding min-content query is
/// cyclic and therefore contributes zero.
fn ratio_only_stretch_size(
    available_space: Size<AvailableSpace>,
    writing_mode: WritingMode,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
    percentage_max_content_inline_size: Option<f32>,
) -> Size<f32> {
    let ratio_basis =
        ratio_only_stretch_basis(available_space, writing_mode, padding_border, percentage_max_content_inline_size);
    complete_replaced_size(apply_aspect_ratio_to_content_size(ratio_basis, aspect_ratio, padding_border), None)
        .expect("a valid preferred ratio resolves the stretch-fit axis")
}

/// Resolve the independently chosen logical inline basis for ratio-only
/// replaced content. Keeping this source separate lets intrinsic measurement
/// report which orthogonal axis was synthesized through the ratio.
fn ratio_only_stretch_basis(
    available_space: Size<AvailableSpace>,
    writing_mode: WritingMode,
    padding_border: Size<f32>,
    percentage_max_content_inline_size: Option<f32>,
) -> Size<Option<f32>> {
    let stretch_inline_axis = |space: AvailableSpace, inset: f32| match space {
        AvailableSpace::Definite(size) => (size - inset).max(0.0),
        AvailableSpace::MinContent => 0.0,
        AvailableSpace::MaxContent => percentage_max_content_inline_size.unwrap_or(0.0),
    };
    if writing_mode.is_horizontal() {
        Size { width: Some(stretch_inline_axis(available_space.width, padding_border.width)), height: None }
    } else {
        Size { width: None, height: Some(stretch_inline_axis(available_space.height, padding_border.height)) }
    }
}

/// Fill missing candidate axes from a complete natural size, or return a
/// fully determined candidate when no fallback exists.
fn complete_replaced_size(size: Size<Option<f32>>, fallback: Option<Size<f32>>) -> Option<Size<f32>> {
    match fallback {
        Some(fallback) => Some(size.unwrap_or(fallback)),
        None => Some(Size { width: size.width?, height: size.height? }),
    }
}

/// Resolve one intrinsic constraint as if preferred lengths in that physical
/// axis were ignored.
///
/// Replaced content has a single intrinsic size rather than independently
/// measured min/max-content extrema. A fixed or preferred opposite axis may
/// replace that natural value through the used aspect ratio, which is why the
/// queried axis is removed before the candidate is completed.
fn intrinsic_constraint_size(
    axis: AbsoluteAxis,
    sizing_basis: Size<Option<f32>>,
    fallback: Option<Size<f32>>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Option<f32> {
    let axis_ignored = match axis {
        AbsoluteAxis::Horizontal => Size { width: None, height: sizing_basis.height },
        AbsoluteAxis::Vertical => Size { width: sizing_basis.width, height: None },
    };
    complete_replaced_size(apply_aspect_ratio_to_content_size(axis_ignored, aspect_ratio, padding_border), fallback)
        .map(|size| size.get_abs(axis))
}

/// Identify axes synthesized from the opposite source through a preferred
/// ratio. Axes already present in the sizing basis retain direct provenance.
fn ratio_derived_axes_from_basis(basis: Size<Option<f32>>, aspect_ratio: ResolvedAspectRatio) -> Size<bool> {
    if !aspect_ratio.has_ratio() {
        return Size { width: false, height: false };
    }
    Size {
        width: basis.width.is_none() && basis.height.is_some(),
        height: basis.height.is_none() && basis.width.is_some(),
    }
}

/// Construct an output whose content extent is the atomic replaced box while
/// retaining operation-local preferred-ratio provenance for intrinsic probes.
fn replaced_output(
    size: Size<f32>,
    run_mode: RunMode,
    requested_axis: RequestedAxis,
    ratio_derived_axes: Size<bool>,
) -> LayoutOutput {
    let applied_aspect_ratio = run_mode == RunMode::ComputeSize
        && match requested_axis {
            RequestedAxis::Horizontal => ratio_derived_axes.width,
            RequestedAxis::Vertical => ratio_derived_axes.height,
            RequestedAxis::Both => ratio_derived_axes.width || ratio_derived_axes.height,
        };
    LayoutOutput::from_sizes(size, size).with_applied_aspect_ratio(applied_aspect_ratio)
}

/// Classify one dimension against its used minimum and maximum.
fn constraint_violation(size: f32, minimum: Option<f32>, maximum: Option<f32>) -> ConstraintViolation {
    if size < minimum.unwrap_or(0.0) {
        ConstraintViolation::Minimum
    } else if size > maximum.unwrap_or(f32::INFINITY) {
        ConstraintViolation::Maximum
    } else {
        ConstraintViolation::None
    }
}

/// Transfer a content-box height to width through the used ratio.
fn content_width_from_height(height: f32, aspect_ratio: ResolvedAspectRatio, padding_border: Size<f32>) -> Option<f32> {
    Size { width: None, height: Some(height) }
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::ContentBox, padding_border)
        .width
}

/// Transfer a content-box width to height through the used ratio.
fn content_height_from_width(width: f32, aspect_ratio: ResolvedAspectRatio, padding_border: Size<f32>) -> Option<f32> {
    Size { width: Some(width), height: None }
        .maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::ContentBox, padding_border)
        .height
}

/// Fill one missing content-box axis through the used ratio.
fn apply_aspect_ratio_to_content_size(
    size: Size<Option<f32>>,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<Option<f32>> {
    size.maybe_apply_aspect_ratio_with_box_sizing(aspect_ratio, BoxSizing::ContentBox, padding_border)
}

/// Compare constraint severity in the ratio's selected sizing-box space.
fn ratio_basis_scale(constrained: f32, original: f32, inset: f32, box_sizing: BoxSizing) -> f32 {
    match box_sizing {
        BoxSizing::ContentBox => constrained / original,
        BoxSizing::BorderBox => (constrained + inset) / (original + inset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dimension, Rect, SizingPurpose, Style};

    type TestStyle = Style<crate::sys::DefaultCheapStr>;

    #[cfg(feature = "calc")]
    #[repr(align(8))]
    struct ReplacedCalcToken;

    #[cfg(feature = "calc")]
    static REPLACED_CALC_TOKEN: ReplacedCalcToken = ReplacedCalcToken;

    fn inputs(parent_size: Size<Option<f32>>) -> LayoutInput {
        LayoutInput {
            run_mode: crate::RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            parent_size,
            parent_writing_mode: WritingMode::HorizontalTb,
            available_space: Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent },
            ..LayoutInput::HIDDEN
        }
    }

    fn context(aspect_ratio: ResolvedAspectRatio, size_containment: SizeContainment) -> ReplacedSizingContext {
        ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            aspect_ratio,
            size_containment,
            ReplacedNaturalSizing::fixed(Size { width: 60.0, height: 60.0 }),
        )
    }

    fn min_content_contribution(
        writing_mode: WritingMode,
        preferred_inline_size: Dimension,
        min_content_kind: ReplacedMinContentKind,
        resolve_calc_value: impl Fn(*const (), f32) -> f32,
    ) -> Size<f32> {
        let style: TestStyle = Style {
            size: writing_mode.to_physical(crate::geometry::LogicalSize {
                inline_size: preferred_inline_size,
                block_size: Dimension::auto(),
            }),
            ..Style::default()
        };
        let context = ReplacedSizingContext::new(
            writing_mode,
            ResolvedAspectRatio::from_option(None, BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::fixed(
                writing_mode.to_physical(crate::geometry::LogicalSize { inline_size: 240.0, block_size: 20.0 }),
            ),
        )
        .with_min_content_kind(min_content_kind);
        let mut input = inputs(Size::NONE);
        input.parent_writing_mode = writing_mode;
        input.sizing_mode = SizingMode::ContentSize;
        input.sizing_purpose = SizingPurpose::IntrinsicContribution;
        input.axis = writing_mode.inline_axis().into();
        input.known_dimensions =
            writing_mode.to_physical(crate::geometry::LogicalSize { inline_size: None, block_size: Some(40.0) });
        input.definite_dimensions = input.known_dimensions;
        input.available_space = writing_mode.to_physical(crate::geometry::LogicalSize {
            inline_size: AvailableSpace::MinContent,
            block_size: AvailableSpace::Definite(40.0),
        });

        compute_replaced_layout(input, &style, context, resolve_calc_value).size
    }

    fn measure(style: &TestStyle) -> Size<f32> {
        compute_replaced_layout(
            inputs(Size::NONE),
            style,
            context(
                ResolvedAspectRatio::from_option(
                    style.aspect_ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0).or(Some(1.0)),
                    style.box_sizing,
                ),
                SizeContainment::NONE,
            ),
            |_, _| 0.0,
        )
        .size
    }

    #[test]
    fn definite_available_space_is_not_an_implicit_maximum() {
        let natural_style: TestStyle = Style::default();
        let percentage_style: TestStyle = Style {
            size: Size { width: Dimension::percent(1.25), height: Dimension::length(40.0) },
            ..Style::default()
        };
        let authored_max_style: TestStyle =
            Style { max_size: Size { width: Dimension::length(40.0), height: Dimension::auto() }, ..Style::default() };
        let measure_with_available_width = |style: &TestStyle, parent_width, available_width| {
            let mut input = inputs(Size { width: parent_width, height: None });
            input.available_space.width = AvailableSpace::Definite(available_width);
            compute_replaced_layout(
                input,
                style,
                context(ResolvedAspectRatio::none(style.box_sizing), SizeContainment::NONE),
                |_, _| 0.0,
            )
            .size
        };

        assert_eq!(measure_with_available_width(&natural_style, None, 40.0).width, 60.0);
        assert_eq!(measure_with_available_width(&percentage_style, Some(200.0), 200.0).width, 250.0);
        assert_eq!(measure_with_available_width(&authored_max_style, None, 200.0).width, 40.0);
    }

    #[test]
    fn containment_replaces_natural_axes_without_creating_a_ratio() {
        let style: TestStyle = Style::default();
        let measure_contained = |containment| {
            compute_replaced_layout(
                inputs(Size::NONE),
                &style,
                context(ResolvedAspectRatio::none(BoxSizing::BorderBox), containment),
                |_, _| 0.0,
            )
            .size
        };

        assert_eq!(
            measure_contained(SizeContainment::new(
                Size { width: true, height: true },
                Size { width: Some(100.0), height: Some(200.0) },
            )),
            Size { width: 100.0, height: 200.0 }
        );
        assert_eq!(
            measure_contained(SizeContainment::new(
                Size { width: true, height: false },
                Size { width: Some(100.0), height: Some(200.0) },
            )),
            Size { width: 100.0, height: 60.0 }
        );

        let explicit_width: TestStyle =
            Style { size: Size { width: Dimension::length(100.0), height: Dimension::auto() }, ..Style::default() };
        assert_eq!(
            compute_replaced_layout(
                inputs(Size::NONE),
                &explicit_width,
                context(
                    ResolvedAspectRatio::none(BoxSizing::BorderBox),
                    SizeContainment::new(
                        Size { width: true, height: true },
                        Size { width: Some(50.0), height: Some(100.0) },
                    ),
                ),
                |_, _| 0.0,
            )
            .size,
            Size { width: 100.0, height: 100.0 }
        );
    }

    #[test]
    fn compressible_control_percentage_contribution_resolves_against_zero() {
        assert_eq!(
            min_content_contribution(
                WritingMode::HorizontalTb,
                Dimension::percent(1.0),
                ReplacedMinContentKind::CompressibleControl,
                |_, _| 0.0,
            ),
            Size { width: 0.0, height: 40.0 }
        );
        assert_eq!(
            min_content_contribution(
                WritingMode::VerticalLr,
                Dimension::percent(1.0),
                ReplacedMinContentKind::CompressibleControl,
                |_, _| 0.0,
            ),
            Size { width: 40.0, height: 0.0 }
        );
    }

    #[cfg(feature = "calc")]
    #[test]
    fn only_compressible_controls_keep_the_absolute_calc_contribution() {
        let preferred_size = Dimension::calc((&REPLACED_CALC_TOKEN as *const ReplacedCalcToken).cast());

        assert_eq!(
            min_content_contribution(
                WritingMode::HorizontalTb,
                preferred_size,
                ReplacedMinContentKind::CompressibleControl,
                |_, basis| 140.0 + basis,
            ),
            Size { width: 140.0, height: 40.0 }
        );
        assert_eq!(
            min_content_contribution(
                WritingMode::VerticalLr,
                preferred_size,
                ReplacedMinContentKind::CompressibleControl,
                |_, basis| 140.0 + basis,
            ),
            Size { width: 40.0, height: 140.0 }
        );
        assert_eq!(
            min_content_contribution(
                WritingMode::HorizontalTb,
                preferred_size,
                ReplacedMinContentKind::NaturalObject,
                |_, basis| 140.0 + basis,
            ),
            Size { width: 0.0, height: 40.0 }
        );
    }

    #[test]
    fn intrinsic_probe_reports_a_cross_size_transferred_from_the_preferred_inline_size() {
        let style: TestStyle =
            Style { size: Size { width: Dimension::length(100.0), height: Dimension::auto() }, ..Style::default() };
        let context = ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::fixed(Size { width: 10.0, height: 10.0 }),
        );
        let mut input = inputs(Size { width: Some(10.0), height: None });
        input.sizing_mode = SizingMode::ContentSize;
        input.sizing_purpose = SizingPurpose::IntrinsicContribution;
        input.axis = RequestedAxis::Vertical;
        input.available_space = Size { width: AvailableSpace::Definite(10.0), height: AvailableSpace::MinContent };

        let result = compute_replaced_layout(input, &style, context, |_, _| 0.0).into_intrinsic_size_result();

        assert_eq!(result.size, Size { width: 100.0, height: 100.0 });
        assert!(result.applied_aspect_ratio);
    }

    #[test]
    fn preferred_ratio_uses_its_selected_sizing_box() {
        let style: TestStyle = Style {
            box_sizing: BoxSizing::BorderBox,
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            padding: Rect {
                left: crate::LengthPercentage::length(10.0),
                right: crate::LengthPercentage::length(10.0),
                top: crate::LengthPercentage::length(10.0),
                bottom: crate::LengthPercentage::length(10.0),
            },
            aspect_ratio: Some(2.0),
            ..Style::default()
        };
        let measure_with_ratio_box = |box_sizing| {
            compute_replaced_layout(
                inputs(Size::NONE),
                &style,
                context(ResolvedAspectRatio::from_option(Some(2.0), box_sizing), SizeContainment::NONE),
                |_, _| 0.0,
            )
            .size
        };

        assert_eq!(measure_with_ratio_box(BoxSizing::BorderBox), Size { width: 100.0, height: 50.0 });
        assert_eq!(measure_with_ratio_box(BoxSizing::ContentBox), Size { width: 100.0, height: 60.0 });
    }

    /// Blink resolves an authored replaced axis independently, then derives
    /// only the still-auto axis through the preferred ratio. A constraint on
    /// that derived axis must not feed back into the authored axis, while a
    /// constraint on the authored axis does change the ratio basis.
    #[test]
    fn definite_preferred_axes_own_replaced_constraint_order() {
        let auto = Dimension::auto;
        let px = Dimension::length;
        let cases = [
            (
                "derived height maximum",
                Size { width: px(200.0), height: auto() },
                Size { width: auto(), height: auto() },
                Size { width: auto(), height: px(60.0) },
                Size { width: 200.0, height: 60.0 },
            ),
            (
                "derived width maximum",
                Size { width: auto(), height: px(90.0) },
                Size { width: auto(), height: auto() },
                Size { width: px(100.0), height: auto() },
                Size { width: 100.0, height: 90.0 },
            ),
            (
                "two authored axes",
                Size { width: px(200.0), height: px(80.0) },
                Size { width: auto(), height: auto() },
                Size { width: auto(), height: px(60.0) },
                Size { width: 200.0, height: 60.0 },
            ),
            (
                "authored width maximum",
                Size { width: px(200.0), height: auto() },
                Size { width: auto(), height: auto() },
                Size { width: px(100.0), height: auto() },
                Size { width: 100.0, height: 50.0 },
            ),
            (
                "authored width minimum",
                Size { width: px(200.0), height: auto() },
                Size { width: px(240.0), height: auto() },
                Size { width: auto(), height: auto() },
                Size { width: 240.0, height: 120.0 },
            ),
            (
                "authored width and derived height constraints",
                Size { width: px(200.0), height: auto() },
                Size { width: auto(), height: px(80.0) },
                Size { width: px(100.0), height: auto() },
                Size { width: 100.0, height: 80.0 },
            ),
        ];

        for (label, size, min_size, max_size, expected) in cases {
            let style: TestStyle = Style { size, min_size, max_size, aspect_ratio: Some(2.0), ..Style::default() };
            let actual = compute_replaced_layout(
                inputs(Size::NONE),
                &style,
                ReplacedSizingContext::new(
                    WritingMode::HorizontalTb,
                    ResolvedAspectRatio::from_option(Some(2.0), BoxSizing::ContentBox),
                    SizeContainment::NONE,
                    ReplacedNaturalSizing::fixed(Size { width: 120.0, height: 60.0 }),
                ),
                |_, _| 0.0,
            )
            .size;
            assert_eq!(actual, expected, "{label}");
        }
    }

    /// Regression for
    /// <https://wpt.live/css/css-sizing/aspect-ratio/box-sizing-squashed.html>.
    #[test]
    fn border_box_floor_precedes_replaced_aspect_ratio_transfer() {
        let border = Rect {
            left: crate::LengthPercentage::length(20.0),
            right: crate::LengthPercentage::length(20.0),
            top: crate::LengthPercentage::length(20.0),
            bottom: crate::LengthPercentage::length(20.0),
        };
        let auto = Dimension::auto;
        let px = Dimension::length;
        let cases = [
            (
                "horizontal explicit inline size",
                2.0,
                Size { width: px(50.0), height: auto() },
                Size { width: auto(), height: auto() },
                Size { width: 50.0, height: 40.0 },
            ),
            (
                "horizontal explicit block size",
                2.0,
                Size { width: auto(), height: px(20.0) },
                Size { width: auto(), height: auto() },
                Size { width: 80.0, height: 40.0 },
            ),
            (
                "horizontal mapped inline size with maximum",
                2.0,
                Size { width: px(20.0), height: auto() },
                Size { width: px(50.0), height: auto() },
                Size { width: 40.0, height: 40.0 },
            ),
            (
                "horizontal mapped block size with maximum",
                2.0,
                Size { width: auto(), height: px(50.0) },
                Size { width: auto(), height: px(20.0) },
                Size { width: 80.0, height: 40.0 },
            ),
            (
                "vertical explicit block size",
                0.5,
                Size { width: auto(), height: px(50.0) },
                Size { width: auto(), height: auto() },
                Size { width: 40.0, height: 50.0 },
            ),
            (
                "vertical explicit inline size",
                0.5,
                Size { width: px(20.0), height: auto() },
                Size { width: auto(), height: auto() },
                Size { width: 40.0, height: 80.0 },
            ),
            (
                "vertical mapped block size with maximum",
                0.5,
                Size { width: auto(), height: px(50.0) },
                Size { width: auto(), height: px(50.0) },
                Size { width: 40.0, height: 50.0 },
            ),
            (
                "vertical mapped inline size with maximum",
                0.5,
                Size { width: px(20.0), height: auto() },
                Size { width: px(20.0), height: auto() },
                Size { width: 40.0, height: 80.0 },
            ),
        ];

        for (label, ratio, size, max_size, expected) in cases {
            let style: TestStyle = Style {
                box_sizing: BoxSizing::BorderBox,
                size,
                max_size,
                border,
                aspect_ratio: Some(ratio),
                ..Style::default()
            };
            let context = ReplacedSizingContext::new(
                WritingMode::HorizontalTb,
                ResolvedAspectRatio::from_option(Some(ratio), BoxSizing::BorderBox),
                SizeContainment::NONE,
                ReplacedNaturalSizing::fixed(Size { width: 20.0, height: 50.0 }),
            );
            let actual = compute_replaced_layout(inputs(Size::NONE), &style, context, |_, _| 0.0).size;
            assert_eq!(actual, expected, "{label}");
        }
    }

    /// Regression for
    /// <https://wpt.live/css/css-sizing/aspect-ratio/replaced-element-034.html>.
    ///
    /// The resource has a natural width but no natural height or ratio. Its
    /// fallback object height must not replace the `min-content` height: that
    /// intrinsic axis is transferred from the natural width through the
    /// authored border-box ratio.
    #[test]
    fn intrinsic_height_uses_natural_width_and_the_preferred_ratio_box() {
        let style: TestStyle = Style {
            box_sizing: BoxSizing::BorderBox,
            size: Size { width: Dimension::auto(), height: Dimension::min_content() },
            padding: Rect { left: crate::LengthPercentage::length(50.0), ..Rect::zero() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };

        let size = compute_replaced_layout(
            inputs(Size::NONE),
            &style,
            ReplacedSizingContext::new(
                WritingMode::HorizontalTb,
                ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::BorderBox),
                SizeContainment::NONE,
                ReplacedNaturalSizing::new(
                    Size { width: Some(50.0), height: None },
                    Size { width: 300.0, height: 150.0 },
                ),
            ),
            |_, _| 0.0,
        )
        .size;

        assert_eq!(size, Size { width: 100.0, height: 100.0 });
    }

    #[test]
    fn default_object_size_fills_only_naturally_missing_axes_without_a_ratio() {
        let style: TestStyle = Style::default();
        let size = compute_replaced_layout(
            inputs(Size::NONE),
            &style,
            ReplacedSizingContext::new(
                WritingMode::HorizontalTb,
                ResolvedAspectRatio::none(BoxSizing::ContentBox),
                SizeContainment::NONE,
                ReplacedNaturalSizing::new(
                    Size { width: Some(50.0), height: None },
                    Size { width: 300.0, height: 150.0 },
                ),
            ),
            |_, _| 0.0,
        )
        .size;

        assert_eq!(size, Size { width: 50.0, height: 150.0 });
    }

    #[test]
    fn preferred_ratio_normalizes_natural_sizes_from_the_logical_inline_axis() {
        let style: TestStyle = Style::default();
        let measure = |writing_mode| {
            compute_replaced_layout(
                inputs(Size::NONE),
                &style,
                ReplacedSizingContext::new(
                    writing_mode,
                    ResolvedAspectRatio::from_option(Some(2.0), BoxSizing::ContentBox),
                    SizeContainment::NONE,
                    ReplacedNaturalSizing::new(
                        Size { width: Some(50.0), height: Some(80.0) },
                        Size { width: 300.0, height: 150.0 },
                    ),
                ),
                |_, _| 0.0,
            )
            .size
        };

        assert_eq!(measure(WritingMode::HorizontalTb), Size { width: 50.0, height: 25.0 });
        assert_eq!(measure(WritingMode::VerticalRl), Size { width: 160.0, height: 80.0 });
    }

    #[test]
    fn ratio_only_replaced_content_stretches_in_its_logical_inline_axis() {
        let style: TestStyle = Style::default();
        let measure = |writing_mode, available_space| {
            let mut input = inputs(Size::NONE);
            input.available_space = available_space;
            compute_replaced_layout(
                input,
                &style,
                ReplacedSizingContext::new(
                    writing_mode,
                    ResolvedAspectRatio::from_option(Some(2.0), BoxSizing::ContentBox),
                    SizeContainment::NONE,
                    ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
                ),
                |_, _| 0.0,
            )
            .size
        };

        assert_eq!(
            measure(
                WritingMode::HorizontalTb,
                Size { width: AvailableSpace::Definite(200.0), height: AvailableSpace::MaxContent },
            ),
            Size { width: 200.0, height: 100.0 },
        );
        assert_eq!(
            measure(
                WritingMode::VerticalRl,
                Size { width: AvailableSpace::MaxContent, height: AvailableSpace::Definite(120.0) },
            ),
            Size { width: 240.0, height: 120.0 },
        );
    }

    #[test]
    fn percentage_padding_uses_the_containing_block_logical_inline_size() {
        let style: TestStyle = Style {
            box_sizing: BoxSizing::ContentBox,
            size: Size { width: Dimension::length(10.0), height: Dimension::length(10.0) },
            padding: Rect {
                left: crate::LengthPercentage::percent(0.1),
                right: crate::LengthPercentage::percent(0.1),
                top: crate::LengthPercentage::length(0.0),
                bottom: crate::LengthPercentage::length(0.0),
            },
            ..Style::default()
        };
        let parent_size = Size { width: Some(100.0), height: Some(200.0) };
        let mut input = inputs(parent_size);
        input.parent_writing_mode = WritingMode::VerticalRl;
        let size = compute_replaced_layout(
            input,
            &style,
            ReplacedSizingContext::new(
                WritingMode::VerticalRl,
                ResolvedAspectRatio::none(BoxSizing::ContentBox),
                SizeContainment::NONE,
                ReplacedNaturalSizing::fixed(Size { width: 60.0, height: 60.0 }),
            ),
            |_, _| 0.0,
        )
        .size;

        assert_eq!(size, Size { width: 50.0, height: 10.0 });
    }

    #[test]
    fn intrinsic_constraints_transfer_the_opposite_preferred_axis() {
        let mut min_width: TestStyle = Style {
            size: Size { width: Dimension::length(30.0), height: Dimension::length(40.0) },
            min_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
            ..Style::default()
        };
        assert_eq!(measure(&min_width), Size { width: 40.0, height: 40.0 });
        min_width.min_size.width = Dimension::max_content();
        assert_eq!(measure(&min_width), Size { width: 40.0, height: 40.0 });

        let mut max_width: TestStyle = Style {
            size: Size { width: Dimension::length(80.0), height: Dimension::length(70.0) },
            max_size: Size { width: Dimension::min_content(), height: Dimension::auto() },
            ..Style::default()
        };
        assert_eq!(measure(&max_width), Size { width: 70.0, height: 70.0 });
        max_width.max_size.width = Dimension::max_content();
        assert_eq!(measure(&max_width), Size { width: 70.0, height: 70.0 });

        let mut min_height: TestStyle = Style {
            size: Size { width: Dimension::length(40.0), height: Dimension::length(30.0) },
            min_size: Size { width: Dimension::auto(), height: Dimension::min_content() },
            ..Style::default()
        };
        assert_eq!(measure(&min_height), Size { width: 40.0, height: 40.0 });
        min_height.min_size.height = Dimension::max_content();
        assert_eq!(measure(&min_height), Size { width: 40.0, height: 40.0 });

        let mut max_height: TestStyle = Style {
            size: Size { width: Dimension::length(70.0), height: Dimension::length(80.0) },
            max_size: Size { width: Dimension::auto(), height: Dimension::min_content() },
            ..Style::default()
        };
        assert_eq!(measure(&max_height), Size { width: 70.0, height: 70.0 });
        max_height.max_size.height = Dimension::max_content();
        assert_eq!(measure(&max_height), Size { width: 70.0, height: 70.0 });
    }

    /// Regression for
    /// <https://wpt.live/css/css-sizing/keyword-sizes-for-intrinsic-contributions-002.html>.
    ///
    /// Intrinsic block-size constraints are resolved with the replaced
    /// element's intrinsic size before its intrinsic inline contribution is
    /// computed. Clamping the preferred block size must therefore feed back
    /// through the preferred ratio into an intrinsic inline-size keyword.
    #[test]
    fn intrinsic_block_constraints_bound_the_intrinsic_inline_contribution() {
        let measure = |style: &TestStyle, ratio| {
            compute_replaced_layout(
                inputs(Size::NONE),
                style,
                ReplacedSizingContext::new(
                    WritingMode::HorizontalTb,
                    ResolvedAspectRatio::from_option(ratio, BoxSizing::ContentBox),
                    SizeContainment::NONE,
                    ReplacedNaturalSizing::fixed(Size { width: 50.0, height: 50.0 }),
                ),
                |_, _| 0.0,
            )
            .size
        };
        let minimum: TestStyle = Style {
            size: Size { width: Dimension::max_content(), height: Dimension::length(0.0) },
            min_size: Size { width: Dimension::auto(), height: Dimension::max_content() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };
        let maximum: TestStyle = Style {
            size: Size { width: Dimension::max_content(), height: Dimension::length(100.0) },
            max_size: Size { width: Dimension::auto(), height: Dimension::max_content() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };

        for ratio in [Some(1.0), None] {
            assert_eq!(measure(&minimum, ratio), Size { width: 50.0, height: 50.0 });
            assert_eq!(measure(&maximum, ratio), Size { width: 50.0, height: 50.0 });
        }
    }

    /// Regression for
    /// <https://wpt.live/css/css-sizing/replaced-aspect-ratio-stretch-fit-003.html>.
    ///
    /// An intrinsic minimum resolved from the opposite preferred axis is a
    /// late constraint. It must still floor an authored maximum before the
    /// replaced-size constraint violation table is applied.
    #[test]
    fn transferred_intrinsic_minimum_takes_precedence_over_authored_maximum() {
        let style: TestStyle = Style {
            size: Size { width: Dimension::auto(), height: Dimension::percent(1.0) },
            min_size: Size { width: Dimension::max_content(), height: Dimension::auto() },
            max_size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
            ..Style::default()
        };
        let mut input = inputs(Size { width: Some(100.0), height: Some(100.0) });
        input.available_space =
            Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::Definite(100.0) };

        let size = compute_replaced_layout(
            input,
            &style,
            ReplacedSizingContext::new(
                WritingMode::HorizontalTb,
                ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::ContentBox),
                SizeContainment::NONE,
                ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
            ),
            |_, _| 0.0,
        )
        .size;

        assert_eq!(size, Size { width: 100.0, height: 100.0 });
    }

    #[test]
    fn min_content_percentages_use_zero_for_preferred_and_max_but_not_minimum() {
        let style: TestStyle = Style {
            size: Size { width: Dimension::percent(0.5), height: Dimension::auto() },
            min_size: Size { width: Dimension::percent(0.25), height: Dimension::auto() },
            max_size: Size { width: Dimension::percent(0.75), height: Dimension::auto() },
            ..Style::default()
        };
        let mut input = inputs(Size { width: Some(200.0), height: None });
        input.available_space.width = AvailableSpace::MinContent;

        assert_eq!(
            compute_replaced_layout(
                input,
                &style,
                context(ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::BorderBox), SizeContainment::NONE,),
                |_, _| 0.0,
            )
            .size,
            Size { width: 50.0, height: 50.0 }
        );
    }

    /// Regression for
    /// <https://wpt.live/css/css-sizing/svg-intrinsic-size-006.html>.
    ///
    /// A percentage-sized replaced box with only a natural ratio contributes
    /// its category's default object size to max-content. Its min-content
    /// contribution remains zero, and a definite containing block resolves
    /// the authored percentage normally. This is the same split as Blink's
    /// replaced StretchFit fallback.
    #[test]
    fn ratio_only_percentage_size_uses_default_object_max_content_contribution() {
        let style: TestStyle = Style {
            size: Size { width: Dimension::percent(0.1), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };
        let context = ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
        );
        let contribution = |available_width| {
            let mut input = inputs(Size::NONE);
            input.sizing_purpose = crate::SizingPurpose::IntrinsicContribution;
            input.axis = RequestedAxis::Horizontal;
            input.available_space.width = available_width;
            compute_replaced_layout(input, &style, context, |_, _| 0.0).size
        };

        assert_eq!(contribution(AvailableSpace::MinContent), Size { width: 0.0, height: 0.0 });
        assert_eq!(contribution(AvailableSpace::MaxContent), Size { width: 300.0, height: 300.0 });

        let mut layout_input = inputs(Size { width: Some(200.0), height: None });
        layout_input.run_mode = crate::RunMode::PerformLayout;
        layout_input.sizing_purpose = crate::SizingPurpose::Layout;
        layout_input.available_space.width = AvailableSpace::Definite(200.0);
        assert_eq!(
            compute_replaced_layout(layout_input, &style, context, |_, _| 0.0).size,
            Size { width: 20.0, height: 20.0 }
        );
    }

    #[cfg(feature = "calc")]
    #[test]
    fn ratio_only_percentage_calc_uses_intrinsic_contribution_rules() {
        let style: TestStyle = Style {
            size: Size {
                width: Dimension::calc((&REPLACED_CALC_TOKEN as *const ReplacedCalcToken).cast()),
                height: Dimension::auto(),
            },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };
        let context = ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
        );
        let resolve_calc = |_: *const (), basis: f32| 20.0 + 0.1 * basis;
        let contribution = |available_width| {
            let mut input = inputs(Size::NONE);
            input.sizing_purpose = SizingPurpose::IntrinsicContribution;
            input.axis = RequestedAxis::Horizontal;
            input.available_space.width = available_width;
            compute_replaced_layout(input, &style, context, resolve_calc).size
        };

        assert_eq!(contribution(AvailableSpace::MinContent), Size { width: 0.0, height: 0.0 });
        assert_eq!(contribution(AvailableSpace::MaxContent), Size { width: 300.0, height: 300.0 });

        let mut layout_input = inputs(Size { width: Some(200.0), height: None });
        layout_input.sizing_purpose = SizingPurpose::Layout;
        layout_input.available_space.width = AvailableSpace::Definite(200.0);
        assert_eq!(
            compute_replaced_layout(layout_input, &style, context, resolve_calc).size,
            Size { width: 40.0, height: 40.0 }
        );
    }

    #[test]
    fn percentage_dependent_min_content_honors_minimum_and_maximum_constraints() {
        let context = ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
        );
        let contribution = |style: &TestStyle, available_width| {
            let mut input = inputs(Size::NONE);
            input.sizing_purpose = SizingPurpose::IntrinsicContribution;
            input.axis = RequestedAxis::Horizontal;
            input.available_space.width = available_width;
            compute_replaced_layout(input, style, context, |_, _| 0.0).size
        };

        let percentage_with_minimum: TestStyle = Style {
            size: Size { width: Dimension::percent(0.1), height: Dimension::auto() },
            min_size: Size { width: Dimension::length(50.0), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };
        assert_eq!(
            contribution(&percentage_with_minimum, AvailableSpace::MinContent),
            Size { width: 50.0, height: 50.0 }
        );
        assert_eq!(
            contribution(&percentage_with_minimum, AvailableSpace::MaxContent),
            Size { width: 300.0, height: 300.0 }
        );

        let percentage_maximum: TestStyle = Style {
            size: Size { width: Dimension::length(100.0), height: Dimension::auto() },
            max_size: Size { width: Dimension::percent(0.5), height: Dimension::auto() },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };
        assert_eq!(contribution(&percentage_maximum, AvailableSpace::MinContent), Size { width: 0.0, height: 0.0 });
        assert_eq!(contribution(&percentage_maximum, AvailableSpace::MaxContent), Size { width: 100.0, height: 100.0 });
    }

    #[test]
    fn percentage_default_object_fallback_uses_logical_inline_axis() {
        let style: TestStyle = Style {
            size: Size { width: Dimension::auto(), height: Dimension::percent(0.1) },
            aspect_ratio: Some(1.0),
            ..Style::default()
        };
        let context = ReplacedSizingContext::new(
            WritingMode::VerticalRl,
            ResolvedAspectRatio::from_option(Some(1.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
        );
        let mut input = inputs(Size::NONE);
        input.parent_writing_mode = WritingMode::VerticalRl;
        input.sizing_purpose = SizingPurpose::IntrinsicContribution;
        input.axis = RequestedAxis::Vertical;
        input.available_space = Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent };

        assert_eq!(
            compute_replaced_layout(input, &style, context, |_, _| 0.0).size,
            Size { width: 150.0, height: 150.0 }
        );
    }

    #[test]
    fn percentage_default_fallback_does_not_override_stronger_replaced_data() {
        let percentage_width: TestStyle = Style {
            size: Size { width: Dimension::percent(0.1), height: Dimension::auto() },
            aspect_ratio: Some(2.0),
            ..Style::default()
        };
        let mut input = inputs(Size::NONE);
        input.sizing_purpose = SizingPurpose::IntrinsicContribution;
        input.axis = RequestedAxis::Horizontal;

        let natural_width = ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            ResolvedAspectRatio::from_option(Some(2.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::new(Size { width: Some(100.0), height: None }, Size { width: 300.0, height: 150.0 }),
        );
        assert_eq!(
            compute_replaced_layout(input, &percentage_width, natural_width, |_, _| 0.0).size,
            Size { width: 100.0, height: 50.0 }
        );

        let fixed_block_axis: TestStyle = Style {
            size: Size { width: Dimension::percent(0.1), height: Dimension::length(50.0) },
            aspect_ratio: Some(2.0),
            ..Style::default()
        };
        let ratio_only = ReplacedSizingContext::new(
            WritingMode::HorizontalTb,
            ResolvedAspectRatio::from_option(Some(2.0), BoxSizing::ContentBox),
            SizeContainment::NONE,
            ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
        );
        assert_eq!(
            compute_replaced_layout(input, &fixed_block_axis, ratio_only, |_, _| 0.0).size,
            Size { width: 100.0, height: 50.0 }
        );
    }

    #[test]
    fn known_parent_size_transfers_then_clamps_without_overriding_known_axis() {
        let style: TestStyle = Style {
            min_size: Size { width: Dimension::auto(), height: Dimension::length(40.0) },
            max_size: Size { width: Dimension::auto(), height: Dimension::length(80.0) },
            ..Style::default()
        };
        let mut input = inputs(Size::NONE);
        input.known_dimensions.width = Some(120.0);

        assert_eq!(
            compute_replaced_layout(
                input,
                &style,
                context(ResolvedAspectRatio::from_option(Some(2.0), BoxSizing::BorderBox), SizeContainment::NONE,),
                |_, _| 0.0,
            )
            .size,
            Size { width: 120.0, height: 60.0 }
        );
    }
}
