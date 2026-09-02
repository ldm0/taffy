//! A cache for storing the results of layout computation

use crate::geometry::{Line, LogicalSize, Rect, Size, WritingMode};
use crate::style::AvailableSpace;
use crate::tree::{
    AutoSizeBehavior, IntrinsicSizeResult, LayoutEnvironment, LayoutInput, LayoutOutput, OrthogonalFallback, RunMode,
    SizingMode, SizingPurpose,
};
use crate::RequestedAxis;

/// The number of cache entries for each node in the tree
const CACHE_SIZE: usize = 9;
/// Number of additional entries retained for measurements whose intrinsic
/// inline size depends on a definite block constraint. Grid sizing commonly
/// probes several such constraints in one pass.
const BLOCK_CONSTRAINT_CACHE_SIZE: usize = 8;

/// Lossless, equality-comparable representation of one available-space axis.
///
/// Keeping the enum tag separate from the float bits prevents an intrinsic
/// constraint from aliasing a numerically non-finite definite constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
enum AvailableSpaceCacheKey {
    /// A numeric constraint, retained by exact IEEE-754 bits.
    Definite(u32),
    /// An intrinsic minimum-content constraint.
    MinContent,
    /// An intrinsic maximum-content constraint.
    MaxContent,
}

/// Convert an optional float to a tagged, exact-bit cache component.
#[inline(always)]
fn option_cache_key(input: Option<f32>) -> Option<u32> {
    input.map(f32::to_bits)
}

/// Convert a logical optional size to exact-bit cache components.
#[inline(always)]
fn logical_size_option_cache_key(input: LogicalSize<Option<f32>>) -> LogicalSize<Option<u32>> {
    LogicalSize { inline_size: option_cache_key(input.inline_size), block_size: option_cache_key(input.block_size) }
}

/// Convert a physical optional size to exact-bit cache components.
#[inline(always)]
fn size_option_cache_key(input: Size<Option<f32>>) -> Size<Option<u32>> {
    Size { width: option_cache_key(input.width), height: option_cache_key(input.height) }
}

/// Convert one available-space axis without conflating its enum variants.
#[inline(always)]
fn available_space_cache_key(input: AvailableSpace) -> AvailableSpaceCacheKey {
    match input {
        AvailableSpace::Definite(value) => AvailableSpaceCacheKey::Definite(value.to_bits()),
        AvailableSpace::MinContent => AvailableSpaceCacheKey::MinContent,
        AvailableSpace::MaxContent => AvailableSpaceCacheKey::MaxContent,
    }
}

/// Convert both physical available-space axes to tagged cache components.
#[inline(always)]
fn size_available_space_cache_key(input: Size<AvailableSpace>) -> Size<AvailableSpaceCacheKey> {
    Size { width: available_space_cache_key(input.width), height: available_space_cache_key(input.height) }
}

/// Complete, lossless constraint-space key for one layout operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
struct CacheKey {
    /// Dimensions fixed by the parent formatting context.
    known_dimensions: Size<Option<u32>>,
    /// Space offered by the parent, even on axes with known dimensions.
    available_space: Size<AvailableSpaceCacheKey>,
    /// Dimensions whose source is definite for descendant percentages.
    definite_dimensions: Size<Option<u32>>,
    /// The containing block size in its own logical axes.
    logical_parent_size: LogicalSize<Option<u32>>,
    /// Physical initial containing block inherited by descendants.
    initial_containing_block_size: Size<Option<u32>>,
    /// Physical axis requested by an intrinsic sizing operation.
    requested_axis: RequestedAxis,
    /// Writing mode that owns the containing block size.
    parent_writing_mode: WritingMode,
    /// Whether inherent size styles participate in this computation.
    sizing_mode: SizingMode,
    /// Whether this result is final layout or an intrinsic contribution.
    sizing_purpose: SizingPurpose,
    /// How an authored logical inline-size auto resolves in this space.
    inline_auto_behavior: AutoSizeBehavior,
    /// How an authored logical block-size auto resolves in this space.
    block_auto_behavior: AutoSizeBehavior,
    /// Whether this boundary permits the orthogonal viewport fallback.
    orthogonal_fallback: OrthogonalFallback,
    /// Physical margin sides omitted only from explicit stretch sizing.
    ignored_margins_for_stretch: Rect<bool>,
    /// Physical block-axis margin collapse permissions at this boundary.
    vertical_margins_are_collapsible: Line<bool>,
}

