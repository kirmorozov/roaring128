//! Port of `tests/serialization.rs`.
//!
//! The CRoaring fixtures (`bitmapwithoutruns.bin`, `bitmapwithruns.bin`) are for the 32-bit
//! format; their data set is kept and checked against the reference encoder of `raw.rs` instead.
//! The invalid inputs of the original are rewritten for the 128-bit format.

use roaring128::RoaringBitmap128;

use crate::common::*;
use crate::raw::*;

fn test_data_bitmap(off: u128) -> RoaringBitmap128 {
    (0..100u128)
        .map(|i| i * 1000)
        .chain((100_000..200_000u128).map(|i| i * 3))
        .chain(700_000..800_000u128)
        .map(|v| off + v)
        .collect::<RoaringBitmap128>()
}

fn serialize_and_deserialize(bitmap: &RoaringBitmap128) -> RoaringBitmap128 {
    let mut buffer = vec![];
    bitmap.serialize_into(&mut buffer).unwrap();
    assert_eq!(buffer.len(), bitmap.serialized_size());
    let back = RoaringBitmap128::deserialize_from(&buffer[..]).unwrap();
    assert_valid(&back);
    back
}

/// Replaces `test_serialize_into_provided_data` / `test_deserialize_without_runs_from_provided_data`.
fn test_data_without_runs(off: u128) {
    let bitmap = test_data_bitmap(off);
    let model: Model = bitmap.iter().collect();
    let expected = Raw::from_model(&model, false).encode();
    let buffer = serialize(&bitmap);
    assert!(
        buffer == expected,
        "serialized bytes differ from the reference encoding"
    );
    assert_eq!(
        RoaringBitmap128::deserialize_from(&expected[..]).unwrap(),
        bitmap
    );
}

/// Replaces `test_serialize_with_runs_into_provided_data` / `test_deserialize_with_runs_from_provided_data`.
fn test_data_with_runs(off: u128) {
    let mut bitmap = test_data_bitmap(off);
    // Call optimize to create run containers
    bitmap.optimize();
    let model: Model = bitmap.iter().collect();
    let expected = Raw::from_model(&model, true).encode();
    let buffer = serialize(&bitmap);
    assert!(
        buffer == expected,
        "serialized bytes differ from the reference encoding"
    );
    let back = RoaringBitmap128::deserialize_from(&mut &expected[..]).unwrap();
    assert_eq!(back, bitmap);
    assert_eq!(
        back.statistics().n_run_containers,
        bitmap.statistics().n_run_containers
    );
}

