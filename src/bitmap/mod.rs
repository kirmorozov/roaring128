use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::container::Container;

mod cmp;
mod fmt;
mod inherent;
mod iter;
mod multiops;
mod ops;
#[cfg(feature = "std")]
mod ops_with_serialized;
#[cfg(feature = "serde")]
mod serde;
#[cfg(feature = "std")]
mod serialization;
mod setops;
mod statistics;
#[cfg(feature = "uuid")]
mod uuid;

#[cfg(test)]
mod tests;

pub use self::iter::{IntoIter, Iter};
pub use self::statistics::Statistics;

/// A compressed bitmap of `u128` values using the
/// [Roaring bitmap compression scheme](https://roaringbitmap.org/).
///
/// Values are split into a 112-bit key and a 16-bit index. Keys are kept in an ordered B-tree
/// index, each key owning one native Roaring container (array, bitmap or run). The total
/// cardinality is cached, so [`len`](Self::len) and [`is_empty`](Self::is_empty) are `O(1)`.
///
/// # Examples
///
/// ```rust
/// use roaring128::RoaringBitmap128;
///
/// let mut rb = RoaringBitmap128::new();
///
/// // insert all primes less than 10
/// rb.insert(2);
/// rb.insert(3);
/// rb.insert(5);
/// rb.insert(7);
/// println!("total bits set to true: {}", rb.len());
/// ```
pub struct RoaringBitmap128 {
    /// Container per 112-bit key. Never contains empty containers.
    pub(crate) map: BTreeMap<u128, Container>,
    /// Cached total cardinality, always equal to the sum of the container lengths.
    pub(crate) len: u64,
}

/// Strategy threshold for binary operations on two key indexes of very different sizes.
///
/// When `small * SKEW_RATIO < big` (counted in containers), operations walk the small side and
/// do `O(log n)` point lookups / entries in the big side instead of a linear merge over both.
/// A B-tree lookup costs roughly 20-30 sequential iteration steps; measured with 1M random
/// 128-bit values, lookups and the merge break even around `n / 16` for intersections,
/// differences and the cardinality / subset queries.
pub(crate) const SKEW_RATIO: usize = 16;

/// Whether `small` containers are few enough, compared to `big`, to prefer point lookups.
#[inline]
pub(crate) fn is_skewed(small: usize, big: usize) -> bool {
    small.saturating_mul(SKEW_RATIO) < big
}

/// Strategy threshold for updating a big index in place with a small one (unions, symmetric
/// differences, "clone the big side and fold the small one into it").
///
/// Lower than [`SKEW_RATIO`]: the merge alternative has to touch every container of the big side
/// and, for keys it does not have yet, rebuild its B-tree, while point updates only touch the
/// small side. Measured with 1M random 128-bit values, point updates win from about `n / 8`
/// down for scattered keys and break even with mostly shared keys.
pub(crate) const UPDATE_SKEW_RATIO: usize = 8;

/// Whether `small` containers are few enough, compared to `big`, to update `big` in place with
/// point entries rather than merging.
#[inline]
pub(crate) fn is_skewed_update(small: usize, big: usize) -> bool {
    small.saturating_mul(UPDATE_SKEW_RATIO) < big
}

/// Number of pending new keys below which they are inserted one by one into an existing index
/// instead of rebuilding it (`BTreeMap::append` is linear in the size of both maps, a single
/// insert is logarithmic but about 5x more expensive per element than a bulk build step).
#[inline]
pub(crate) fn prefer_point_inserts(new_keys: usize, existing: usize) -> bool {
    new_keys.saturating_mul(4) < existing
}

impl RoaringBitmap128 {
    /// Builds a bitmap from strictly increasing keys with non-empty containers and their total
    /// cardinality.
    ///
    /// `BTreeMap::from_iter` sorts its input with a stable sort (linear on sorted input) and then
    /// bulk-builds a densely packed tree in linear time.
    pub(crate) fn from_sorted_entries_with_len(
        entries: Vec<(u128, Container)>,
        len: u64,
    ) -> RoaringBitmap128 {
        debug_assert!(entries.windows(2).all(|w| w[0].0 < w[1].0));
        debug_assert_eq!(entries.iter().map(|(_, c)| c.len()).sum::<u64>(), len);
        RoaringBitmap128 {
            map: BTreeMap::from_iter(entries),
            len,
        }
    }

    /// Adds containers whose keys are not yet present (strictly increasing, non empty) to the
    /// index, choosing between point inserts and a linear rebuild. Does not touch `len`.
    pub(crate) fn merge_new_entries(&mut self, entries: Vec<(u128, Container)>) {
        debug_assert!(entries.windows(2).all(|w| w[0].0 < w[1].0));
        if entries.is_empty() {
            return;
        }
        if self.map.is_empty() {
            self.map = BTreeMap::from_iter(entries);
        } else if prefer_point_inserts(entries.len(), self.map.len()) {
            for (key, container) in entries {
                let previous = self.map.insert(key, container);
                debug_assert!(previous.is_none());
            }
        } else {
            let mut other = BTreeMap::from_iter(entries);
            self.map.append(&mut other);
        }
    }
}
