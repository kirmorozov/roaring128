use core::ops::{Bound, RangeBounds, RangeInclusive};

/// Number of low bits stored inside a container.
pub(crate) const INDEX_BITS: u32 = 16;

/// The largest valid container key (`u128::MAX >> 16`).
pub(crate) const MAX_KEY: u128 = u128::MAX >> INDEX_BITS;

/// Returns the container key and the index in this container for a given integer.
#[inline(always)]
pub(crate) const fn split(value: u128) -> (u128, u16) {
    (value >> INDEX_BITS, value as u16)
}

/// Returns the original integer from the container key and the index of it in the container.
#[inline(always)]
pub(crate) const fn join(key: u128, index: u16) -> u128 {
    (key << INDEX_BITS) | index as u128
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ConvertRangeError {
    Empty,
    StartGreaterThanEnd,
    StartAndEndEqualExcluded,
}

/// Convert a `RangeBounds<u128>` object to `RangeInclusive<u128>`.
pub(crate) fn convert_range_to_inclusive<R>(
    range: R,
) -> Result<RangeInclusive<u128>, ConvertRangeError>
where
    R: RangeBounds<u128>,
{
    let start_bound = range.start_bound().cloned();
    let end_bound = range.end_bound().cloned();
    match (start_bound, end_bound) {
        (Bound::Excluded(s), Bound::Excluded(e)) if s == e => {
            Err(ConvertRangeError::StartAndEndEqualExcluded)
        }
        (Bound::Included(s) | Bound::Excluded(s), Bound::Included(e) | Bound::Excluded(e))
            if s > e =>
        {
            Err(ConvertRangeError::StartGreaterThanEnd)
        }
        _ => {
            let start = match start_bound {
                Bound::Included(s) => s,
                Bound::Excluded(s) => s.checked_add(1).ok_or(ConvertRangeError::Empty)?,
                Bound::Unbounded => 0,
            };

            let end = match end_bound {
                Bound::Included(e) => e,
                Bound::Excluded(e) => e.checked_sub(1).ok_or(ConvertRangeError::Empty)?,
                Bound::Unbounded => u128::MAX,
            };

            if start > end {
                // This handles e.g. `x..x`: we've ruled out `start > end` overall, so a value must
                // have been changed via exclusion.
                Err(ConvertRangeError::Empty)
            } else {
                Ok(start..=end)
            }
        }
    }
}

/// Splits an inclusive `u128` range into the container key span and the low indices.
///
/// Returns `(start_key, start_index, end_key, end_index)`.
#[inline]
pub(crate) fn split_range(range: &RangeInclusive<u128>) -> (u128, u16, u128, u16) {
    let (start_key, start_index) = split(*range.start());
    let (end_key, end_index) = split(*range.end());
    (start_key, start_index, end_key, end_index)
}

/// The index range of the container `key` that intersects the `[start, end]` key span, given the
/// partial indices of the first and last containers.
#[inline]
pub(crate) fn container_range(
    key: u128,
    start_key: u128,
    start_index: u16,
    end_key: u128,
    end_index: u16,
) -> RangeInclusive<u16> {
    let lo = if key == start_key { start_index } else { 0 };
    let hi = if key == end_key { end_index } else { u16::MAX };
    lo..=hi
}

#[cfg(test)]
mod test {
    use super::{ConvertRangeError, convert_range_to_inclusive, join, split};
    use core::ops::Bound;

    #[test]
    fn test_split_u128() {
        assert_eq!((0, 0), split(0));
        assert_eq!((0, 1), split(1));
        assert_eq!((0, 0xFFFF), split(0xFFFF));
        assert_eq!((1, 0), split(0x1_0000));
        assert_eq!((1, 1), split(0x1_0001));
        assert_eq!((u128::MAX >> 16, 0xFFFE), split(u128::MAX - 1));
        assert_eq!((u128::MAX >> 16, 0xFFFF), split(u128::MAX));
        assert_eq!((1 << 96, 0x1234), split((1 << 112) | 0x1234));
    }

    #[test]
    fn test_join_u128() {
        assert_eq!(0, join(0, 0));
        assert_eq!(1, join(0, 1));
        assert_eq!(0x1_0000, join(1, 0));
        assert_eq!(u128::MAX, join(u128::MAX >> 16, 0xFFFF));
        for v in [
            0,
            1,
            0xFFFF,
            0x1_0000,
            u64::MAX as u128,
            1 << 100,
            u128::MAX - 7,
            u128::MAX,
        ] {
            let (k, i) = split(v);
            assert_eq!(join(k, i), v);
        }
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn test_convert_range_to_inclusive() {
        assert_eq!(Ok(1..=5), convert_range_to_inclusive(1..6));
        assert_eq!(Ok(1..=u128::MAX), convert_range_to_inclusive(1..));
        assert_eq!(Ok(0..=u128::MAX), convert_range_to_inclusive(..));
        assert_eq!(Ok(16..=16), convert_range_to_inclusive(16..=16));
        assert_eq!(
            Ok(11..=19),
            convert_range_to_inclusive((Bound::Excluded(10), Bound::Excluded(20)))
        );

        assert_eq!(
            Err(ConvertRangeError::Empty),
            convert_range_to_inclusive(0..0)
        );
        assert_eq!(
            Err(ConvertRangeError::Empty),
            convert_range_to_inclusive(5..5)
        );
        assert_eq!(
            Err(ConvertRangeError::StartGreaterThanEnd),
            convert_range_to_inclusive(1..0)
        );
        assert_eq!(
            Err(ConvertRangeError::StartGreaterThanEnd),
            convert_range_to_inclusive(10..5)
        );
        assert_eq!(
            Err(ConvertRangeError::Empty),
            convert_range_to_inclusive((Bound::Excluded(u128::MAX), Bound::Included(u128::MAX)))
        );
        assert_eq!(
            Err(ConvertRangeError::StartAndEndEqualExcluded),
            convert_range_to_inclusive((Bound::Excluded(u128::MAX), Bound::Excluded(u128::MAX)))
        );
        assert_eq!(
            Err(ConvertRangeError::Empty),
            convert_range_to_inclusive((Bound::Excluded(0), Bound::Included(0)))
        );
    }
}
