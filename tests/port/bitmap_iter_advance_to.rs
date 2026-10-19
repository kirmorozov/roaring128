//! Port of `tests/iter_advance_to.rs`.

use roaring128::RoaringBitmap128;

use crate::common::*;

fn iter_basic(off: u128) {
    let bm = shifted(off, [1, 2, 3, 4, 11, 12, 13, 14]);
    let mut i = bm.iter();
    i.advance_to(off + 10);
    for n in 11..=14 {
        assert_eq!(i.next(), Some(off + n))
    }
    assert_eq!(i.next(), None);
}

fn to_missing_container(off: u128) {
    let bm = shifted(off, [1, 0x2_0001, 0x2_0002]);
    let mut i = bm.iter();
    i.advance_to(off + 0x1_0000);
    assert_eq!(i.next(), Some(off + 0x2_0001));
    assert_eq!(i.next(), Some(off + 0x2_0002));
    assert_eq!(i.next(), None);
}

fn iter_back_basic(off: u128) {
    let bm = shifted(off, [1, 2, 3, 4, 11, 12, 13, 14]);
    let mut i = bm.iter();
    i.advance_back_to(off + 10);
    assert_eq!(i.next(), Some(off + 1));
    assert_eq!(i.next(), Some(off + 2));
    assert_eq!(i.next_back(), Some(off + 4));
    assert_eq!(i.next_back(), Some(off + 3));

    assert_eq!(i.next(), None);
    assert_eq!(i.next_back(), None);
}

fn iter_advance_past_end(off: u128) {
    let bm = shifted(off, [1, 2, 3, 4, 11, 12, 13, 14]);
    let mut i = bm.iter();
    i.advance_to(off + 15);
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None);
}

fn iter_multi_container(off: u128) {
    let bm = shifted(off, [1, 2, 3, 100000, 100001]);
    let mut i = bm.iter();
    i.advance_to(off + 3);
    assert_eq!(i.size_hint(), (3, Some(3)));
    assert_eq!(i.next(), Some(off + 3));
    assert_eq!(i.size_hint(), (2, Some(2)));
    assert_eq!(i.next(), Some(off + 100000));
    assert_eq!(i.size_hint(), (1, Some(1)));
    assert_eq!(i.next(), Some(off + 100001));
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None);
    assert_eq!(i.size_hint(), (0, Some(0)));
}

fn iter_empty(off: u128) {
    let bm = RoaringBitmap128::new();
    let mut i = bm.iter();
    i.advance_to(off + 31337);
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None)
}

fn iter_back_empty(off: u128) {
    let bm = RoaringBitmap128::new();
    let mut i = bm.iter();
    i.advance_back_to(off + 31337);
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None)
}

fn into_iter_basic(off: u128) {
    let bm = shifted(off, [1, 2, 3, 4, 11, 12, 13, 14]);
    let mut i = bm.into_iter();
    i.advance_to(off + 10);
    let mut expected_size_hint = 4;
    assert_eq!(i.size_hint(), (expected_size_hint, Some(expected_size_hint)));
    for n in 11..=14 {
        assert_eq!(i.next(), Some(off + n));
        expected_size_hint -= 1;
        assert_eq!(i.size_hint(), (expected_size_hint, Some(expected_size_hint)));
    }
    assert_eq!(i.next(), None);
}

fn into_iter_multi_container(off: u128) {
    let bm = shifted(off, [1, 2, 3, 100000, 100001]);
    let mut i = bm.into_iter();
    i.advance_to(off + 3);
    assert_eq!(i.size_hint(), (3, Some(3)));
    assert_eq!(i.next(), Some(off + 3));
    assert_eq!(i.next(), Some(off + 100000));
    assert_eq!(i.next(), Some(off + 100001));
    assert_eq!(i.next(), None);
}

fn into_iter_empty(off: u128) {
    let bm = RoaringBitmap128::new();
    let mut i = bm.into_iter();
    i.advance_to(off + 31337);
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None)
}

fn into_iter_back_empty(off: u128) {
    let bm = RoaringBitmap128::new();
    let mut i = bm.into_iter();
    i.advance_back_to(off + 31337);
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None)
}

