//! Port of `tests/treemap_iter_advance_to.rs`: the tests that differ from
//! `tests/iter_advance_to.rs` (the shared ones are in `bitmap_iter_advance_to`).
//!
//! The treemap tests jump between 32-bit bitmaps (`1 << 32`, `u32::MAX + x`). Here the "far"
//! position is a parameter: `orig` uses the original values, the variants put the far values at
//! 2^64, 2^100, 2^112 and just below `u128::MAX`.

use roaring128::RoaringBitmap128;

macro_rules! far_variants {
    ($($f:ident),* $(,)?) => {
        variants!({
            orig = 1u128 << 32,
            far_2p64 = 1u128 << 64,
            far_2p100 = 1u128 << 100,
            far_2p112 = 1u128 << 112,
            far_max = u128::MAX - 0x2_0003,
        }; $($f),*);
    };
}

fn to_next_bitmap(far: u128) {
    // original: 0x2_0001 + u32::MAX, i.e. far - 1 + 0x2_0001
    let bm = RoaringBitmap128::from([1, far - 1 + 0x2_0001, far - 1 + 0x2_0002]);
    let mut i = bm.iter();
    i.advance_to(0x1_0000);
    assert_eq!(i.next(), Some(far - 1 + 0x2_0001));
    assert_eq!(i.next(), Some(far - 1 + 0x2_0002));
    assert_eq!(i.next(), None);
}

fn advance_to_later_bitmap_after_iteration_started(far: u128) {
    let later_bitmap = far;
    let bm = RoaringBitmap128::from([1, 3, later_bitmap]);
    let mut i = bm.iter();

    assert_eq!(i.next(), Some(1));
    i.advance_to(later_bitmap);

    assert_eq!(i.next(), Some(later_bitmap));
    assert_eq!(i.next(), None);
}

fn advance_to_missing_bitmap(far: u128) {
    let later_bitmap = far;
    let bm = RoaringBitmap128::from([later_bitmap]);
    let mut i = bm.iter();

    i.advance_to(3);

    assert_eq!(i.next(), Some(later_bitmap));
    assert_eq!(i.next(), None);
}

fn advance_back_to_earlier_bitmap_after_iteration_started(far: u128) {
    let later_bitmap = far;
    let bm = RoaringBitmap128::from([1, later_bitmap, later_bitmap + 2]);
    let mut i = bm.iter();

    assert_eq!(i.next_back(), Some(later_bitmap + 2));
    i.advance_back_to(1);

    assert_eq!(i.next_back(), Some(1));
    assert_eq!(i.next_back(), None);
}

fn advance_back_to_missing_bitmap(far: u128) {
    let later_bitmap = far;
    let bm = RoaringBitmap128::from([3]);
    let mut i = bm.iter();

    i.advance_back_to(later_bitmap);

    assert_eq!(i.next_back(), Some(3));
    assert_eq!(i.next_back(), None);
}

fn iter_multi_container_multi_bitmap(far: u128) {
    let m = far - 1; // u32::MAX in the original
    let bm = RoaringBitmap128::from([
        1,
        2,
        3,
        100000,
        100001,
        1 + m,
        2 + m,
        3 + m,
        100000 + m,
        100001 + m,
    ]);
    let mut i = bm.iter();
    i.advance_to(3);
    assert_eq!(i.size_hint(), (8, Some(8)));
    assert_eq!(i.next(), Some(3));
    assert_eq!(i.size_hint(), (7, Some(7)));
    assert_eq!(i.next(), Some(100000));
    assert_eq!(i.size_hint(), (6, Some(6)));
    assert_eq!(i.next(), Some(100001));
    assert_eq!(i.size_hint(), (5, Some(5)));
    assert_eq!(i.next(), Some(1 + m));
    assert_eq!(i.size_hint(), (4, Some(4)));
    assert_eq!(i.next(), Some(2 + m));
    assert_eq!(i.size_hint(), (3, Some(3)));
    assert_eq!(i.next(), Some(3 + m));
    assert_eq!(i.size_hint(), (2, Some(2)));
    assert_eq!(i.next(), Some(100000 + m));
    assert_eq!(i.size_hint(), (1, Some(1)));
    assert_eq!(i.next(), Some(100001 + m));
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None);
    assert_eq!(i.size_hint(), (0, Some(0)));
}

far_variants!(
    to_next_bitmap,
    advance_to_later_bitmap_after_iteration_started,
    advance_to_missing_bitmap,
    advance_back_to_earlier_bitmap_after_iteration_started,
    advance_back_to_missing_bitmap,
    iter_multi_container_multi_bitmap
);

#[test]
fn advance_to_end() {
    let bitmap = RoaringBitmap128::from([u64::MAX as u128]);
    let mut iter = bitmap.iter();
    iter.advance_to(u64::MAX as u128);
    assert_eq!(Some(u64::MAX as u128), iter.next());
    assert_eq!(None, iter.next());
}

#[test]
fn advance_to_end_u128() {
    let bitmap = RoaringBitmap128::from([u64::MAX as u128, u128::MAX]);
    let mut iter = bitmap.iter();
    iter.advance_to(u64::MAX as u128 + 1);
    assert_eq!(Some(u128::MAX), iter.next());
    assert_eq!(None, iter.next());
    let mut iter = bitmap.iter();
    iter.advance_back_to(u128::MAX - 1);
    assert_eq!(Some(u64::MAX as u128), iter.next_back());
    assert_eq!(None, iter.next_back());
}