impl CacheKey {
    /// Construct a cache key for one node input and layout-pass environment.
    #[inline(always)]
    fn new(input: &LayoutInput, environment: LayoutEnvironment) -> Self {
        Self {
            known_dimensions: size_option_cache_key(input.known_dimensions),
            available_space: size_available_space_cache_key(input.available_space),
            definite_dimensions: size_option_cache_key(input.definite_dimensions),
            logical_parent_size: logical_size_option_cache_key(input.parent_writing_mode.to_logical(input.parent_size)),
            initial_containing_block_size: size_option_cache_key(environment.initial_containing_block_size),
            requested_axis: input.axis,
            parent_writing_mode: input.parent_writing_mode,
            sizing_mode: input.sizing_mode,
            sizing_purpose: input.sizing_purpose,
            inline_auto_behavior: input.inline_auto_behavior,
            block_auto_behavior: input.block_auto_behavior,
            orthogonal_fallback: input.orthogonal_fallback,
            ignored_margins_for_stretch: input.ignored_margins_for_stretch,
            vertical_margins_are_collapsible: input.vertical_margins_are_collapsible,
        }
    }

    /// Compare the observable inputs of a block-independent intrinsic result.
    ///
    /// Only the containing block's logical block-size may be omitted. The
    /// result must still match every other constraint-space component,
    /// including the node's own definite dimensions and available-space kind.
    #[inline(always)]
    fn matches_block_independent_measurement(self, other: Self) -> bool {
        self.known_dimensions == other.known_dimensions
            && self.available_space == other.available_space
            && self.definite_dimensions == other.definite_dimensions
            && self.logical_parent_size.inline_size == other.logical_parent_size.inline_size
            && self.initial_containing_block_size == other.initial_containing_block_size
            && self.requested_axis == other.requested_axis
            && self.parent_writing_mode == other.parent_writing_mode
            && self.sizing_mode == other.sizing_mode
            && self.sizing_purpose == other.sizing_purpose
            && self.inline_auto_behavior == other.inline_auto_behavior
            && self.block_auto_behavior == other.block_auto_behavior
            && self.orthogonal_fallback == other.orthogonal_fallback
            && self.ignored_margins_for_stretch == other.ignored_margins_for_stretch
            && self.vertical_margins_are_collapsible == other.vertical_margins_are_collapsible
    }
}

impl From<&LayoutInput> for CacheKey {
    fn from(input: &LayoutInput) -> Self {
        Self::new(input, LayoutEnvironment::NONE)
    }
}

/// Cached intermediate layout results
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub(crate) struct CacheEntry<T> {
    /// The key for the cache entry
    key: CacheKey,
    /// The cached size and baselines of the item
    content: T,
}

/// The subset of a measurement result retained by the size cache.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
struct CachedMeasurement {
    /// Measured outer size.
    size: Size<f32>,
    /// Whether the result must be keyed by parent block-size.
    depends_on_block_constraints: bool,
    /// Whether this probe obtained its inline contribution by applying the
    /// preferred aspect ratio.
    applied_aspect_ratio: bool,
}

