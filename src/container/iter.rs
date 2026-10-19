//! Container iterators over `u16` indices.
//!
//! Both iterators are double ended and exact sized, and support the seeking and range helpers
//! needed by the top-level iterators (`advance_to`, `advance_back_to`, `next_range`,
//! `next_range_back`, `peek`, `peek_back`), with the semantics of the `roaring` crate:
//!
//! * `advance_to(n)`: the next value returned from the front is the first remaining value
//!   `>= n` (no effect if the front is already past `n`).
//! * `advance_back_to(n)`: the next value returned from the back is the last remaining value
//!   `<= n`.
//! * `next_range()`: consumes and returns the maximal run of consecutive values starting at the
//!   front.
//! * `next_range_back()`: consumes and returns the maximal run of consecutive values ending at
//!   the back.

use core::ops::RangeInclusive;

/// Borrowing container iterator.
#[derive(Clone)]
pub(crate) struct Iter<'a> {
    _todo: core::marker::PhantomData<&'a ()>,
}

/// Owning container iterator.
#[derive(Clone)]
pub(crate) struct IntoIter {
    _todo: (),
}

macro_rules! iter_api {
    ($ty:ty) => {
        #[allow(dead_code)]
        impl $ty {
            pub(crate) fn advance_to(&mut self, n: u16) {
                todo!("{n}")
            }

            pub(crate) fn advance_back_to(&mut self, n: u16) {
                todo!("{n}")
            }

            pub(crate) fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
                todo!()
            }

            pub(crate) fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
                todo!()
            }

            pub(crate) fn peek(&self) -> Option<u16> {
                todo!()
            }

            pub(crate) fn peek_back(&self) -> Option<u16> {
                todo!()
            }
        }

        impl Iterator for $ty {
            type Item = u16;

            fn next(&mut self) -> Option<u16> {
                todo!()
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                todo!()
            }

            // Implementations should also override `count`, `nth` and `fold` for speed.
        }

        impl DoubleEndedIterator for $ty {
            fn next_back(&mut self) -> Option<u16> {
                todo!()
            }
        }

        impl ExactSizeIterator for $ty {}
        impl core::iter::FusedIterator for $ty {}
    };
}

iter_api!(Iter<'_>);
iter_api!(IntoIter);
