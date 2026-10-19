//! Port of `tests/ops.rs`.

use roaring128::RoaringBitmap128;

use crate::common::*;

fn or(off: u128) {
    let mut rb1 = shifted(off, 1..4);
    let rb2 = shifted(off, 3..6);
    let rb3 = shifted(off, 1..6);

    assert_eq!(rb3, &rb1 | &rb2);
    assert_eq!(rb3, &rb1 | rb2.clone());
    assert_eq!(rb3, rb1.clone() | &rb2);
    assert_eq!(rb3, rb1.clone() | rb2.clone());
    assert_eq!(rb3.len(), rb1.union_len(&rb2));

    rb1 |= &rb2;
    rb1 |= rb2;

    assert_eq!(rb3, rb1);
}

fn and(off: u128) {
    let mut rb1 = shifted(off, 1..4);
    let rb2 = shifted(off, 3..6);
    let rb3 = shifted(off, 3..4);

    assert_eq!(rb3, &rb1 & &rb2);
    assert_eq!(rb3, &rb1 & rb2.clone());
    assert_eq!(rb3, rb1.clone() & &rb2);
    assert_eq!(rb3, rb1.clone() & rb2.clone());
    assert_eq!(rb3.len(), rb1.intersection_len(&rb2));

    rb1 &= &rb2;
    rb1 &= rb2;

    assert_eq!(rb3, rb1);
}

fn sub(off: u128) {
    let mut rb1 = shifted(off, 1..4000);
    let rb2 = shifted(off, 3..5000);
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

// See roaring issue #327
fn subtraction_preserves_zero_element(off: u128) {
    let mut a = shifted(off, [0, 35, 80, 104, 138, 214, 235, 258]);
    let b = shifted(off, [9, 35, 42, 51, 111, 134, 231, 239]);

    a -= b;

    // The bug: element 0 should still be present but was being removed
    assert!(
        a.contains(off),
        "Element 0 should be present after subtraction"
    );

    // Verify the complete result
    let expected = shift_vec(off, [0, 80, 104, 138, 214, 235, 258]);
    let actual: Vec<u128> = a.iter().collect();
    assert_eq!(
        actual, expected,
        "Subtraction result should match expected values"
    );
}

fn xor(off: u128) {
    let mut rb1 = shifted(off, 1..4);
    let rb2 = shifted(off, 3..6);
    let rb3 = shifted(off, (1..3).chain(4..6));
    let rb4 = shifted(off, 0..0);

    assert_eq!(rb3, &rb1 ^ &rb2);
    assert_eq!(rb3, &rb1 ^ rb2.clone());
    assert_eq!(rb3, rb1.clone() ^ &rb2);
    assert_eq!(rb3, rb1.clone() ^ rb2.clone());
    assert_eq!(rb3.len(), rb1.symmetric_difference_len(&rb2));

    rb1 ^= &rb2;

    assert_eq!(rb3, rb1);

    rb1 ^= rb3;

    assert_eq!(rb4, rb1);
}

offset_variants!(or, and, sub, subtraction_preserves_zero_element, xor);

#[test]
fn ops_on_max_value_u128() {
    let a = RoaringBitmap128::from([u128::MAX, u128::MAX - 1, 0]);
    let b = RoaringBitmap128::from([u128::MAX, 1 << 64]);
    assert!((&a | &b).iter().eq([0, 1 << 64, u128::MAX - 1, u128::MAX]));
    assert!((&a & &b).iter().eq([u128::MAX]));
    assert!((&a - &b).iter().eq([0, u128::MAX - 1]));
    assert!((&a ^ &b).iter().eq([0, 1 << 64, u128::MAX - 1]));
    assert_eq!(a.union_len(&b), 4);
    assert_eq!(a.intersection_len(&b), 1);
    assert_eq!(a.difference_len(&b), 2);
    assert_eq!(a.symmetric_difference_len(&b), 3);
}

#[test]
fn len_ops_on_empty_u128() {
    let e = RoaringBitmap128::new();
    let a = RoaringBitmap128::from([1 << 100]);
    assert_eq!(e.union_len(&e), 0);
    assert_eq!(e.intersection_len(&a), 0);
    assert_eq!(a.difference_len(&e), 1);
    assert_eq!(e.difference_len(&a), 0);
    assert_eq!(e.symmetric_difference_len(&a), 1);
    assert_eq!(a.symmetric_difference_len(&a), 0);
}
