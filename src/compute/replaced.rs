//! CSS sizing for replaced content such as images, canvas and form controls.
//!
//! Replaced content differs from an ordinary measured leaf in two important
//! ways: its natural dimensions participate in preferred/min/max sizing, and
//! constraints transferred through its preferred aspect ratio must be applied
//! as part of the same sizing operation. Keeping this algorithm in Taffy means
//! embedding engines provide content metrics rather than reimplementing the
//! CSS box model in a measurement callback.

use crate::geometry::Size;
use crate::style::{AvailableSpace, BoxSizing, CoreStyle, ResolvedAspectRatio, SizeContainment};
use crate::tree::{LayoutInput, LayoutOutput, RequestedAxis, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::WritingMode;

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
    /// Host-provided preferred-size hint used only when CSS leaves both
    /// physical preferred axes automatic. HTML dimension attributes are one
    /// source of such a hint.
    pub preferred_size_hint: Size<Option<f32>>,
}

impl ReplacedSizingContext {
    /// Construct the used values for a replaced sizing operation.
    pub const fn new(
        writing_mode: WritingMode,
        aspect_ratio: ResolvedAspectRatio,
        size_containment: SizeContainment,
        natural_sizing: ReplacedNaturalSizing,
        preferred_size_hint: Size<Option<f32>>,
    ) -> Self {
        Self { writing_mode, aspect_ratio, size_containment, natural_sizing, preferred_size_hint }
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
    let LayoutInput { known_dimensions, parent_size, available_space, sizing_mode, axis: requested_axis, .. } = inputs;

    let padding = style.padding().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let border = style.border().resolve_or_zero(percentage_basis, &resolve_calc_value);
    let padding_border = padding + border;
    let padding_border_sum = padding_border.sum_axes();
    let box_sizing_adjustment =
        if style.box_sizing() == BoxSizing::BorderBox { padding_border_sum } else { Size::ZERO };

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
    let natural_size = normalized_natural_size(
        natural_dimensions,
        default_object_size,
        context.aspect_ratio,
        context.writing_mode,
        padding_border_sum,
    );
    let preferred_size_hint = Size {
        width: if context.size_containment.axes.width { None } else { context.preferred_size_hint.width },
        height: if context.size_containment.axes.height { None } else { context.preferred_size_hint.height },
    };

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
    let mut preferred_size =
        raw_size.maybe_resolve(preferred_percentage_basis, &resolve_calc_value).maybe_sub(box_sizing_adjustment);
    let mut min_size = raw_min_size.maybe_resolve(parent_size, &resolve_calc_value).maybe_sub(box_sizing_adjustment);
    let mut max_size = raw_max_size
        .maybe_resolve(preferred_percentage_basis, &resolve_calc_value)
        .maybe_sub(box_sizing_adjustment)
        .maybe_max(min_size);

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

    // Intrinsic min/max constraints on replaced content use the size
    // transferred from a definite preferred opposite axis, rather than
    // falling back to the resource's natural dimension.
    if context.aspect_ratio.ratio.is_some() {
        let transferred_width = preferred_size
            .height
            .and_then(|height| content_width_from_height(height, context.aspect_ratio, padding_border_sum));
        let transferred_height = preferred_size
            .width
            .and_then(|width| content_height_from_width(width, context.aspect_ratio, padding_border_sum));
        if is_min_or_max_content(raw_min_size.width) {
            min_size.width = transferred_width;
        }
        if is_min_or_max_content(raw_min_size.height) {
            min_size.height = transferred_height;
        }
        if is_min_or_max_content(raw_max_size.width) {
            max_size.width = transferred_width;
        }
        if is_min_or_max_content(raw_max_size.height) {
            max_size.height = transferred_height;
        }
    }

    // A content-size probe ignores preferred and minimum constraints in the
    // requested axis. Opposite-axis constraints remain available for ratio
    // transfer.
    if sizing_mode == SizingMode::ContentSize {
        match requested_axis {
            RequestedAxis::Horizontal => {
                preferred_size.width = None;
                min_size.width = None;
            }
            RequestedAxis::Vertical => {
                preferred_size.height = None;
                min_size.height = None;
            }
            RequestedAxis::Both => {}
        }
    }

    if known_dimensions.width.is_some() || known_dimensions.height.is_some() {
        let style_max_size = raw_max_size
            .maybe_resolve(preferred_percentage_basis, &resolve_calc_value)
            .maybe_sub(box_sizing_adjustment)
            .maybe_max(min_size);
        let content_known = known_dimensions.maybe_sub(padding_border_sum);
        let transferred = complete_replaced_size(
            apply_aspect_ratio_to_content_size(
                content_known.maybe_clamp(min_size, style_max_size),
                context.aspect_ratio,
                padding_border_sum,
            ),
            natural_size,
        )
        .expect("known replaced dimensions or natural sizing determine both axes");
        let size = content_known.unwrap_or(transferred.maybe_clamp(min_size, style_max_size));
        return replaced_output(size.map(|value| value.max(0.0)) + padding_border_sum);
    }

    let unclamped = if preferred_size.width.is_some() || preferred_size.height.is_some() {
        complete_replaced_size(
            apply_aspect_ratio_to_content_size(preferred_size, context.aspect_ratio, padding_border_sum),
            natural_size,
        )
        .expect("preferred replaced dimensions or natural sizing determine both axes")
    } else if preferred_size_hint.width.is_some() || preferred_size_hint.height.is_some() {
        complete_replaced_size(
            apply_aspect_ratio_to_content_size(preferred_size_hint, context.aspect_ratio, padding_border_sum),
            natural_size,
        )
        .expect("preferred replaced hints or natural sizing determine both axes")
    } else {
        natural_size.unwrap_or_else(|| {
            ratio_only_stretch_size(available_space, context.writing_mode, context.aspect_ratio, padding_border_sum)
        })
    };
    let size = unclamped.map(|value| value.max(0.0));
    let width_violation = constraint_violation(size.width, min_size.width, max_size.width);
    let height_violation = constraint_violation(size.height, min_size.height, max_size.height);

    if context.aspect_ratio.ratio.is_none() {
        return replaced_output(size.maybe_clamp(min_size, max_size) + padding_border_sum);
    }

    let size = match (width_violation, height_violation) {
        (ConstraintViolation::None, ConstraintViolation::None) => size,
        (ConstraintViolation::Maximum, ConstraintViolation::None) => {
            let width = max_size.width.expect("maximum width violation has a bound");
            Size {
                width,
                height: content_height_from_width(width, context.aspect_ratio, padding_border_sum)
                    .expect("a resolved ratio transfers width to height")
                    .maybe_max(min_size.height),
            }
        }
        (ConstraintViolation::Minimum, ConstraintViolation::None) => {
            let width = min_size.width.expect("minimum width violation has a bound");
            Size {
                width,
                height: content_height_from_width(width, context.aspect_ratio, padding_border_sum)
                    .expect("a resolved ratio transfers width to height")
                    .maybe_min(max_size.height),
            }
        }
        (ConstraintViolation::None, ConstraintViolation::Maximum) => {
            let height = max_size.height.expect("maximum height violation has a bound");
            Size {
                width: content_width_from_height(height, context.aspect_ratio, padding_border_sum)
                    .expect("a resolved ratio transfers height to width")
                    .maybe_max(min_size.width),
                height,
            }
        }
        (ConstraintViolation::None, ConstraintViolation::Minimum) => {
            let height = min_size.height.expect("minimum height violation has a bound");
            Size {
                width: content_width_from_height(height, context.aspect_ratio, padding_border_sum)
                    .expect("a resolved ratio transfers height to width")
                    .maybe_min(max_size.width),
                height,
            }
        }
        (ConstraintViolation::Maximum, ConstraintViolation::Maximum) => {
            let width = max_size.width.expect("maximum width violation has a bound");
            let height = max_size.height.expect("maximum height violation has a bound");
            if ratio_basis_scale(width, size.width, padding_border_sum.width, context.aspect_ratio.box_sizing)
                <= ratio_basis_scale(height, size.height, padding_border_sum.height, context.aspect_ratio.box_sizing)
            {
                Size {
                    width,
                    height: content_height_from_width(width, context.aspect_ratio, padding_border_sum)
                        .expect("a resolved ratio transfers width to height")
                        .maybe_max(min_size.height),
                }
            } else {
                Size {
                    width: content_width_from_height(height, context.aspect_ratio, padding_border_sum)
                        .expect("a resolved ratio transfers height to width")
                        .maybe_max(min_size.width),
                    height,
                }
            }
        }
        (ConstraintViolation::Minimum, ConstraintViolation::Minimum) => {
            let width = min_size.width.expect("minimum width violation has a bound");
            let height = min_size.height.expect("minimum height violation has a bound");
            if ratio_basis_scale(width, size.width, padding_border_sum.width, context.aspect_ratio.box_sizing)
                <= ratio_basis_scale(height, size.height, padding_border_sum.height, context.aspect_ratio.box_sizing)
            {
                Size {
                    width: content_width_from_height(height, context.aspect_ratio, padding_border_sum)
                        .expect("a resolved ratio transfers height to width")
                        .maybe_min(max_size.width),
                    height,
                }
            } else {
                Size {
                    width,
                    height: content_height_from_width(width, context.aspect_ratio, padding_border_sum)
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
    };
    replaced_output(size + padding_border_sum)
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
) -> Option<Size<f32>> {
    if !aspect_ratio.ratio.is_some_and(|ratio| ratio.is_finite() && ratio > 0.0) {
        return Some(dimensions.unwrap_or(default_object_size));
    }

    let ratio_basis = if writing_mode.is_horizontal() {
        if let Some(width) = dimensions.width {
            Size { width: Some(width), height: None }
        } else {
            Size { width: None, height: Some(dimensions.height?) }
        }
    } else if let Some(height) = dimensions.height {
        Size { width: None, height: Some(height) }
    } else {
        Size { width: Some(dimensions.width?), height: None }
    };

    complete_replaced_size(apply_aspect_ratio_to_content_size(ratio_basis, aspect_ratio, padding_border), None)
}

/// Resolve the stretch-fit fallback for replaced content that has only a
/// preferred ratio. Replaced layout stretches in its logical inline axis and
/// derives the orthogonal axis through that ratio.
fn ratio_only_stretch_size(
    available_space: Size<AvailableSpace>,
    writing_mode: WritingMode,
    aspect_ratio: ResolvedAspectRatio,
    padding_border: Size<f32>,
) -> Size<f32> {
    let stretch_axis = |space: AvailableSpace, inset: f32| match space {
        AvailableSpace::Definite(size) => (size - inset).max(0.0),
        AvailableSpace::MinContent | AvailableSpace::MaxContent => 0.0,
    };
    let ratio_basis = if writing_mode.is_horizontal() {
        Size { width: Some(stretch_axis(available_space.width, padding_border.width)), height: None }
    } else {
        Size { width: None, height: Some(stretch_axis(available_space.height, padding_border.height)) }
    };
    complete_replaced_size(apply_aspect_ratio_to_content_size(ratio_basis, aspect_ratio, padding_border), None)
        .expect("a valid preferred ratio resolves the stretch-fit axis")
}

/// Fill missing candidate axes from a complete natural size, or return a
/// fully determined candidate when no fallback exists.
fn complete_replaced_size(size: Size<Option<f32>>, fallback: Option<Size<f32>>) -> Option<Size<f32>> {
    match fallback {
        Some(fallback) => Some(size.unwrap_or(fallback)),
        None => Some(Size { width: size.width?, height: size.height? }),
    }
}

/// Construct an output whose content extent is the atomic replaced box.
fn replaced_output(size: Size<f32>) -> LayoutOutput {
    LayoutOutput::from_sizes(size, size)
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

/// Whether a dimension names either intrinsic content-size extremum.
fn is_min_or_max_content(dimension: crate::Dimension) -> bool {
    dimension.is_min_content() || dimension.is_max_content()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dimension, Rect, Style};

    type TestStyle = Style<crate::sys::DefaultCheapStr>;

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
            Size::NONE,
        )
    }

    fn measure(style: &TestStyle) -> Size<f32> {
        compute_replaced_layout(
            inputs(Size::NONE),
            style,
            context(
                ResolvedAspectRatio {
                    ratio: style.aspect_ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0).or(Some(1.0)),
                    box_sizing: style.box_sizing,
                },
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
                context(ResolvedAspectRatio { ratio: None, box_sizing: style.box_sizing }, SizeContainment::NONE),
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
                context(ResolvedAspectRatio { ratio: None, box_sizing: BoxSizing::BorderBox }, containment),
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
                    ResolvedAspectRatio { ratio: None, box_sizing: BoxSizing::BorderBox },
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
                context(ResolvedAspectRatio { ratio: Some(2.0), box_sizing }, SizeContainment::NONE),
                |_, _| 0.0,
            )
            .size
        };

        assert_eq!(measure_with_ratio_box(BoxSizing::BorderBox), Size { width: 100.0, height: 50.0 });
        assert_eq!(measure_with_ratio_box(BoxSizing::ContentBox), Size { width: 100.0, height: 60.0 });
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
                ResolvedAspectRatio { ratio: Some(1.0), box_sizing: BoxSizing::BorderBox },
                SizeContainment::NONE,
                ReplacedNaturalSizing::new(
                    Size { width: Some(50.0), height: None },
                    Size { width: 300.0, height: 150.0 },
                ),
                Size::NONE,
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
                ResolvedAspectRatio { ratio: None, box_sizing: BoxSizing::ContentBox },
                SizeContainment::NONE,
                ReplacedNaturalSizing::new(
                    Size { width: Some(50.0), height: None },
                    Size { width: 300.0, height: 150.0 },
                ),
                Size::NONE,
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
                    ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::ContentBox },
                    SizeContainment::NONE,
                    ReplacedNaturalSizing::new(
                        Size { width: Some(50.0), height: Some(80.0) },
                        Size { width: 300.0, height: 150.0 },
                    ),
                    Size::NONE,
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
                    ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::ContentBox },
                    SizeContainment::NONE,
                    ReplacedNaturalSizing::new(Size::NONE, Size { width: 300.0, height: 150.0 }),
                    Size::NONE,
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
                ResolvedAspectRatio { ratio: None, box_sizing: BoxSizing::ContentBox },
                SizeContainment::NONE,
                ReplacedNaturalSizing::fixed(Size { width: 60.0, height: 60.0 }),
                Size::NONE,
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
                context(
                    ResolvedAspectRatio { ratio: Some(1.0), box_sizing: BoxSizing::BorderBox },
                    SizeContainment::NONE,
                ),
                |_, _| 0.0,
            )
            .size,
            Size { width: 50.0, height: 50.0 }
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
                context(
                    ResolvedAspectRatio { ratio: Some(2.0), box_sizing: BoxSizing::BorderBox },
                    SizeContainment::NONE,
                ),
                |_, _| 0.0,
            )
            .size,
            Size { width: 120.0, height: 60.0 }
        );
    }
}
