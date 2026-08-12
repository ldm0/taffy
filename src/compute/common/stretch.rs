//! Shared used-value resolution for the CSS `stretch` sizing keyword.

use crate::{Dimension, Size};

/// Preferred and limiting border-box sizes produced by `stretch`.
///
/// Unlike intrinsic keywords, `stretch` does not measure content. Its used
/// value is the border-box space left after margins in the selected physical
/// axis. With indefinite available size, preferred and maximum stretch remain
/// unresolved (auto/content and infinity), while minimum stretch becomes an
/// explicit zero floored by border and padding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct StretchSizeConstraints {
    /// Preferred border-box sizes resolved from `stretch`.
    pub preferred: Size<Option<f32>>,
    /// Minimum border-box sizes resolved from `stretch`.
    pub min: Size<Option<f32>>,
    /// Maximum border-box sizes resolved from `stretch`.
    pub max: Size<Option<f32>>,
}

/// Authored axes that use the CSS `stretch` sizing value.
///
/// Flex layout retains this provenance until its line cross-size is known;
/// other formatting contexts can resolve it immediately at their child
/// sizing boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StretchSizeProperties {
    /// Preferred axes authored as `stretch`.
    preferred: Size<bool>,
    /// Minimum axes authored as `stretch`.
    min: Size<bool>,
    /// Maximum axes authored as `stretch`.
    max: Size<bool>,
}

impl StretchSizeProperties {
    /// Capture stretch provenance before ordinary length resolution erases it.
    #[inline(always)]
    pub(crate) fn new(preferred: Size<Dimension>, min: Size<Dimension>, max: Size<Dimension>) -> Self {
        Self {
            preferred: preferred.map(Dimension::is_stretch),
            min: min.map(Dimension::is_stretch),
            max: max.map(Dimension::is_stretch),
        }
    }

    /// Resolve the captured properties against a possibly-definite
    /// margin-adjusted area.
    #[inline(always)]
    pub(crate) fn resolve(
        self,
        available_border_box_size: Size<Option<f32>>,
        padding_border: Size<f32>,
    ) -> StretchSizeConstraints {
        let resolve_preferred_or_maximum = |is_stretch: bool, available: Option<f32>, minimum: f32| {
            is_stretch.then(|| available.map(|size| size.max(minimum))).flatten()
        };
        let resolve_minimum = |is_stretch: bool, available: Option<f32>, minimum: f32| {
            is_stretch.then(|| available.unwrap_or(0.0).max(minimum))
        };
        let available = available_border_box_size.zip_map(padding_border, |size, minimum| (size, minimum));
        StretchSizeConstraints {
            preferred: self
                .preferred
                .zip_map(available, |value, (size, minimum)| resolve_preferred_or_maximum(value, size, minimum)),
            min: self.min.zip_map(available, |value, (size, minimum)| resolve_minimum(value, size, minimum)),
            max: self
                .max
                .zip_map(available, |value, (size, minimum)| resolve_preferred_or_maximum(value, size, minimum)),
        }
    }
}

/// Resolve physical `width`/`height` stretch properties.
///
/// The containing formatting context owns margin collapsing, flex-line, grid
/// area, and inset semantics, so callers provide the already-adjusted
/// border-box space. Preferred-ratio transfer and minimum-wins clamping remain
/// in the common size-constraint resolver.
#[inline(always)]
pub(crate) fn resolve_stretch_size_constraints(
    preferred: Size<Dimension>,
    min: Size<Dimension>,
    max: Size<Dimension>,
    available_border_box_size: Size<Option<f32>>,
    padding_border: Size<f32>,
) -> StretchSizeConstraints {
    StretchSizeProperties::new(preferred, min, max).resolve(available_border_box_size, padding_border)
}
