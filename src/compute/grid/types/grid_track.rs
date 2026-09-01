//! Contains GridTrack used to represent a single grid track (row/column) during layout
use crate::{
    compute::common::baseline::BaselineGroup,
    prelude::TaffyZero,
    style::{LengthPercentage, MaxTrackSizingFunction, MinTrackSizingFunction},
    util::sys::f32_min,
    CompactLength,
};

/// Whether a GridTrack represents an actual track or a gutter.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(in super::super) enum GridTrackKind {
    /// Track is an actual track
    Track,
    /// Track is a gutter (aka grid line) (aka gap)
    Gutter, // { name: Option<u16> },
}

/// Internal sizing information for a single grid track (row/column)
/// Gutters between tracks are sized similarly to actual tracks, so they
/// are also represented by this struct
#[derive(Debug, Clone)]
pub(in super::super) struct GridTrack {
    #[allow(dead_code)] // Used in tests + may be useful in future
    /// Whether the track is a full track, a gutter, or a placeholder that has not yet been initialised
    pub kind: GridTrackKind,

    /// Whether the track is a collapsed track/gutter. Collapsed tracks are effectively treated as if
    /// they don't exist for the purposes of grid sizing. Gutters between collapsed tracks are also collapsed.
    pub is_collapsed: bool,

    /// The minimum track sizing function of the track
    pub min_track_sizing_function: MinTrackSizingFunction,

    /// The maximum track sizing function of the track
    pub max_track_sizing_function: MaxTrackSizingFunction,

    /// The distance of the start of the track from the start of the grid container
    pub offset: f32,

    /// The size (width/height as applicable) of the track
    pub base_size: f32,

    /// Greatest baseline distance in the track's start-side sharing group.
    pub major_baseline: Option<f32>,

    /// Greatest baseline distance in the track's end-side sharing group.
    pub minor_baseline: Option<f32>,

    /// A temporary scratch value when sizing tracks
    /// Note: can be infinity
    pub growth_limit: f32,

    /// A temporary scratch value when sizing tracks. Is used as an additional amount to add to the
    /// estimate for the available space in the opposite axis when content sizing items
    pub content_alignment_adjustment: f32,

    /// A temporary scratch value when "distributing space" to avoid clobbering planned increase variable
    pub item_incurred_increase: f32,
    /// A temporary scratch value when "distributing space" to avoid clobbering the main variable
    pub base_size_planned_increase: f32,
    /// A temporary scratch value when "distributing space" to avoid clobbering the main variable
    pub growth_limit_planned_increase: f32,
    /// A temporary scratch value when "distributing space"
    /// See: https://www.w3.org/TR/css3-grid-layout/#infinitely-growable
    pub infinitely_growable: bool,
}

impl GridTrack {
    /// GridTrack constructor with all configuration parameters for the other constructors exposed
    const fn new_with_kind(
        kind: GridTrackKind,
        min_track_sizing_function: MinTrackSizingFunction,
        max_track_sizing_function: MaxTrackSizingFunction,
    ) -> GridTrack {
        GridTrack {
            kind,
            is_collapsed: false,
            min_track_sizing_function,
            max_track_sizing_function,
            offset: 0.0,
            base_size: 0.0,
            major_baseline: None,
            minor_baseline: None,
            growth_limit: 0.0,
            content_alignment_adjustment: 0.0,
            item_incurred_increase: 0.0,
            base_size_planned_increase: 0.0,
            growth_limit_planned_increase: 0.0,
            infinitely_growable: false,
        }
    }

    /// Create new GridTrack representing an actual track (not a gutter)
    pub const fn new(
        min_track_sizing_function: MinTrackSizingFunction,
        max_track_sizing_function: MaxTrackSizingFunction,
    ) -> GridTrack {
        Self::new_with_kind(GridTrackKind::Track, min_track_sizing_function, max_track_sizing_function)
    }

    /// Create a new GridTrack representing a gutter
    pub fn gutter(size: LengthPercentage) -> GridTrack {
        Self::new_with_kind(
            GridTrackKind::Gutter,
            MinTrackSizingFunction::from(size),
            MaxTrackSizingFunction::from(size),
        )
    }

    /// Return the percentage basis used while initializing this track.
    ///
    /// Grid gaps resolve cyclic percentages against zero during intrinsic
    /// sizing. Giving gutters a zero basis preserves the length component of
    /// `calc(<length> + <percentage>)`, while ordinary percentage tracks stay
    /// indefinite and are therefore treated as `auto`.
    #[inline(always)]
    pub fn initial_percentage_basis(&self, axis_inner_node_size: Option<f32>) -> Option<f32> {
        match self.kind {
            GridTrackKind::Gutter => Some(axis_inner_node_size.unwrap_or(0.0)),
            GridTrackKind::Track => axis_inner_node_size,
        }
    }

    /// Mark a GridTrack as collapsed. Also sets both of the track's sizing functions
    /// to fixed zero-sized sizing functions.
    pub fn collapse(&mut self) {
        self.is_collapsed = true;
        self.min_track_sizing_function = MinTrackSizingFunction::ZERO;
        self.max_track_sizing_function = MaxTrackSizingFunction::ZERO;
    }