fn test_empty(_off: u128) {
    let original = RoaringBitmap128::new();
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_one(off: u128) {
    let original = shifted(off, 1..2);
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_array(off: u128) {
    let original = shifted(off, 1000..3000);
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_array_boundary(off: u128) {
    let original = shifted(off, 1000..5096);
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_bitmap_boundary(off: u128) {
    let original = shifted(off, 1000..5097);
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_bitmap(off: u128) {
    let original = shifted(off, 1000..6000);
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_arrays(off: u128) {
    let original = shifted(off, (1000..3000).chain(70000..74000));
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_bitmaps(off: u128) {
    let original = shifted(off, (1000..6000).chain(70000..77000));
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_mixed(off: u128) {
    let original = shifted(off, (1000..3000).chain(70000..77000));
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

fn test_runs(off: u128) {
    let mut original = shifted(off, (1000..3000).chain(70000..77000));
    original.optimize();
    let new = serialize_and_deserialize(&original);
    assert_eq!(original.len(), new.len());
    assert_eq!(original.min(), new.min());
    assert_eq!(original.max(), new.max());
    assert_eq!(original, new);
}

const STRANGE: &[u64] = &[
    6619162, 6619180, 6619181, 6619217, 6619218, 6619257, 6619258, 6619259, 6619260, 6619261,
    6619262, 6619263, 6619264, 6619265, 6619266, 6619292, 6619294, 6619322, 6619461, 6619485,
    6619490, 6619500, 6619594, 6619619, 6619620, 6619623, 6619630, 6619632, 6619700, 6619701,
    6619702, 6619703, 6619813, 6619896, 6619967, 6620022, 6620034, 6620038, 6620110, 6620182,
    6620189, 6620194, 6620200, 6620262, 6620289, 6620321, 6620353, 6620354, 6620355, 6620356,
    6620384, 6620403, 6620431, 6620501, 6620529, 6620596, 6620674, 6620691, 6620770, 6620813,
    6620889, 6620890, 6620891, 6620892, 6620893, 6620894, 6620895, 6620928, 6620929, 6620948,
    6621023, 6621035, 6621046, 6621183, 6621224, 6621227, 6621261, 6621274, 6621345, 6621346,
    6621347, 6621348, 6621349, 6621350, 6621351, 6621352, 6621353, 6621354, 6621355, 6621356,
    6621357, 6621358, 6621359, 6621360, 6621361, 6621362, 6621363, 6621364, 6621365, 6621395,
    6621442, 6621457, 6621518, 6621519, 6621520, 6621521, 6621522, 6621523, 6621524, 6621525,
    6621526, 6621527, 6621538, 6621539, 6621540, 6621541, 6621542, 6621543, 6621544, 6621545,
    6621546, 6621547, 6621548, 6621553, 6621666, 6621715, 6621724, 6621738, 6621844, 6621845,
    6621859, 6621875, 6621891, 6621912, 6621964, 6621969, 6621998, 6621999, 6622037, 6622068,
    6622125, 6622126, 6622137, 6622196, 6622327, 6622328, 6622331, 6622333, 6622355, 6622374,
    6622390, 6622410, 6622411, 6622412, 6622417, 6622418, 6622419, 6622420, 6622421, 6622422,
    6622423, 6622424, 6622425, 6622426, 6622427, 6622428, 6622429, 6622430, 6622431, 6622432,
    6622433, 6622434, 6622518, 6622519, 6622551, 6622571, 6622588, 6622597, 6622599, 6622619,
    6622638, 6622703, 6622704, 6622705, 6622714, 6622734, 6622778, 6622833, 6622834, 6622849,
    6622872, 6622891, 6622964, 6622986, 6623000, 6623004, 6623037, 6623053, 6623054, 6623128,
    6623153, 6623233, 6623243, 6623247, 6623249, 6623267, 6623271, 6623302, 6623339, 6623341,
    6623342, 6623350, 6623353, 6623419, 6623437, 6623513, 6623603, 6623632, 6623656, 6623683,
    6681954, 6681955, 6681956, 6681957, 6681958, 6681959, 6681960, 6681961, 6681962, 6681966,
    6681993, 6682073, 6682074, 6682093, 6682115, 6682131, 6682176, 6682204, 6682291, 6682359,
    6682393, 6682399, 6682433, 6682445, 6682508, 6682522, 6682523, 6682524, 6682528, 6682629,
    6682697, 6682724, 6682754, 6682829, 6682830, 6682831, 6682832, 6682833, 6682834, 6682835,
    6682836, 6682837, 6682838, 6682839, 6682840, 6682841, 6682842, 6682843, 6682844, 6682845,
    6682846, 6682847, 6682880, 6682897, 6682932, 6682966, 6682969, 6682981, 6682992, 6682999,
    6683023, 6683030, 6683038, 6683042, 6683062, 6683080, 6683081, 6683082, 6683083, 6683084,
    6683085, 6683086, 6683087, 6683088, 6683089, 6683090, 6683091, 6683092, 6683093, 6683094,
    6683095, 6683096, 6683097, 6683098, 6683099, 6683100, 6683101, 6683102, 6683103, 6683104,
    6683105, 6683106, 6683107, 6683108, 6683136, 6683140, 6683211, 6683212, 6683213, 6683233,
    6683248, 6683251, 6683263, 6683339, 6683363, 6683365, 6683369, 6683370, 6683371, 6683372,
    6683418, 6683431, 6683441, 6683447, 6683541, 6683604, 6683626, 6683627, 6683628, 6683629,
    6683630, 6683631, 6683632, 6683633, 6683634, 6683635, 6683636, 6683747, 6683758, 6683878,
    6683984, 6684040, 6684070, 6684115, 6684116, 6684117, 6684118, 6684119, 6684120, 6684145,
    6684154, 6684156, 6684258, 6684261, 6684262, 6684282, 6684283, 6684285, 6684314, 6684316,
    6684318, 6684320, 6684323, 6684324, 6684325, 6684326, 6684327, 6684328, 6684329, 6684330,
    6684331, 6684332, 6684333, 6684334, 6684335, 6684336, 6684337, 6684378, 6684407, 6684414,
    6684416, 6684424, 6684472, 6684563, 6684574, 6684575, 6684576, 6684577, 6684601, 6684635,
    6684636, 6684639, 6684640, 6684641, 6684642, 6684666, 108658947,
];

fn test_strange(off: u128) {
    // the largest value is ~1.1e8: use the wide offsets near the top
    let off = if off == OFF_NEAR_MAX {
        OFF_NEAR_MAX_WIDE
    } else {
        off
    };
    let original = STRANGE
        .iter()
        .map(|&v| off + v as u128)
        .collect::<RoaringBitmap128>();
    let new = serialize_and_deserialize(&original);
    assert_eq!(original, new);
}

offset_variants!(
    test_data_without_runs,
    test_data_with_runs,
    test_empty,
    test_one,
    test_array,
    test_array_boundary,
    test_bitmap_boundary,
    test_bitmap,
    test_arrays,
    test_bitmaps,
    test_mixed,
    test_runs,
    test_strange
);

#[test]
fn test_bitmap_high16bits() {
    let mut bitmap = RoaringBitmap128::new();
    for i in 0..1u128 << 16 {
        let value = i << 16;
        bitmap.insert(value);
    }

    let mut buffer = vec![];
    bitmap.serialize_into(&mut buffer).unwrap();

    let new = RoaringBitmap128::deserialize_from(&buffer[..]);
    assert!(new.is_ok());
    assert_eq!(bitmap, new.unwrap());
}

#[test]
fn test_bitmap_high_bits_u128() {
    // one value per key, keys spread over all 112 key bits (the UUID case: 18 bytes per value)
    let mut bitmap = RoaringBitmap128::new();
    for i in 0..1u128 << 16 {
        bitmap.insert(i.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835) | (1 << 127));
    }
    bitmap.insert(u128::MAX);
    let buffer = serialize(&bitmap);
    let s = bitmap.statistics();
    assert_eq!(s.n_array_containers, s.n_containers);
    // 16 bytes of header, then 16 bytes of descriptor + 2 bytes per value
    assert_eq!(
        buffer.len() as u64,
        16 + 16 * s.n_containers + 2 * bitmap.len()
    );
    let new = RoaringBitmap128::deserialize_from(&buffer[..]).unwrap();
    assert_valid(&new);
    assert_eq!(bitmap, new);
}

fn assert_invalid_serialization(serialized: &[u8], msg: &str) {
    let result = RoaringBitmap128::deserialize_from(serialized);
    if let Ok(res) = result {
        panic!("Expected error: {msg}. Got: {res:?}");
    }
}

// `deserialize_huge_container_count` / `deserialize_negative_container_count`: see the
// `serialization_limits` binary.

#[test]
fn deserialize_run_container_contiguous_ranges() {
    let raw = Raw::new(vec![(0, Payload::Run(vec![(0, 0), (1, 0)]))]);
    assert_invalid_serialization(&raw.encode(), "Contiguous ranges in run container");
}

#[test]
fn deserialize_run_container_overlap() {
    let raw = Raw::new(vec![(0, Payload::Run(vec![(0, 4), (1, 0)]))]);
    assert_invalid_serialization(&raw.encode(), "Overlapping ranges in run container");
}

#[test]
fn deserialize_run_container_overflow() {
    let raw = Raw::new(vec![(0, Payload::Run(vec![(0xFFFE, 4)]))]);
    assert_invalid_serialization(&raw.encode(), "Overflow in run container");
}

#[test]
fn deserialize_duplicate_keys() {
    let raw = Raw::new(vec![
        (0, Payload::Array(vec![0])),
        (0, Payload::Array(vec![0])),
    ]);
    assert_invalid_serialization(&raw.encode(), "Duplicate keys in containers");
}

#[test]
fn deserialize_unsorted_keys() {
    let raw = Raw::new(vec![
        (1, Payload::Array(vec![0])),
        (0, Payload::Array(vec![0])),
    ]);
    assert_invalid_serialization(&raw.encode(), "Unsorted keys in containers");
}

#[test]
fn deserialize_array_duplicate_value() {
    let raw = Raw::new(vec![(0, Payload::Array(vec![0, 0]))]);
    assert_invalid_serialization(&raw.encode(), "Duplicate values in array container");
}

#[test]
fn deserialize_array_unsorted_values() {
    let raw = Raw::new(vec![(0, Payload::Array(vec![1, 0]))]);
    assert_invalid_serialization(&raw.encode(), "Unsorted values in array container");
}

#[test]
fn deserialize_bitset_incorrect_cardinality() {
    // A full bitmap with the correct cardinality is accepted
    let raw = Raw::new(vec![(
        0,
        Payload::Bitmap {
            words: vec![u64::MAX; 1024],
            card: 0x1_0000,
        },
    )]);
    let result = RoaringBitmap128::deserialize_from(&raw.encode()[..]).unwrap();
    assert_eq!(result.len(), 0x1_0000);
    assert_valid(&result);

    // Bitset will no longer have the correct cardinality
    let mut words = vec![u64::MAX; 1024];
    words[0] = 0xFF00_FFFF_FFFF_FFFF;
    let raw = Raw::new(vec![(
        0,
        Payload::Bitmap {
            words,
            card: 0x1_0000,
        },
    )]);
    assert_invalid_serialization(&raw.encode(), "Bitset incorrect cardinality");
}
