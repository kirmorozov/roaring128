//! Port of `tests/treemap_ops.rs` (differs from `tests/ops.rs` only in `sub`).

use crate::common::*;

fn sub(off: u128) {
    let mut rb1 = shifted(off, 1..4);
    let rb2 = shifted(off, 3..6);
    let rb3 = shifted(off, 1..3);

    assert_eq!(rb3, &rb1 - &rb2);
    assert_eq!(rb3, &rb1 - rb2.clone());
    assert_eq!(rb3, rb1.clone() - &rb2);
    assert_eq!(rb3, rb1.clone() - rb2.clone());
    assert_eq!(rb3.len(), rb1.difference_len(&rb2));

    rb1 -= &rb2;
    rb1 -= rb2;

    assert_eq!(rb3, rb1);
}

/// The treemap ops across 2^32 buckets: one value per "bucket" boundary side.
fn ops_across_buckets(off: u128) {
    let lo = (1u64 << 32) - 2;
    let rb1 = shifted(off, lo..lo + 4);
    let rb2 = shifted(off, lo + 2..lo + 6);
    assert_eq!(&rb1 | &rb2, shifted(off, lo..lo + 6));
    assert_eq!(&rb1 & &rb2, shifted(off, lo + 2..lo + 4));
    assert_eq!(&rb1 - &rb2, shifted(off, lo..lo + 2));
    assert_eq!(&rb1 ^ &rb2, shifted(off, (lo..lo + 2).chain(lo + 4..lo + 6)));
    assert_eq!(rb1.union_len(&rb2), 6);
    assert_eq!(rb1.intersection_len(&rb2), 2);
    assert_eq!(rb1.difference_len(&rb2), 2);
    assert_eq!(rb1.symmetric_difference_len(&rb2), 4);
}

offset_variants!(sub);
wide_offset_variants!(ops_across_buckets);
