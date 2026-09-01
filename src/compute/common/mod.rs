//! Generic code that is shared between multiple layout algorithms
pub(crate) mod aspect_ratio;

#[cfg(any(feature = "block_layout", feature = "flexbox", feature = "grid"))]
/// Shared sizing helpers for absolutely positioned boxes.
pub(crate) mod absolute;

pub(crate) mod alignment;

pub(crate) mod baseline;

pub(crate) mod intrinsic_size;

pub(crate) mod used_size;

#[cfg(feature = "content_size")]
pub(crate) mod content_size;
