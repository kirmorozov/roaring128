//! Model-based property tests: every public method of `RoaringBitmap128` is compared with a
//! `BTreeSet<u128>` model on structured sets (array clusters, dense bitmap blocks, runs spanning
//! several keys, sparse UUID-like values, values around 2^32, 2^64, 2^112 and `u128::MAX`), and
//! `internal_validate()` is asserted after every mutation.
//!
//! Default case counts are small enough for the whole binary to run in seconds; raise them with
//! `PROPTEST_CASES`.

#[macro_use]
#[path = "../common/mod.rs"]
mod common;

mod build;
mod iter;
mod misc;
mod multiops;
mod point;
mod ranges;
mod sequence;
mod setops;
