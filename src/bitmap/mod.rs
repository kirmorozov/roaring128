use alloc::collections::BTreeMap;

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
mod statistics;
#[cfg(feature = "uuid")]
mod uuid;

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
