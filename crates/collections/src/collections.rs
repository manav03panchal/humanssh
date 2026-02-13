//! Fast collection types for HumanSSH.
//!
//! Re-exports `FxHashMap` and `FxHashSet` (faster than std for small keys),
//! plus `IndexMap`/`IndexSet` with FxHash for insertion-ordered maps.

pub use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
pub use std::collections::*;

/// Insertion-ordered hash map with FxHash (faster than default hasher).
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, FxBuildHasher>;

/// Insertion-ordered hash set with FxHash.
pub type IndexSet<T> = indexmap::IndexSet<T, FxBuildHasher>;