fn advance_to_with_tail_iter(off: u128) {
    let bm = shifted(off, [1, 2, 3, 100000, 100001]);
    let mut i = bm.iter();
    i.next_back();
    i.advance_to(off + 100000);
    assert_eq!(i.size_hint(), (1, Some(1)));
    assert_eq!(i.next(), Some(off + 100000));
    assert_eq!(i.size_hint(), (0, Some(0)));
    assert_eq!(i.next(), None);
}

fn advance_bitset(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    for i in (0..=0x2_0000).step_by(2) {
        bitmap.insert(off + i);
    }
    let mut iter = bitmap.iter();
    iter.advance_to(off + 0x1_0000 - 4);
    // 0x1_0000 + 5 is not in the bitmap, so the next value will be the first value less than that
    iter.advance_back_to(off + 0x1_0000 + 5);
    assert_eq!(iter.next(), Some(off + 0x1_0000 - 4));
    assert_eq!(iter.next_back(), Some(off + 0x1_0000 + 4));

    assert_eq!(iter.next(), Some(off + 0x1_0000 - 2));
    assert_eq!(iter.next(), Some(off + 0x1_0000));
    assert_eq!(iter.next(), Some(off + 0x1_0000 + 2));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

fn advance_bitset_current_word(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    for i in (0..=0x2_0000).step_by(2) {
        bitmap.insert(off + i);
    }
    let mut iter = bitmap.iter();
    iter.advance_to(off + 4);
    iter.advance_back_to(off + 0x2_0000 - 4);
    for i in (4..=(0x2_0000 - 4)).step_by(2) {
        assert_eq!(iter.next(), Some(off + i));
    }
    assert_eq!(iter.next(), None);
}

fn advance_bitset_to_end_word(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    for i in (0..=0x2_0000).step_by(2) {
        bitmap.insert(off + i);
    }
    let mut iter = bitmap.iter();
    iter.advance_to(off + 0x1_0000 - 4);
    for i in ((0x1_0000 - 4)..=0x2_0000).step_by(2) {
        assert_eq!(iter.next(), Some(off + i));
    }
    assert_eq!(iter.next(), None);
}

fn advance_bitset_back_to_start_word(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    for i in (0..=0x2_0000).step_by(2) {
        bitmap.insert(off + i);
    }
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 0x1_0000 - 4);
    for i in (0..=(0x1_0000 - 4)).step_by(2) {
        assert_eq!(iter.next(), Some(off + i));
    }
    assert_eq!(iter.next(), None);
}

fn advance_run_past_the_end(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..=off + 0x35B00);
    let mut iter = bitmap.iter();
    iter.advance_to(off + 0x35B01);
    assert_eq!(iter.next(), None);
}

fn advance_run_back_before_start(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off + 500..=off + 0x35B00);
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 499);
    assert_eq!(iter.next_back(), None);
}

fn advance_run_back_reduces_forward_iter(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..=off + 0x4000);
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 1);

    assert_eq!(iter.next(), Some(off));
    assert_eq!(iter.next(), Some(off + 1));
    assert_eq!(iter.next(), None);
}

fn advance_run_front_and_back_past_each_other(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..=off + 0x4000);
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 100);
    iter.advance_to(off + 300);
    assert_eq!(iter.next(), None);
}

fn advance_run_both_sides_past_each_other(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..off + 0x1000);
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 100);
    iter.advance_to(off + 0xFFFF);
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.nth_back(0), None);
}

fn advance_run_with_nth(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off + 36141..=off + 224407);
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 101779);
    assert_eq!(iter.nth(100563), None);
}

fn advance_to_with_next_len(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off + 100..off + 0x4000);
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 100);
    assert_eq!(iter.next(), Some(off + 100));
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.nth_back(0), None);
}

fn tmp(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off + 196363..=off + 262143);
    let mut iter = bitmap.iter();
    assert_eq!(iter.next_back(), Some(off + 262143));
    iter.advance_to(off + 228960);
    assert_eq!(iter.nth(36643), None);
}