/// A cache for caching the results of a sizing a Grid Item or Flexbox Item
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Cache {
    /// The cache entry for the node's final layout
    final_layout_entry: Option<CacheEntry<LayoutOutput>>,
    /// The cache entries for the node's preliminary size measurements
    measure_entries: [Option<CacheEntry<CachedMeasurement>>; CACHE_SIZE],
    /// Bounded cache for measurements that must be keyed by parent block-size.
    block_constraint_entries: [Option<CacheEntry<CachedMeasurement>>; BLOCK_CONSTRAINT_CACHE_SIZE],
    /// Next entry replaced when the block-constraint cache is full.
    next_block_constraint_entry: u8,
    /// Tracks if all cache entries are empty
    is_empty: bool,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache {
    /// Create a new empty cache
    pub const fn new() -> Self {
        Self {
            final_layout_entry: None,
            measure_entries: [None; CACHE_SIZE],
            block_constraint_entries: [None; BLOCK_CONSTRAINT_CACHE_SIZE],
            next_block_constraint_entry: 0,
            is_empty: true,
        }
    }

    /// Return the cache slot to cache the current computed result in
    ///
    /// ## Caching Strategy
    ///
    /// We need multiple cache slots, because a node's size is often queried by it's parent multiple times in the course of the layout
    /// process, and we don't want later results to clobber earlier ones.
    ///
    /// The two variables that we care about when determining cache slot are:
    ///
    ///   - How many "known_dimensions" are set. In the worst case, a node may be called first with neither dimension known, then with one
    ///     dimension known (either width of height - which doesn't matter for our purposes here), and then with both dimensions known.
    ///   - Whether unknown dimensions are being sized under a min-content or a max-content available space constraint (definite available space
    ///     shares a cache slot with max-content because a node will generally be sized under one or the other but not both).
    ///
    /// ## Cache slots:
    ///
    /// - Slot 0: Both known_dimensions were set
    /// - Slots 1-4: 1 of 2 known_dimensions were set and:
    ///   - Slot 1: width but not height known_dimension was set and the other dimension was either a MaxContent or Definite available space constraintraint
    ///   - Slot 2: width but not height known_dimension was set and the other dimension was a MinContent constraint
    ///   - Slot 3: height but not width known_dimension was set and the other dimension was either a MaxContent or Definite available space constraintable space constraint
    ///   - Slot 4: height but not width known_dimension was set and the other dimension was a MinContent constraint
    /// - Slots 5-8: Neither known_dimensions were set and:
    ///   - Slot 5: x-axis available space is MaxContent or Definite and y-axis available space is MaxContent or Definite
    ///   - Slot 6: x-axis available space is MaxContent or Definite and y-axis available space is MinContent
    ///   - Slot 7: x-axis available space is MinContent and y-axis available space is MaxContent or Definite
    ///   - Slot 8: x-axis available space is MinContent and y-axis available space is MinContent
    ///
    /// Results that report a block-constraint dependency bypass these nine
    /// shape slots and use a separate bounded cache keyed by the full parent
    /// size. Independent results remain reusable across parent block-sizes.
    #[inline]
    fn compute_cache_slot(known_dimensions: Size<Option<f32>>, available_space: Size<AvailableSpace>) -> usize {
        use AvailableSpace::{Definite, MaxContent, MinContent};

        let has_known_width = known_dimensions.width.is_some();
        let has_known_height = known_dimensions.height.is_some();

        // Slot 0: Both known_dimensions were set
        if has_known_width && has_known_height {
            return 0;
        }

        // Slot 1: width but not height known_dimension was set and the other dimension was either a MaxContent or Definite available space constraint
        // Slot 2: width but not height known_dimension was set and the other dimension was a MinContent constraint
        if has_known_width && !has_known_height {
            return 1 + (available_space.height == MinContent) as usize;
        }

        // Slot 3: height but not width known_dimension was set and the other dimension was either a MaxContent or Definite available space constraint
        // Slot 4: height but not width known_dimension was set and the other dimension was a MinContent constraint
        if has_known_height && !has_known_width {
            return 3 + (available_space.width == MinContent) as usize;
        }

        // Slots 5-8: Neither known_dimensions were set and:
        match (available_space.width, available_space.height) {
            // Slot 5: x-axis available space is MaxContent or Definite and y-axis available space is MaxContent or Definite
            (MaxContent | Definite(_), MaxContent | Definite(_)) => 5,
            // Slot 6: x-axis available space is MaxContent or Definite and y-axis available space is MinContent
            (MaxContent | Definite(_), MinContent) => 6,
            // Slot 7: x-axis available space is MinContent and y-axis available space is MaxContent or Definite
            (MinContent, MaxContent | Definite(_)) => 7,
            // Slot 8: x-axis available space is MinContent and y-axis available space is MinContent
            (MinContent, MinContent) => 8,
        }
    }

    /// Try to retrieve a cached result from the cache
    #[inline]
    pub fn get(&self, input: &LayoutInput) -> Option<LayoutOutput> {
        self.get_with_environment(input, LayoutEnvironment::NONE)
    }

    /// Try to retrieve a cached result for a specific layout-pass environment.
    #[inline]
    pub fn get_with_environment(&self, input: &LayoutInput, environment: LayoutEnvironment) -> Option<LayoutOutput> {
        let key = CacheKey::new(input, environment);
        match input.run_mode {
            RunMode::PerformLayout => self.final_layout_entry.filter(|entry| entry.key == key).map(|e| e.content),
            RunMode::ComputeSize => {
                self.get_size_with_environment(input, environment).map(LayoutOutput::from_intrinsic_size_result)
            }
            RunMode::PerformHiddenLayout => None,
        }
    }

    /// Try to retrieve a dedicated intrinsic size result from the measurement
    /// caches.
    #[inline]
    pub fn get_size(&self, input: &LayoutInput) -> Option<IntrinsicSizeResult> {
        self.get_size_with_environment(input, LayoutEnvironment::NONE)
    }

    /// Try to retrieve an intrinsic result for a specific layout-pass environment.
    #[inline]
    pub fn get_size_with_environment(
        &self,
        input: &LayoutInput,
        environment: LayoutEnvironment,
    ) -> Option<IntrinsicSizeResult> {
        debug_assert_eq!(input.run_mode, RunMode::ComputeSize);
        let key = CacheKey::new(input, environment);
        for entry in self.measure_entries.iter().flatten() {
            if entry.key.matches_block_independent_measurement(key) {
                return Some(IntrinsicSizeResult {
                    size: entry.content.size,
                    depends_on_block_constraints: entry.content.depends_on_block_constraints,
                    applied_aspect_ratio: entry.content.applied_aspect_ratio,
                });
            }
        }

        for entry in self.block_constraint_entries.iter().flatten() {
            if entry.key == key {
                return Some(IntrinsicSizeResult {
                    size: entry.content.size,
                    depends_on_block_constraints: entry.content.depends_on_block_constraints,
                    applied_aspect_ratio: entry.content.applied_aspect_ratio,
                });
            }
        }

        None
    }

    /// Store a computed size in the cache
    pub fn store(&mut self, input: &LayoutInput, layout_output: LayoutOutput) {
        self.store_with_environment(input, layout_output, LayoutEnvironment::NONE);
    }

    /// Store a computed result for a specific layout-pass environment.
    pub fn store_with_environment(
        &mut self,
        input: &LayoutInput,
        layout_output: LayoutOutput,
        environment: LayoutEnvironment,
    ) {
        let key = CacheKey::new(input, environment);
        match input.run_mode {
            RunMode::PerformLayout => {
                self.is_empty = false;
                self.final_layout_entry = Some(CacheEntry { key, content: layout_output })
            }
            RunMode::ComputeSize => {
                self.store_size_with_environment(input, layout_output.into_intrinsic_size_result(), environment);
            }
            RunMode::PerformHiddenLayout => {}
        }
    }

    /// Store a dedicated intrinsic size result in the appropriate measurement
    /// cache.
    pub fn store_size(&mut self, input: &LayoutInput, result: IntrinsicSizeResult) {
        self.store_size_with_environment(input, result, LayoutEnvironment::NONE);
    }

    /// Store an intrinsic result for a specific layout-pass environment.
    pub fn store_size_with_environment(
        &mut self,
        input: &LayoutInput,
        result: IntrinsicSizeResult,
        environment: LayoutEnvironment,
    ) {
        debug_assert_eq!(input.run_mode, RunMode::ComputeSize);
        self.is_empty = false;
        let key = CacheKey::new(input, environment);
        let entry = CacheEntry {
            key,
            content: CachedMeasurement {
                size: result.size,
                depends_on_block_constraints: result.depends_on_block_constraints,
                applied_aspect_ratio: result.applied_aspect_ratio,
            },
        };
        if result.depends_on_block_constraints {
            if let Some(existing_index) = self.block_constraint_entries.iter().position(|existing| match existing {
                Some(existing) => existing.key == key,
                None => false,
            }) {
                self.block_constraint_entries[existing_index] = Some(entry);
                return;
            }
            let cache_slot = self
                .block_constraint_entries
                .iter()
                .position(Option::is_none)
                .unwrap_or(self.next_block_constraint_entry as usize);
            self.block_constraint_entries[cache_slot] = Some(entry);
            self.next_block_constraint_entry = ((cache_slot + 1) % BLOCK_CONSTRAINT_CACHE_SIZE) as u8;
        } else {
            let cache_slot = Self::compute_cache_slot(input.known_dimensions, input.available_space);
            self.measure_entries[cache_slot] = Some(entry);
        }
    }

    /// Clear all cache entries and reports clear operation outcome ([`ClearState`])
    pub fn clear(&mut self) -> ClearState {
        if self.is_empty {
            return ClearState::AlreadyEmpty;
        }
        self.is_empty = true;
        self.final_layout_entry = None;
        self.measure_entries = [None; CACHE_SIZE];
        self.block_constraint_entries = [None; BLOCK_CONSTRAINT_CACHE_SIZE];
        self.next_block_constraint_entry = 0;
        ClearState::Cleared
    }

    /// Returns true if all cache entries are None, else false
    pub fn is_empty(&self) -> bool {
        self.final_layout_entry.is_none()
            && !self.measure_entries.iter().any(|entry| entry.is_some())
            && !self.block_constraint_entries.iter().any(|entry| entry.is_some())
    }
}

/// Clear operation outcome. See [`Cache::clear`]
pub enum ClearState {
    /// Cleared some values
    Cleared,
    /// Everything was already cleared
    AlreadyEmpty,
}

#[cfg(test)]
mod tests {
    use super::Cache;
    use crate::geometry::{Line, Rect, Size, WritingMode};
    use crate::style::AvailableSpace;
    use crate::tree::{
        AutoSizeBehavior, IntrinsicSizeResult, LayoutEnvironment, LayoutInput, LayoutOutput, OrthogonalFallback,
        RequestedAxis, RunMode, SizingMode, SizingPurpose,
    };

    fn input(sizing_purpose: SizingPurpose) -> LayoutInput {
        LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            sizing_purpose,
            axis: RequestedAxis::Horizontal,
            inline_auto_behavior: AutoSizeBehavior::FitContent,
            block_auto_behavior: AutoSizeBehavior::FitContent,
            orthogonal_fallback: OrthogonalFallback::UseInitialContainingBlock,
            known_dimensions: Size::NONE,
            definite_dimensions: Size::NONE,
            parent_size: Size::NONE,
            parent_writing_mode: WritingMode::HorizontalTb,
            available_space: Size { width: AvailableSpace::Definite(100.0), height: AvailableSpace::MaxContent },
            ignored_margins_for_stretch: Rect::default(),
            vertical_margins_are_collapsible: Line::FALSE,
        }
    }

    #[test]
    fn intrinsic_contributions_do_not_alias_layout_measurements() {
        let mut cache = Cache::new();
        let contribution = input(SizingPurpose::IntrinsicContribution);
        let layout = input(SizingPurpose::Layout);
        cache.store(&contribution, LayoutOutput::from_outer_size(Size { width: 60.0, height: 20.0 }));

        assert!(cache.get(&layout).is_none());
        assert_eq!(cache.get(&contribution).unwrap().size.width, 60.0);
    }

    #[test]
    fn intrinsic_cache_preserves_operation_provenance() {
        let mut cache = Cache::new();
        let input = input(SizingPurpose::IntrinsicContribution);
        let expected = IntrinsicSizeResult {
            size: Size { width: 60.0, height: 30.0 },
            depends_on_block_constraints: false,
            applied_aspect_ratio: true,
        };

        cache.store_size(&input, expected);

        assert_eq!(cache.get_size(&input), Some(expected));
    }

    #[test]
    fn intrinsic_measurements_distinguish_layout_view_sizes() {
        let mut cache = Cache::new();
        let input = input(SizingPurpose::IntrinsicContribution);
        let initial =
            LayoutEnvironment { initial_containing_block_size: Size { width: Some(800.0), height: Some(600.0) } };
        cache.store_with_environment(
            &input,
            LayoutOutput::from_outer_size(Size { width: 80.0, height: 600.0 }),
            initial,
        );

        let resized =
            LayoutEnvironment { initial_containing_block_size: Size { width: Some(800.0), height: Some(400.0) } };
        assert!(cache.get_with_environment(&input, resized).is_none());
        assert_eq!(cache.get_with_environment(&input, initial).unwrap().size.height, 600.0);
    }

    #[test]
    fn intrinsic_measurements_distinguish_inline_auto_behavior() {
        let mut cache = Cache::new();
        let fit_content = input(SizingPurpose::IntrinsicContribution);
        cache.store(&fit_content, LayoutOutput::from_outer_size(Size { width: 60.0, height: 25.0 }));

        let stretch = LayoutInput { inline_auto_behavior: AutoSizeBehavior::StretchImplicit, ..fit_content };
        assert!(cache.get(&stretch).is_none());
        assert_eq!(cache.get(&fit_content).unwrap().size.width, 60.0);
    }

    #[test]
    fn orthogonal_fallback_policies_do_not_alias() {
        let mut cache = Cache::new();
        let fallback = input(SizingPurpose::IntrinsicContribution);
        cache.store(&fallback, LayoutOutput::from_outer_size(Size { width: 60.0, height: 25.0 }));

        let suppressed = LayoutInput { orthogonal_fallback: OrthogonalFallback::Suppress, ..fallback };
        assert!(cache.get(&suppressed).is_none());
        assert_eq!(cache.get(&fallback).unwrap().size.width, 60.0);
    }

    #[test]
    fn intrinsic_measurements_distinguish_definite_dimensions() {
        let mut cache = Cache::new();
        let indefinite = LayoutInput {
            known_dimensions: Size { width: None, height: Some(100.0) },
            ..input(SizingPurpose::IntrinsicContribution)
        };
        cache.store(&indefinite, LayoutOutput::from_outer_size(Size { width: 40.0, height: 100.0 }));

        let definite = LayoutInput { definite_dimensions: Size { width: None, height: Some(100.0) }, ..indefinite };

        assert!(cache.get(&definite).is_none());
        assert_eq!(cache.get(&indefinite).unwrap().size.width, 40.0);
    }

    #[test]
    fn known_dimensions_do_not_hide_available_space() {
        let mut cache = Cache::new();
        let min_content = LayoutInput {
            known_dimensions: Size { width: Some(60.0), height: None },
            available_space: Size { width: AvailableSpace::MinContent, height: AvailableSpace::MaxContent },
            ..input(SizingPurpose::IntrinsicContribution)
        };
        cache.store(&min_content, LayoutOutput::from_outer_size(Size { width: 60.0, height: 20.0 }));

        let max_content = LayoutInput {
            available_space: Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent },
            ..min_content
        };

        assert!(cache.get(&max_content).is_none());
        assert_eq!(cache.get(&min_content).unwrap().size.width, 60.0);
    }

    #[test]
    fn intrinsic_available_space_does_not_alias_numeric_infinity() {
        let mut cache = Cache::new();
        let max_content = LayoutInput {
            available_space: Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent },
            ..input(SizingPurpose::IntrinsicContribution)
        };
        cache.store(&max_content, LayoutOutput::from_outer_size(Size { width: 60.0, height: 20.0 }));

        let definite_infinity = LayoutInput {
            available_space: Size {
                width: AvailableSpace::Definite(f32::INFINITY),
                height: AvailableSpace::MaxContent,
            },
            ..max_content
        };

        assert!(cache.get(&definite_infinity).is_none());
        assert_eq!(cache.get(&max_content).unwrap().size.width, 60.0);
    }

    #[test]
    fn intrinsic_measurements_distinguish_margin_collapse_constraints() {
        let mut cache = Cache::new();
        let non_collapsing = input(SizingPurpose::IntrinsicContribution);
        cache.store(&non_collapsing, LayoutOutput::from_outer_size(Size { width: 60.0, height: 20.0 }));

        let collapsing = LayoutInput { vertical_margins_are_collapsible: Line::TRUE, ..non_collapsing };

        assert!(cache.get(&collapsing).is_none());
        assert_eq!(cache.get(&non_collapsing).unwrap().size.height, 20.0);
    }

    #[test]
    fn intrinsic_measurements_distinguish_stretch_margin_masks() {
        let mut cache = Cache::new();
        let accounting_for_all_margins = input(SizingPurpose::IntrinsicContribution);
        cache.store(&accounting_for_all_margins, LayoutOutput::from_outer_size(Size { width: 60.0, height: 80.0 }));

        let ignoring_block_start = LayoutInput {
            ignored_margins_for_stretch: Rect { left: false, right: false, top: true, bottom: false },
            ..accounting_for_all_margins
        };

        assert!(cache.get(&ignoring_block_start).is_none());
        assert_eq!(cache.get(&accounting_for_all_margins).unwrap().size.height, 80.0);
    }

    #[test]
    fn intrinsic_measurements_distinguish_parent_block_constraints() {
        let mut cache = Cache::new();
        let mut initial = input(SizingPurpose::IntrinsicContribution);
        initial.parent_size = Size { width: Some(200.0), height: Some(100.0) };
        cache.store(
            &initial,
            LayoutOutput::from_outer_size(Size { width: 100.0, height: 100.0 }).with_block_constraint_dependency(true),
        );

        let mut changed_block_constraint = initial;
        changed_block_constraint.parent_size.height = Some(50.0);

        assert!(cache.get(&changed_block_constraint).is_none());
    }

    #[test]
    fn independent_measurements_ignore_parent_block_constraints() {
        let mut cache = Cache::new();
        let mut initial = input(SizingPurpose::IntrinsicContribution);
        initial.parent_size = Size { width: Some(200.0), height: Some(100.0) };
        cache.store(&initial, LayoutOutput::from_outer_size(Size { width: 80.0, height: 20.0 }));

        let mut changed_block_constraint = initial;
        changed_block_constraint.parent_size.height = Some(50.0);

        assert_eq!(cache.get(&changed_block_constraint).unwrap().size.width, 80.0);
    }

    #[test]
    fn dependent_measurements_retain_multiple_block_constraints() {
        let mut cache = Cache::new();
        let mut input = input(SizingPurpose::IntrinsicContribution);
        input.parent_size.width = Some(200.0);

        for height in [100.0, 50.0] {
            input.parent_size.height = Some(height);
            cache.store(
                &input,
                LayoutOutput::from_outer_size(Size { width: height, height }).with_block_constraint_dependency(true),
            );
        }

        input.parent_size.height = Some(100.0);
        assert_eq!(cache.get(&input).unwrap().size.width, 100.0);
        input.parent_size.height = Some(50.0);
        assert_eq!(cache.get(&input).unwrap().size.width, 50.0);
    }

    #[test]
    fn vertical_measurements_key_the_parent_inline_axis_in_logical_space() {
        let mut cache = Cache::new();
        let mut initial = input(SizingPurpose::IntrinsicContribution);
        initial.parent_writing_mode = WritingMode::VerticalRl;
        initial.parent_size = Size { width: Some(100.0), height: Some(200.0) };
        cache.store(&initial, LayoutOutput::from_outer_size(Size { width: 80.0, height: 20.0 }));

        let mut changed_block_constraint = initial;
        changed_block_constraint.parent_size.width = Some(50.0);
        assert_eq!(cache.get(&changed_block_constraint).unwrap().size.width, 80.0);

        let mut changed_inline_constraint = initial;
        changed_inline_constraint.parent_size.height = Some(150.0);
        assert!(cache.get(&changed_inline_constraint).is_none());
    }

    #[test]
    fn measurements_from_different_parent_writing_modes_do_not_alias() {
        let mut cache = Cache::new();
        let mut horizontal = input(SizingPurpose::IntrinsicContribution);
        horizontal.parent_size = Size { width: Some(100.0), height: Some(100.0) };
        cache.store(&horizontal, LayoutOutput::from_outer_size(Size { width: 80.0, height: 20.0 }));

        let mut vertical = horizontal;
        vertical.parent_writing_mode = WritingMode::VerticalRl;

        assert!(cache.get(&vertical).is_none());
    }
}
