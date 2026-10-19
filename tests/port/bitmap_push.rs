//! Port of `tests/push.rs` (`append` / `from_sorted_iter`, both `RoaringBitmap` and
//! `RoaringTreemap` flavours).

use roaring128::{IntegerTooSmall, RoaringBitmap128};

use crate::common::*;

/// macro created to reduce code duplication
macro_rules! test_from_sorted_iter {
    ($values: expr) => {{
        let values: Vec<u128> = $values;
        let rb1 = RoaringBitmap128::from_iter(values.clone());
        let rb2 = RoaringBitmap128::from_sorted_iter(values).unwrap();

        for (x, y) in rb1.iter().zip(rb2.iter()) {
            assert_eq!(x, y);
        }
        assert_eq!(rb1.len(), rb2.len());
        assert_eq!(rb1.min(), rb2.min());
        assert_eq!(rb1.max(), rb2.max());
        assert_eq!(rb1.is_empty(), rb2.is_empty());
        assert_eq!(rb1, rb2);
        assert_valid(&rb2);
    }};
}

fn append(off: u128) {
    test_from_sorted_iter!((0..1_000_000u128).map(|x| off + 13 * x).collect::<Vec<u128>>());
    test_from_sorted_iter!(shift_vec(off, [1, 2, 4, 5, 7, 8, 9]));
}

fn append_tree(off: u128) {
    // the treemap flavour used `u64` values spread over 2^32 buckets
    test_from_sorted_iter!((0..1_000_000u128).map(|x| off + 13 * x).collect::<Vec<u128>>());
    test_from_sorted_iter!(shift_vec(off, [1, 2, 4, 5, 7, 8, 9]));
    test_from_sorted_iter!((0..10_000u128).map(|x| off + (x << 10)).collect::<Vec<u128>>());
}

offset_variants!(append, append_tree);

#[test]
fn append_empty() {
    assert_eq!(RoaringBitmap128::new().append(vec![]), Ok(0u64))
}

#[test]
fn append_error() {
    match [100u128].iter().cloned().collect::<RoaringBitmap128>().append(vec![10, 20, 0]) {
        Ok(_) => {
            panic!("The 0th element in the iterator was < the max of the bitmap")
        }
        Err(non_sorted_error) => {
            assert_eq!(non_sorted_error.valid_until(), 0)
        }
    }

    match [100u128].iter().cloned().collect::<RoaringBitmap128>().append(vec![200, 201, 201]) {
        Ok(_) => {
            panic!("The 3rd element in the iterator was < 2nd")
        }
        Err(non_sorted_error) => {
            assert_eq!(non_sorted_error.valid_until(), 2)
        }
    }
}

#[test]
fn append_error_keeps_the_valid_prefix_u128() {
    let mut rb = RoaringBitmap128::from([1 << 64]);
    let err = rb.append([(1 << 64) + 1, 1 << 100, u128::MAX - 1, 5, u128::MAX]).unwrap_err();
    assert_eq!(err.valid_until(), 3);
    assert_valid(&rb);
    assert!(rb.iter().eq([1 << 64, (1 << 64) + 1, 1 << 100, u128::MAX - 1]));

    // equal to the maximum is an error too
    let err = rb.append([u128::MAX - 1]).unwrap_err();
    assert_eq!(err.valid_until(), 0);
    assert_eq!(rb.append([u128::MAX]), Ok(1));
    assert_eq!(rb.max(), Some(u128::MAX));
    // nothing can follow u128::MAX
    assert_eq!(rb.append([u128::MAX]).unwrap_err().valid_until(), 0);
    assert_eq!(rb.append(std::iter::empty()), Ok(0));
    assert_eq!(rb.len(), 5);
}

#[test]
fn from_sorted_iter_errors_u128() {
    let err = RoaringBitmap128::from_sorted_iter([u128::MAX, 0]).unwrap_err();
    assert_eq!(err.valid_until(), 1);
    let err = RoaringBitmap128::from_sorted_iter([0, 1 << 64, 1 << 64]).unwrap_err();
    assert_eq!(err.valid_until(), 2);
    let rb = RoaringBitmap128::from_sorted_iter([0, 1 << 64, u128::MAX]).unwrap();
    assert_valid(&rb);
    assert_eq!(rb.len(), 3);
    // Display of the error
    assert_eq!(err.to_string(), "integers are ordered up to the 2th element");
}

#[test]
#[allow(deprecated)]
fn push_and_try_push_u128() {
    let mut rb = RoaringBitmap128::new();
    assert!(rb.try_push(u64::MAX as u128).is_ok());
    assert_eq!(rb.try_push(u64::MAX as u128), Err(IntegerTooSmall));
    assert_eq!(rb.try_push(5), Err(IntegerTooSmall));
    assert!(rb.push(1 << 64));
    assert!(!rb.push(1 << 64));
    assert!(rb.try_push(u128::MAX - 1).is_ok());
    assert!(rb.push(u128::MAX));
    assert!(!rb.push(u128::MAX));
    assert_eq!(rb.try_push(u128::MAX), Err(IntegerTooSmall));
    assert_valid(&rb);
    assert!(rb.iter().eq([u64::MAX as u128, 1 << 64, u128::MAX - 1, u128::MAX]));
    assert_eq!(IntegerTooSmall.to_string(), "inserted integer is smaller than the largest integer");
}

#[test]
fn try_push_across_container_kinds_u128() {
    // push through an array, into a bitmap (> 4096 values) and across keys
    let off = OFF_STRADDLE_2P64;
    let mut rb = RoaringBitmap128::new();
    for v in (0..10_000u128).map(|x| off + 2 * x) {
        assert!(rb.try_push(v).is_ok(), "try_push({v:#x})");
    }
    assert_valid(&rb);
    assert_eq!(rb.len(), 10_000);
    assert!(rb.iter().eq((0..10_000u128).map(|x| off + 2 * x)));
    // push after a run container
    let mut rb = RoaringBitmap128::new();
    rb.insert_range(off..off + 100_000);
    assert_eq!(rb.try_push(off + 99_999), Err(IntegerTooSmall));
    assert!(rb.try_push(off + 100_000).is_ok());
    assert!(rb.try_push(off + 200_000).is_ok());
    assert_valid(&rb);
    assert_eq!(rb.len(), 100_002);
}