fn advance_bitset_front_and_back_past_each_other(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..=off + 0x4000);
    bitmap.remove_run_compression();
    let mut iter = bitmap.iter();
    iter.advance_back_to(off + 100);
    iter.advance_to(off + 300);
    assert_eq!(iter.next(), None);
}

fn combine_with_nth(off: u128) {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(off..=off + 0xFFFF);
    bitmap.remove_run_compression();
    let mut iter = bitmap.iter();

    // Use nth to skip to a specific position
    assert_eq!(iter.nth(100), Some(off + 100));
    iter.advance_back_to(off + 50);
    assert_eq!(iter.next_back(), None);
}

/// Advancing backwards (to a smaller value than the current front) is a no-op.
fn advance_to_never_goes_back(off: u128) {
    let bm = shifted(off, [1, 2, 3, 100000, 100001]);
    let mut i = bm.iter();
    i.advance_to(off + 100000);
    i.advance_to(off + 1);
    assert_eq!(i.next(), Some(off + 100000));
    let mut i = bm.iter();
    i.advance_back_to(off + 2);
    i.advance_back_to(off + 100001);
    assert_eq!(i.next_back(), Some(off + 2));
    assert_eq!(i.len(), 1);
}

offset_variants!(
    iter_basic,
    to_missing_container,
    iter_back_basic,
    iter_advance_past_end,
    iter_multi_container,
    iter_empty,
    iter_back_empty,
    into_iter_basic,
    into_iter_multi_container,
    into_iter_empty,
    into_iter_back_empty,
    advance_to_with_tail_iter,
    advance_bitset,
    advance_bitset_current_word,
    advance_bitset_to_end_word,
    advance_bitset_back_to_start_word,
    advance_run_past_the_end,
    advance_run_back_before_start,
    advance_run_back_reduces_forward_iter,
    advance_run_front_and_back_past_each_other,
    advance_run_both_sides_past_each_other,
    advance_run_with_nth,
    advance_to_with_next_len,
    tmp,
    advance_bitset_front_and_back_past_each_other,
    combine_with_nth,
    advance_to_never_goes_back
);

#[test]
fn advance_to_end() {
    let bitmap = RoaringBitmap128::from([u32::MAX as u128]);
    let mut iter = bitmap.iter();
    iter.advance_to(u32::MAX as u128);
    assert_eq!(Some(u32::MAX as u128), iter.next());
    assert_eq!(None, iter.next());
}

#[test]
fn advance_to_end_u128() {
    let bitmap = RoaringBitmap128::from([1, u128::MAX]);
    let mut iter = bitmap.iter();
    iter.advance_to(u128::MAX);
    assert_eq!(iter.len(), 1);
    assert_eq!(Some(u128::MAX), iter.next());
    assert_eq!(None, iter.next());

    let mut iter = bitmap.clone().into_iter();
    iter.advance_to(u128::MAX);
    assert_eq!(Some(u128::MAX), iter.next());
    assert_eq!(None, iter.next());

    // advance_back_to(u128::MAX) is a no-op, advance_back_to(0) empties
    let mut iter = bitmap.iter();
    iter.advance_back_to(u128::MAX);
    assert_eq!(iter.len(), 2);
    iter.advance_back_to(0);
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);

    // advance_to(0) is a no-op
    let mut iter = bitmap.iter();
    iter.advance_to(0);
    assert_eq!(iter.len(), 2);
}

#[test]
fn advance_to_full_last_container_u128() {
    let mut bitmap = RoaringBitmap128::new();
    bitmap.insert_range(u128::MAX - 0xFFFF..);
    let mut iter = bitmap.iter();
    iter.advance_to(u128::MAX - 1);
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.next_back(), Some(u128::MAX));
    assert_eq!(iter.next(), Some(u128::MAX - 1));
    assert_eq!(iter.next(), None);
    let mut iter = bitmap.iter();
    iter.advance_back_to(u128::MAX - 0xFFFF);
    assert_eq!(iter.len(), 1);
    assert_eq!(iter.next(), Some(u128::MAX - 0xFFFF));
}
