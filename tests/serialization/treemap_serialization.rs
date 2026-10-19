//! Port of `tests/treemap_serialization.rs`.

use roaring128::RoaringBitmap128;

use crate::common::*;

fn serialize_deserialize<Dataset, I>(dataset: Dataset)
where
    Dataset: IntoIterator<Item = u128, IntoIter = I>,
    I: Iterator<Item = u128>,
{
    let rb = RoaringBitmap128::from_iter(dataset);

    let mut buffer = vec![];
    rb.serialize_into(&mut buffer).unwrap();

    assert_eq!(buffer.len(), rb.serialized_size());

    let new_rb = RoaringBitmap128::deserialize_from(&buffer[..]).unwrap();

    assert_eq!(rb, new_rb);
    assert_valid(&new_rb);
}

fn empty(_off: u128) {
    serialize_deserialize(vec![])
}

fn basic(off: u128) {
    serialize_deserialize(shift_vec(off, [1, 2, 3, 4, 5, 100, 1000]))
}

fn basic_2(off: u128) {
    serialize_deserialize(shift_vec(
        off,
        [1, 2, 3, 4, 5, 100, 1000, 10000, 100000, 1000000],
    ))
}

fn basic_3(off: u128) {
    let u32max = u32::MAX as u64;
    serialize_deserialize(shift_vec(
        off,
        vec![
            1,
            2,
            3,
            4,
            5,
            100,
            1000,
            10000,
            100000,
            1000000,
            u32max + 10,
            u32max << 10,
        ]
        .into_iter()
        .chain(u32max..(u32max + 2 * (1 << 16))),
    ))
}

offset_variants!(empty, basic, basic_2);
// `u32::MAX << 10` needs 2^42 of room
wide_offset_variants!(basic_3);

#[test]
fn basic_u128_extremes() {
    serialize_deserialize(
        [
            0u128,
            1,
            u64::MAX as u128,
            1 << 64,
            1 << 100,
            (1 << 112) - 1,
            1 << 112,
            u128::MAX - 1,
            u128::MAX,
        ]
        .into_iter()
        .chain(u128::MAX - 2 * (1 << 16)..u128::MAX),
    )
}
