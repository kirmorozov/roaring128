//! Serialization tests (`FORMAT.md`): round trips, `serialized_size`, exact bytes against an
//! independent reference encoder, decoding of hand-written inputs, rejection of corrupted input,
//! `deserialize_unchecked_from`, `intersection_with_serialized_unchecked` and serde.
//!
//! Ports: `bitmap_serialization` (`tests/serialization.rs`; the CRoaring fixture files do not
//! apply to the 128-bit format), `treemap_serialization` (`tests/treemap_serialization.rs`),
//! `src_bitmap_serialization` (round trip proptest of `src/bitmap/serialization.rs`; its
//! `from_lsb0_bytes` tests live in the `model` binary), `src_bitmap_ops_with_serialized`,
//! `src_bitmap_serde` (`src/{bitmap,treemap}/serde.rs`).
//!
//! Inputs with oversized container counts are in the separate `serialization_limits` binary so
//! that an allocation failure (abort) there cannot hide the results of this binary.

#[macro_use]
#[path = "../common/mod.rs"]
mod common;

mod raw;

mod bitmap_serialization;
mod corrupt;
mod format;
mod src_bitmap_ops_with_serialized;
mod src_bitmap_serde;
mod src_bitmap_serialization;
mod treemap_serialization;