    /// Clear baseline-sharing metrics before recomputing an axis.
    pub fn reset_baselines(&mut self) {
        self.major_baseline = None;
        self.minor_baseline = None;
    }

    /// Store the greatest baseline distance observed for `group`.
    pub fn set_baseline(&mut self, group: BaselineGroup, baseline: f32) {
        let slot = match group {
            BaselineGroup::Major => &mut self.major_baseline,
            BaselineGroup::Minor => &mut self.minor_baseline,
        };
        *slot = Some(slot.map_or(baseline, |current| current.max(baseline)));
    }

    /// Return the shared baseline distance for `group`.
    pub fn baseline(&self, group: BaselineGroup) -> Option<f32> {
        match group {
            BaselineGroup::Major => self.major_baseline,
            BaselineGroup::Minor => self.minor_baseline,
        }
    }

    #[inline(always)]
    /// Returns true if the track is flexible (has a Flex MaxTrackSizingFunction), else false.
    pub fn is_flexible(&self) -> bool {
        self.max_track_sizing_function.is_fr()
    }

    #[inline(always)]
    /// Returns true if either track sizing function uses a percentage.
    pub fn uses_percentage(&self) -> bool {
        self.min_track_sizing_function.uses_percentage() || self.max_track_sizing_function.uses_percentage()
    }

    /// Whether resolving the grid container's used size can change this
    /// track's result.
    ///
    /// Percentage tracks acquire a percentage basis and flexible tracks rerun
    /// the fr expansion step against the now-definite free space. This mirrors
    /// Blink's `kIsDependentOnAvailableSize` track-collection property.
    #[inline(always)]
    pub fn depends_on_available_size(&self) -> bool {
        self.uses_percentage() || self.is_flexible()
    }

    #[inline(always)]
    /// Returns true if the track has an intrinsic min and or max sizing function
    pub fn has_intrinsic_sizing_function(&self) -> bool {
        self.min_track_sizing_function.is_intrinsic() || self.max_track_sizing_function.is_intrinsic()
    }

    #[inline]
    /// Resolve the `fit-content()` cap for this track, or infinity when the
    /// track is not fit-content limited.
    pub fn fit_content_limit(&self, axis_available_grid_space: Option<f32>) -> f32 {
        match self.max_track_sizing_function.0.tag() {
            CompactLength::FIT_CONTENT_PX_TAG => self.max_track_sizing_function.0.value(),
            CompactLength::FIT_CONTENT_PERCENT_TAG => match axis_available_grid_space {
                Some(space) => space * self.max_track_sizing_function.0.value(),
                None => f32::INFINITY,
            },
            _ => f32::INFINITY,
        }
    }

    #[inline]
    /// Clamp the current growth limit to the resolved `fit-content()` cap.
    pub fn fit_content_limited_growth_limit(&self, axis_available_grid_space: Option<f32>) -> f32 {
        f32_min(self.growth_limit, self.fit_content_limit(axis_available_grid_space))
    }

    /// Return the finite value used when distributing intrinsic contributions
    /// to a growth limit. An indefinite growth limit starts at the track's
    /// base size and can grow from there.
    #[inline]
    pub fn definite_growth_limit(&self) -> f32 {
        if self.growth_limit == f32::INFINITY {
            self.base_size
        } else {
            self.growth_limit
        }
    }

    #[inline]
    /// Returns the track's flex factor if it is a flex track, else 0.
    pub fn flex_factor(&self) -> f32 {
        if self.max_track_sizing_function.is_fr() {
            self.max_track_sizing_function.0.value()
        } else {
            0.0
        }
    }
}

#[cfg(all(test, feature = "calc"))]
mod tests {
    use super::*;

    #[repr(align(8))]
    struct CalcToken;

    static CALC_TOKEN: CalcToken = CalcToken;

    fn resolve_gap_calc(_: *const (), percentage_basis: f32) -> f32 {
        20.0 + (0.05 * percentage_basis)
    }

    #[test]
    fn indefinite_gutter_preserves_the_length_part_of_a_calc_gap() {
        let calc = LengthPercentage::calc((&CALC_TOKEN as *const CalcToken).cast());
        let gutter = GridTrack::gutter(calc);
        let track = GridTrack::new(MinTrackSizingFunction::from(calc), MaxTrackSizingFunction::from(calc));

        let gutter_basis = gutter.initial_percentage_basis(None);
        assert_eq!(gutter_basis, Some(0.0));
        assert_eq!(gutter.min_track_sizing_function.definite_value(gutter_basis, resolve_gap_calc), Some(20.0));

        let track_basis = track.initial_percentage_basis(None);
        assert_eq!(track_basis, None);
        assert_eq!(track.min_track_sizing_function.definite_value(track_basis, resolve_gap_calc), None);

        let definite_basis = gutter.initial_percentage_basis(Some(200.0));
        assert_eq!(definite_basis, Some(200.0));
        assert_eq!(gutter.min_track_sizing_function.definite_value(definite_basis, resolve_gap_calc), Some(30.0));
    }
}
