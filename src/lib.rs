//! Native 128-bit [Roaring bitmaps](https://roaringbitmap.org/).
//!
//! [`RoaringBitmap128`] is a compressed set of `u128` values, designed for UUIDs and other
//! 128-bit identifiers. It is not a wrapper around 32- or 64-bit Roaring bitmaps: every value is
//! split into a 112-bit *key* and a 16-bit *index*, the keys live in an ordered index (a B-tree)
//! and each key owns one native Roaring container (sorted array, 8 KiB bitset or run-length
//! encoded intervals). The container format, conversion thresholds and container algorithms follow
//! the Roaring specification, so the behaviour and the API mirror the
//! [`roaring`](https://docs.rs/roaring) crate (`RoaringBitmap` / `RoaringTreemap`).
//!
//! Hot loops are vectorized, and there is deliberately no fallback for older CPUs:
//!
//! * `x86_64`: AVX2 + BMI1/BMI2 + POPCNT is the minimum (`x86-64-v3`: Intel Haswell / AMD Zen
//!   or newer). AVX-512 F/BW/VL/DQ/VPOPCNTDQ/BITALG/VBMI2 (Intel Ice Lake, AMD Zen 4 and newer)
//!   is used automatically when present. On a CPU without AVX2 the first use of a bitmap panics.
//! * `aarch64`: NEON.
//! * Other architectures are rejected at compile time.
//!
//! With the `std` feature the backend is detected at runtime; without it, it is chosen from the
//! target features enabled at compile time (see [`SimdBackend`]). The portable scalar code is
//! only the reference implementation the vector kernels are tested against; it is never selected
//! as a backend. [`simd_backend`] reports which code path is used on the running machine.
//!
//! Sparse data (such as random UUIDs, where virtually every value owns a container) is handled
//! without per-value heap allocations: small array containers are stored inline in the 16-byte
//! container slot.
//!
//! # Example
//!
//! ```
//! use roaring128::RoaringBitmap128;
//!
//! let mut ids = RoaringBitmap128::new();
//! ids.insert(0x0192_7a3e_8f00_7cc1_9e3a_1b2c_3d4e_5f60);
//! ids.insert_range(10..20);
//! assert!(ids.contains(15));
//! assert_eq!(ids.len(), 11);
//!
//! let other: RoaringBitmap128 = (15..30).collect();
//! assert_eq!((&ids & &other).len(), 5);
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(unsafe_op_in_unsafe_fn)]
#![deny(unnameable_types)]

extern crate alloc;

use core::fmt;

mod bitmap;
mod container;
mod simd;
mod util;

pub use bitmap::{IntoIter, Iter, RoaringBitmap128, Statistics};
pub use simd::{SimdBackend, simd_backend};

/// An error type that is returned when a `try_push` in a bitmap did not succeed.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntegerTooSmall;

impl fmt::Display for IntegerTooSmall {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("inserted integer is smaller than the largest integer")
    }
}

impl core::error::Error for IntegerTooSmall {}

/// An error type that is returned when an iterator isn't sorted.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonSortedIntegers {
    valid_until: u64,
}

impl NonSortedIntegers {
    /// Returns the number of elements that were sorted before the first unsorted one.
    pub fn valid_until(&self) -> u64 {
        self.valid_until
    }
}

impl fmt::Display for NonSortedIntegers {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "integers are ordered up to the {}th element",
            self.valid_until()
        )
    }
}

impl core::error::Error for NonSortedIntegers {}

/// A [`Iterator::collect`] blanket implementation that provides extra methods for
/// [`RoaringBitmap128`].
///
/// When merging multiple bitmap with the same operation it's usually faster to call the
/// method in this trait than to write your own for loop and merging the bitmaps yourself.
///
/// # Examples
/// ```
/// use roaring128::{MultiOps, RoaringBitmap128};
///
/// let bitmaps = [
///     RoaringBitmap128::from_iter(0..10),
///     RoaringBitmap128::from_iter(10..20),
///     RoaringBitmap128::from_iter(20..30),
/// ];
///
/// // Stop doing this
/// let naive = bitmaps.clone().into_iter().reduce(|a, b| a | b).unwrap_or_default();
///
/// // And start doing this instead, it will be much faster!
/// let iter = bitmaps.union();
///
/// assert_eq!(naive, iter);
/// ```
pub trait MultiOps<T>: IntoIterator<Item = T> {
    /// The type of output from operations.
    type Output;

    /// The `union` between all elements.
    fn union(self) -> Self::Output;

    /// The `intersection` between all elements.
    fn intersection(self) -> Self::Output;

    /// The `difference` between all elements.
    fn difference(self) -> Self::Output;

    /// The `symmetric difference` between all elements.
    fn symmetric_difference(self) -> Self::Output;
}
