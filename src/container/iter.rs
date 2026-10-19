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
//!
//! Every iterator keeps the exact number of remaining values, so `len`, `size_hint` and `count`
//! are `O(1)`; `nth`/`nth_back` skip whole words (bitmaps) or intervals (runs).
//!
//! The owning iterator keeps the container's storage (the inline array, the heap vector, the
//! bitmap block or the run list) and walks it in place, nothing is copied.

use core::ops::RangeInclusive;

use super::array::ArrayStore;
use super::bitmap::{BitmapStore, popcount_words, select_in_word};
use super::run::{Interval, RunStore};
use crate::simd::{BITMAP_WORDS, Words};

// ---------------------------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------------------------

/// Storage of the values remaining in an array iterator.
trait ArraySource {
    /// The remaining values.
    fn rest(&self) -> &[u16];
    /// Drops `n <= rest().len()` values from the front.
    fn consume_front(&mut self, n: usize);
    /// Drops `n <= rest().len()` values from the back.
    fn consume_back(&mut self, n: usize);
}

/// The remaining values of a borrowed array.
#[derive(Clone)]
struct Borrowed<'a>(&'a [u16]);

impl ArraySource for Borrowed<'_> {
    #[inline]
    fn rest(&self) -> &[u16] {
        self.0
    }

    #[inline]
    fn consume_front(&mut self, n: usize) {
        self.0 = &self.0[n..];
    }

    #[inline]
    fn consume_back(&mut self, n: usize) {
        self.0 = &self.0[..self.0.len() - n];
    }
}

/// An owned array and the bounds of its remaining values.
#[derive(Clone)]
struct Owned {
    store: ArrayStore,
    front: usize,
    back: usize,
}

impl ArraySource for Owned {
    #[inline]
    fn rest(&self) -> &[u16] {
        &self.store.as_slice()[self.front..self.back]
    }

    #[inline]
    fn consume_front(&mut self, n: usize) {
        debug_assert!(n <= self.back - self.front);
        self.front += n;
    }

    #[inline]
    fn consume_back(&mut self, n: usize) {
        debug_assert!(n <= self.back - self.front);
        self.back -= n;
    }
}

/// Length of the run of consecutive values at the start of the (non empty, strictly
/// increasing) slice, found by exponential search: `r[i] - r[0] == i` holds exactly on a prefix.
#[inline]
fn first_run_len(r: &[u16]) -> usize {
    let first = r[0] as usize;
    let in_run = |i: usize| r[i] as usize - first == i;
    // Invariant: `in_run(lo)`, and `hi == r.len()` or `!in_run(hi)`.
    let mut lo = 0;
    let mut step = 1;
    let mut hi;
    loop {
        hi = lo + step;
        if hi >= r.len() {
            hi = r.len();
            break;
        }
        if !in_run(hi) {
            break;
        }
        lo = hi;
        step *= 2;
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if in_run(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo + 1
}

/// Length of the run of consecutive values at the end of the (non empty, strictly increasing)
/// slice.
#[inline]
fn last_run_len(r: &[u16]) -> usize {
    let n = r.len();
    let last = r[n - 1] as usize;
    // `i` values before the end: `last - r[n - 1 - i] == i` holds exactly on a prefix of `i`.
    let in_run = |i: usize| last - r[n - 1 - i] as usize == i;
    let mut lo = 0;
    let mut step = 1;
    let mut hi;
    loop {
        hi = lo + step;
        if hi >= n {
            hi = n;
            break;
        }
        if !in_run(hi) {
            break;
        }
        lo = hi;
        step *= 2;
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if in_run(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo + 1
}

#[derive(Clone)]
struct ArrayIter<S>(S);

impl<S: ArraySource> ArrayIter<S> {
    #[inline]
    fn len(&self) -> usize {
        self.0.rest().len()
    }

    #[inline]
    fn next(&mut self) -> Option<u16> {
        let value = *self.0.rest().first()?;
        self.0.consume_front(1);
        Some(value)
    }

    #[inline]
    fn next_back(&mut self) -> Option<u16> {
        let value = *self.0.rest().last()?;
        self.0.consume_back(1);
        Some(value)
    }

    fn nth(&mut self, n: usize) -> Option<u16> {
        let rest = self.0.rest();
        match rest.get(n) {
            Some(&value) => {
                self.0.consume_front(n + 1);
                Some(value)
            }
            None => {
                let len = rest.len();
                self.0.consume_front(len);
                None
            }
        }
    }

    fn nth_back(&mut self, n: usize) -> Option<u16> {
        let rest = self.0.rest();
        let len = rest.len();
        if n < len {
            let value = rest[len - 1 - n];
            self.0.consume_back(n + 1);
            Some(value)
        } else {
            self.0.consume_back(len);
            None
        }
    }

    fn advance_to(&mut self, n: u16) {
        let skip = self.0.rest().partition_point(|&v| v < n);
        self.0.consume_front(skip);
    }

    fn advance_back_to(&mut self, n: u16) {
        let rest = self.0.rest();
        let keep = rest.partition_point(|&v| v <= n);
        let drop = rest.len() - keep;
        self.0.consume_back(drop);
    }

    fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
        let rest = self.0.rest();
        let first = *rest.first()?;
        let len = first_run_len(rest);
        self.0.consume_front(len);
        Some(first..=first + (len - 1) as u16)
    }

    fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
        let rest = self.0.rest();
        let last = *rest.last()?;
        let len = last_run_len(rest);
        self.0.consume_back(len);
        Some(last - (len - 1) as u16..=last)
    }

    #[inline]
    fn peek(&self) -> Option<u16> {
        self.0.rest().first().copied()
    }

    #[inline]
    fn peek_back(&self) -> Option<u16> {
        self.0.rest().last().copied()
    }

    fn fold<B, F: FnMut(B, u16) -> B>(self, init: B, mut f: F) -> B {
        self.0.rest().iter().fold(init, |acc, &v| f(acc, v))
    }

    fn rfold<B, F: FnMut(B, u16) -> B>(self, init: B, mut f: F) -> B {
        self.0.rest().iter().rev().fold(init, |acc, &v| f(acc, v))
    }
}

// ---------------------------------------------------------------------------------------------
// Bitmaps
// ---------------------------------------------------------------------------------------------

/// Borrowed or owned bitmap words.
trait WordsSource {
    fn words(&self) -> &Words;
}

impl WordsSource for &Words {
    #[inline]
    fn words(&self) -> &Words {
        self
    }
}

impl WordsSource for BitmapStore {
    #[inline]
    fn words(&self) -> &Words {
        self.as_words()
    }
}

const LAST_WORD: u16 = BITMAP_WORDS as u16 - 1;

/// Iterator over the set bits of a bitmap.
///
/// Invariants: `front_key <= back_key`. While `front_key < back_key`, `front_word` and
/// `back_word` hold the remaining bits of the words `front_key` and `back_key` and every word
/// strictly between them is untouched. Once `front_key == back_key` the remaining bits of that
/// shared word are in `front_word` and `back_word` is zero. `len` is the number of remaining
/// values.
#[derive(Clone)]
struct BitmapIter<B> {
    bits: B,
    front_key: u16,
    back_key: u16,
    front_word: u64,
    back_word: u64,
    len: u32,
}

impl<B: WordsSource> BitmapIter<B> {
    fn new(bits: B, len: u64) -> Self {
        let words = bits.words();
        let (front_word, back_word) = (words[0], words[BITMAP_WORDS - 1]);
        BitmapIter {
            bits,
            front_key: 0,
            back_key: LAST_WORD,
            front_word,
            back_word,
            len: len as u32,
        }
    }

    #[inline]
    fn load(&self, key: u16) -> u64 {
        self.bits.words()[key as usize]
    }

    /// Sum of the popcounts of the untouched words `lo..hi`.
    #[inline]
    fn count_words(&self, lo: u16, hi: u16) -> u32 {
        popcount_words(&self.bits.words()[lo as usize..hi as usize]) as u32
    }

    #[inline]
    fn len(&self) -> usize {
        self.len as usize
    }

    fn exhaust(&mut self) {
        self.len = 0;
        self.front_word = 0;
        self.back_word = 0;
        self.front_key = self.back_key;
    }

    /// Moves the front to the next word (`front_key < back_key` must hold).
    #[inline]
    fn step_front(&mut self) {
        debug_assert!(self.front_key < self.back_key);
        self.front_key += 1;
        self.front_word = if self.front_key == self.back_key {
            core::mem::take(&mut self.back_word)
        } else {
            self.load(self.front_key)
        };
    }

    /// Moves the back to the previous word (`front_key < back_key` must hold).
    #[inline]
    fn step_back(&mut self) {
        debug_assert!(self.front_key < self.back_key);
        self.back_key -= 1;
        self.back_word = if self.back_key == self.front_key {
            0
        } else {
            self.load(self.back_key)
        };
    }

    /// Makes `front_word` non zero (there must be remaining values).
    #[inline]
    fn seek_front(&mut self) {
        debug_assert!(self.len > 0);
        while self.front_word == 0 {
            self.step_front();
        }
    }

    /// Moves the back to the word holding the biggest remaining value (there must be remaining
    /// values).
    #[inline]
    fn seek_back(&mut self) {
        debug_assert!(self.len > 0);
        while self.front_key < self.back_key && self.back_word == 0 {
            self.step_back();
        }
    }

    /// The remaining bits of the back word: `front_word` once the keys met, `back_word`
    /// otherwise.
    #[inline]
    fn back_word_mut(&mut self) -> &mut u64 {
        if self.front_key == self.back_key {
            &mut self.front_word
        } else {
            &mut self.back_word
        }
    }

    #[inline]
    fn next(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        self.seek_front();
        let bit = self.front_word.trailing_zeros() as u16;
        self.front_word &= self.front_word - 1;
        self.len -= 1;
        Some(self.front_key * 64 + bit)
    }

    #[inline]
    fn next_back(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        self.seek_back();
        let word = self.back_word_mut();
        let bit = 63 - word.leading_zeros() as u16;
        *word ^= 1 << bit;
        self.len -= 1;
        Some(self.back_key * 64 + bit)
    }

    fn nth(&mut self, n: usize) -> Option<u16> {
        if n >= self.len() {
            self.exhaust();
            return None;
        }
        let mut n = n as u32;
        loop {
            let count = self.front_word.count_ones();
            if n < count {
                let bit = select_in_word(self.front_word, n);
                // Drop the bits up to and including the returned one.
                self.front_word &= (u64::MAX << bit) << 1;
                self.len -= n + 1;
                return Some(self.front_key * 64 + bit as u16);
            }
            n -= count;
            self.len -= count;
            self.step_front();
        }
    }

    fn nth_back(&mut self, n: usize) -> Option<u16> {
        if n >= self.len() {
            self.exhaust();
            return None;
        }
        let mut n = n as u32;
        loop {
            let shared = self.front_key == self.back_key;
            let word = if shared {
                self.front_word
            } else {
                self.back_word
            };
            let count = word.count_ones();
            if n < count {
                let bit = select_in_word(word, count - 1 - n);
                // Drop the bits from the returned one upwards.
                let kept = word & !(u64::MAX << bit);
                if shared {
                    self.front_word = kept;
                } else {
                    self.back_word = kept;
                }
                self.len -= n + 1;
                return Some(self.back_key * 64 + bit as u16);
            }
            n -= count;
            self.len -= count;
            self.back_word = 0;
            self.step_back();
        }
    }

    fn advance_to(&mut self, n: u16) {
        if self.len == 0 {
            return;
        }
        let key = n / 64;
        let low = (1u64 << (n % 64)) - 1;
        if key < self.front_key {
            return;
        }
        if key == self.front_key {
            self.len -= (self.front_word & low).count_ones();
            self.front_word &= !low;
            return;
        }
        if key > self.back_key {
            self.exhaust();
            return;
        }
        if key == self.back_key {
            self.front_word = self.back_word & !low;
            self.back_word = 0;
            self.front_key = key;
            self.len = self.front_word.count_ones();
            return;
        }
        // front_key < key < back_key: count whichever side has fewer whole words.
        let word = self.load(key);
        if key - self.front_key <= self.back_key - key {
            let dropped = self.front_word.count_ones()
                + self.count_words(self.front_key + 1, key)
                + (word & low).count_ones();
            self.len -= dropped;
        } else {
            self.len = (word & !low).count_ones()
                + self.count_words(key + 1, self.back_key)
                + self.back_word.count_ones();
        }
        self.front_key = key;
        self.front_word = word & !low;
    }

    fn advance_back_to(&mut self, n: u16) {
        if self.len == 0 {
            return;
        }
        let key = n / 64;
        // Bits above `n` inside its word.
        let high = (u64::MAX << (n % 64)) << 1;
        if key > self.back_key {
            return;
        }
        if key == self.back_key {
            let word = self.back_word_mut();
            let dropped = (*word & high).count_ones();
            *word &= !high;
            self.len -= dropped;
            return;
        }
        if key < self.front_key {
            self.exhaust();
            return;
        }
        if key == self.front_key {
            self.front_word &= !high;
            self.back_word = 0;
            self.back_key = key;
            self.len = self.front_word.count_ones();
            return;
        }
        // front_key < key < back_key.
        let word = self.load(key);
        if self.back_key - key <= key - self.front_key {
            let dropped = self.back_word.count_ones()
                + self.count_words(key + 1, self.back_key)
                + (word & high).count_ones();
            self.len -= dropped;
        } else {
            self.len = self.front_word.count_ones()
                + self.count_words(self.front_key + 1, key)
                + (word & !high).count_ones();
        }
        self.back_key = key;
        self.back_word = word & !high;
    }

    fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
        if self.len == 0 {
            return None;
        }
        self.seek_front();
        let offset = self.front_word.trailing_zeros();
        let start = self.front_key as u32 * 64 + offset;
        let ones = (self.front_word >> offset).trailing_ones();
        self.len -= ones;
        if offset + ones < 64 {
            self.front_word &= u64::MAX << (offset + ones);
            return Some(start as u16..=(start + ones - 1) as u16);
        }
        // The run reaches the end of the word: continue through the following words.
        self.front_word = 0;
        let mut end = self.front_key as u32 * 64 + 63;
        while self.front_key < self.back_key {
            self.step_front();
            let ones = self.front_word.trailing_ones();
            end += ones;
            self.len -= ones;
            if ones < 64 {
                self.front_word &= u64::MAX << ones;
                break;
            }
            self.front_word = 0;
        }
        Some(start as u16..=end as u16)
    }

    fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
        if self.len == 0 {
            return None;
        }
        self.seek_back();
        let back_key = self.back_key;
        let word = self.back_word_mut();
        let offset = word.leading_zeros();
        let end = back_key as u32 * 64 + 63 - offset;
        let ones = (*word << offset).leading_ones();
        if offset + ones < 64 {
            *word &= u64::MAX >> (offset + ones);
            self.len -= ones;
            return Some((end + 1 - ones) as u16..=end as u16);
        }
        // The run reaches bit 0 of the word: continue through the preceding words.
        *word = 0;
        self.len -= ones;
        let mut start = back_key as u32 * 64;
        while self.front_key < self.back_key {
            self.step_back();
            let word = self.back_word_mut();
            let ones = word.leading_ones();
            *word = if ones < 64 {
                *word & (u64::MAX >> ones)
            } else {
                0
            };
            start -= ones;
            self.len -= ones;
            if ones < 64 {
                break;
            }
        }
        Some(start as u16..=end as u16)
    }

    fn peek(&self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let mut key = self.front_key;
        let mut word = self.front_word;
        while word == 0 {
            key += 1;
            word = if key == self.back_key {
                self.back_word
            } else {
                self.load(key)
            };
        }
        Some(key * 64 + word.trailing_zeros() as u16)
    }

    fn peek_back(&self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let mut key = self.back_key;
        let mut word = if self.front_key == self.back_key {
            self.front_word
        } else {
            self.back_word
        };
        while word == 0 {
            key -= 1;
            word = if key == self.front_key {
                self.front_word
            } else {
                self.load(key)
            };
        }
        Some(key * 64 + 63 - word.leading_zeros() as u16)
    }

    fn fold<A, F: FnMut(A, u16) -> A>(self, init: A, mut f: F) -> A {
        if self.len == 0 {
            return init;
        }
        let mut emit = |mut acc: A, key: u16, mut word: u64| {
            let base = key * 64;
            while word != 0 {
                acc = f(acc, base + word.trailing_zeros() as u16);
                word &= word - 1;
            }
            acc
        };
        let mut acc = emit(init, self.front_key, self.front_word);
        if self.front_key < self.back_key {
            let words = self.bits.words();
            for key in self.front_key + 1..self.back_key {
                acc = emit(acc, key, words[key as usize]);
            }
            acc = emit(acc, self.back_key, self.back_word);
        }
        acc
    }

    fn rfold<A, F: FnMut(A, u16) -> A>(self, init: A, mut f: F) -> A {
        if self.len == 0 {
            return init;
        }
        let mut emit = |mut acc: A, key: u16, mut word: u64| {
            let base = key * 64;
            while word != 0 {
                let bit = 63 - word.leading_zeros() as u16;
                acc = f(acc, base + bit);
                word ^= 1 << bit;
            }
            acc
        };
        let mut acc = init;
        if self.front_key < self.back_key {
            acc = emit(acc, self.back_key, self.back_word);
            let words = self.bits.words();
            for key in (self.front_key + 1..self.back_key).rev() {
                acc = emit(acc, key, words[key as usize]);
            }
        }
        emit(acc, self.front_key, self.front_word)
    }
}

// ---------------------------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------------------------

/// Borrowed or owned run intervals.
trait RunsSource {
    fn runs(&self) -> &[Interval];
}

impl RunsSource for &[Interval] {
    #[inline]
    fn runs(&self) -> &[Interval] {
        self
    }
}

impl RunsSource for RunStore {
    #[inline]
    fn runs(&self) -> &[Interval] {
        self.intervals()
    }
}

/// Iterator over the values of a run container.
///
/// While values remain (`len > 0`), the remaining values are `next_front..=` the end of interval
/// `front`, the intervals strictly between `front` and `back`, and the start of interval `back`
/// `..=next_back`; when `front == back` they are `next_front..=next_back`.
#[derive(Clone)]
struct RunIter<R> {
    runs: R,
    front: usize,
    back: usize,
    next_front: u16,
    next_back: u16,
    len: u32,
}

impl<R: RunsSource> RunIter<R> {
    fn new(runs: R) -> Self {
        let intervals = runs.runs();
        let len = intervals.iter().map(Interval::run_len).sum::<u64>() as u32;
        let (next_front, next_back) = match (intervals.first(), intervals.last()) {
            (Some(first), Some(last)) => (first.start(), last.end()),
            _ => (0, 0),
        };
        let back = intervals.len().saturating_sub(1);
        RunIter {
            runs,
            front: 0,
            back,
            next_front,
            next_back,
            len,
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.len as usize
    }

    fn exhaust(&mut self) {
        self.len = 0;
        self.front = self.back;
    }

    /// Number of values from `from` in interval `i` to `to` in interval `j` (`i <= j`).
    fn count_between(&self, i: usize, from: u16, j: usize, to: u16) -> u32 {
        if i == j {
            return (to - from) as u32 + 1;
        }
        let runs = self.runs.runs();
        let inner: u64 = runs[i + 1..j].iter().map(Interval::run_len).sum();
        (runs[i].end() - from) as u32 + 1 + inner as u32 + (to - runs[j].start()) as u32 + 1
    }

    #[inline]
    fn next(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let value = self.next_front;
        self.len -= 1;
        if self.len > 0 {
            if self.front < self.back && value == self.runs.runs()[self.front].end() {
                self.front += 1;
                self.next_front = self.runs.runs()[self.front].start();
            } else {
                self.next_front = value + 1;
            }
        }
        Some(value)
    }

    #[inline]
    fn next_back(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let value = self.next_back;
        self.len -= 1;
        if self.len > 0 {
            if self.front < self.back && value == self.runs.runs()[self.back].start() {
                self.back -= 1;
                self.next_back = self.runs.runs()[self.back].end();
            } else {
                self.next_back = value - 1;
            }
        }
        Some(value)
    }

    fn nth(&mut self, n: usize) -> Option<u16> {
        if n >= self.len() {
            self.exhaust();
            return None;
        }
        let mut n = n as u32;
        loop {
            let end = if self.front == self.back {
                self.next_back
            } else {
                self.runs.runs()[self.front].end()
            };
            let available = (end - self.next_front) as u32 + 1;
            if n < available {
                self.next_front += n as u16;
                self.len -= n;
                return self.next();
            }
            n -= available;
            self.len -= available;
            self.front += 1;
            self.next_front = self.runs.runs()[self.front].start();
        }
    }

    fn nth_back(&mut self, n: usize) -> Option<u16> {
        if n >= self.len() {
            self.exhaust();
            return None;
        }
        let mut n = n as u32;
        loop {
            let start = if self.front == self.back {
                self.next_front
            } else {
                self.runs.runs()[self.back].start()
            };
            let available = (self.next_back - start) as u32 + 1;
            if n < available {
                self.next_back -= n as u16;
                self.len -= n;
                return self.next_back();
            }
            n -= available;
            self.len -= available;
            self.back -= 1;
            self.next_back = self.runs.runs()[self.back].end();
        }
    }

    fn advance_to(&mut self, n: u16) {
        if self.len == 0 || n <= self.next_front {
            return;
        }
        if n > self.next_back {
            self.exhaust();
            return;
        }
        let runs = self.runs.runs();
        // First interval (from the front one) ending at or after `n`; at most `back` because
        // the back interval holds `next_back >= n`.
        let j = self.front + runs[self.front..=self.back].partition_point(|iv| iv.end() < n);
        let new_front = n.max(runs[j].start());
        if j - self.front <= self.back - j {
            // Values in `next_front..new_front`.
            let dropped = if j == self.front {
                (new_front - self.next_front) as u32
            } else {
                self.count_between(self.front, self.next_front, j, runs[j].start()) - 1
                    + (new_front - runs[j].start()) as u32
            };
            self.len -= dropped;
        } else {
            self.len = self.count_between(j, new_front, self.back, self.next_back);
        }
        self.front = j;
        self.next_front = new_front;
    }

    fn advance_back_to(&mut self, n: u16) {
        if self.len == 0 || n >= self.next_back {
            return;
        }
        if n < self.next_front {
            self.exhaust();
            return;
        }
        let runs = self.runs.runs();
        // Last interval (up to the back one) starting at or before `n`; at least `front`
        // because the front interval holds `next_front <= n`.
        let j = self.front + runs[self.front..=self.back].partition_point(|iv| iv.start() <= n) - 1;
        let new_back = n.min(runs[j].end());
        if self.back - j <= j - self.front {
            // Values in `new_back + 1..=next_back`.
            let dropped = if j == self.back {
                (self.next_back - new_back) as u32
            } else {
                self.count_between(j, runs[j].end(), self.back, self.next_back) - 1
                    + (runs[j].end() - new_back) as u32
            };
            self.len -= dropped;
        } else {
            self.len = self.count_between(self.front, self.next_front, j, new_back);
        }
        self.back = j;
        self.next_back = new_back;
    }

    fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
        if self.len == 0 {
            return None;
        }
        if self.front == self.back {
            let range = self.next_front..=self.next_back;
            self.len = 0;
            return Some(range);
        }
        let end = self.runs.runs()[self.front].end();
        let range = self.next_front..=end;
        self.len -= (end - self.next_front) as u32 + 1;
        self.front += 1;
        self.next_front = self.runs.runs()[self.front].start();
        Some(range)
    }

    fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
        if self.len == 0 {
            return None;
        }
        if self.front == self.back {
            let range = self.next_front..=self.next_back;
            self.len = 0;
            return Some(range);
        }
        let start = self.runs.runs()[self.back].start();
        let range = start..=self.next_back;
        self.len -= (self.next_back - start) as u32 + 1;
        self.back -= 1;
        self.next_back = self.runs.runs()[self.back].end();
        Some(range)
    }

    #[inline]
    fn peek(&self) -> Option<u16> {
        (self.len > 0).then_some(self.next_front)
    }

    #[inline]
    fn peek_back(&self) -> Option<u16> {
        (self.len > 0).then_some(self.next_back)
    }

    fn fold<A, F: FnMut(A, u16) -> A>(self, init: A, mut f: F) -> A {
        if self.len == 0 {
            return init;
        }
        if self.front == self.back {
            return (self.next_front..=self.next_back).fold(init, f);
        }
        let runs = self.runs.runs();
        let mut acc = (self.next_front..=runs[self.front].end()).fold(init, &mut f);
        for iv in &runs[self.front + 1..self.back] {
            acc = (iv.start()..=iv.end()).fold(acc, &mut f);
        }
        (runs[self.back].start()..=self.next_back).fold(acc, f)
    }

    fn rfold<A, F: FnMut(A, u16) -> A>(self, init: A, mut f: F) -> A {
        if self.len == 0 {
            return init;
        }
        if self.front == self.back {
            return (self.next_front..=self.next_back).rev().fold(init, f);
        }
        let runs = self.runs.runs();
        let mut acc = (runs[self.back].start()..=self.next_back)
            .rev()
            .fold(init, &mut f);
        for iv in runs[self.front + 1..self.back].iter().rev() {
            acc = (iv.start()..=iv.end()).rev().fold(acc, &mut f);
        }
        (self.next_front..=runs[self.front].end())
            .rev()
            .fold(acc, f)
    }
}

// ---------------------------------------------------------------------------------------------
// Public (crate) iterators
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
enum Inner<A, B, R> {
    Array(ArrayIter<A>),
    Bitmap(BitmapIter<B>),
    Run(RunIter<R>),
}

/// Forwards a method call to the representation-specific iterator.
macro_rules! forward {
    ($self:expr, $it:ident => $call:expr) => {
        match $self {
            Inner::Array($it) => $call,
            Inner::Bitmap($it) => $call,
            Inner::Run($it) => $call,
        }
    };
}

/// Borrowing container iterator.
#[derive(Clone)]
pub(crate) struct Iter<'a>(Inner<Borrowed<'a>, &'a Words, &'a [Interval]>);

/// Owning container iterator.
#[derive(Clone)]
pub(crate) struct IntoIter(Inner<Owned, BitmapStore, RunStore>);

impl<'a> Iter<'a> {
    pub(crate) fn from_array(values: &'a [u16]) -> Iter<'a> {
        Iter(Inner::Array(ArrayIter(Borrowed(values))))
    }

    /// `len` must be the exact cardinality of the bitmap.
    pub(crate) fn from_bitmap(words: &'a Words, len: u64) -> Iter<'a> {
        Iter(Inner::Bitmap(BitmapIter::new(words, len)))
    }

    pub(crate) fn from_runs(runs: &'a [Interval]) -> Iter<'a> {
        Iter(Inner::Run(RunIter::new(runs)))
    }
}

impl IntoIter {
    pub(crate) fn from_array(store: ArrayStore) -> IntoIter {
        let back = store.as_slice().len();
        IntoIter(Inner::Array(ArrayIter(Owned {
            store,
            front: 0,
            back,
        })))
    }

    /// The cached cardinality of `bitmap` must be exact.
    pub(crate) fn from_bitmap(bitmap: BitmapStore) -> IntoIter {
        let len = bitmap.len();
        IntoIter(Inner::Bitmap(BitmapIter::new(bitmap, len)))
    }

    pub(crate) fn from_runs(runs: RunStore) -> IntoIter {
        IntoIter(Inner::Run(RunIter::new(runs)))
    }
}

macro_rules! iter_api {
    ($ty:ty) => {
        #[allow(dead_code)]
        impl $ty {
            pub(crate) fn advance_to(&mut self, n: u16) {
                forward!(&mut self.0, it => it.advance_to(n))
            }

            pub(crate) fn advance_back_to(&mut self, n: u16) {
                forward!(&mut self.0, it => it.advance_back_to(n))
            }

            pub(crate) fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
                forward!(&mut self.0, it => it.next_range())
            }

            pub(crate) fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
                forward!(&mut self.0, it => it.next_range_back())
            }

            pub(crate) fn peek(&self) -> Option<u16> {
                forward!(&self.0, it => it.peek())
            }

            pub(crate) fn peek_back(&self) -> Option<u16> {
                forward!(&self.0, it => it.peek_back())
            }
        }

        impl Iterator for $ty {
            type Item = u16;

            #[inline]
            fn next(&mut self) -> Option<u16> {
                forward!(&mut self.0, it => it.next())
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                let len = forward!(&self.0, it => it.len());
                (len, Some(len))
            }

            #[inline]
            fn count(self) -> usize {
                forward!(&self.0, it => it.len())
            }

            #[inline]
            fn last(mut self) -> Option<u16> {
                self.next_back()
            }

            fn nth(&mut self, n: usize) -> Option<u16> {
                forward!(&mut self.0, it => it.nth(n))
            }

            fn fold<B, F: FnMut(B, u16) -> B>(self, init: B, f: F) -> B {
                forward!(self.0, it => it.fold(init, f))
            }

            #[inline]
            fn min(mut self) -> Option<u16> {
                self.next()
            }

            #[inline]
            fn max(mut self) -> Option<u16> {
                self.next_back()
            }
        }

        impl DoubleEndedIterator for $ty {
            #[inline]
            fn next_back(&mut self) -> Option<u16> {
                forward!(&mut self.0, it => it.next_back())
            }

            fn nth_back(&mut self, n: usize) -> Option<u16> {
                forward!(&mut self.0, it => it.nth_back(n))
            }

            fn rfold<B, F: FnMut(B, u16) -> B>(self, init: B, f: F) -> B {
                forward!(self.0, it => it.rfold(init, f))
            }
        }

        impl ExactSizeIterator for $ty {
            #[inline]
            fn len(&self) -> usize {
                forward!(&self.0, it => it.len())
            }
        }

        impl core::iter::FusedIterator for $ty {}
    };
}

iter_api!(Iter<'_>);
iter_api!(IntoIter);

#[cfg(test)]
pub(crate) mod tests {
    use alloc::vec::Vec;
    use core::ops::RangeInclusive;

    use super::*;
    use crate::container::tests::{Model, Rng, cases, intervals_of, store_model};

    /// The container iterator API, so that both iterators can be tested generically.
    pub(crate) trait Walk:
        DoubleEndedIterator<Item = u16> + ExactSizeIterator + Clone
    {
        fn advance_to(&mut self, n: u16);
        fn advance_back_to(&mut self, n: u16);
        fn next_range(&mut self) -> Option<RangeInclusive<u16>>;
        fn next_range_back(&mut self) -> Option<RangeInclusive<u16>>;
        fn peek(&self) -> Option<u16>;
        fn peek_back(&self) -> Option<u16>;
    }

    macro_rules! impl_walk {
        ($ty:ty) => {
            impl Walk for $ty {
                fn advance_to(&mut self, n: u16) {
                    <$ty>::advance_to(self, n)
                }
                fn advance_back_to(&mut self, n: u16) {
                    <$ty>::advance_back_to(self, n)
                }
                fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
                    <$ty>::next_range(self)
                }
                fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
                    <$ty>::next_range_back(self)
                }
                fn peek(&self) -> Option<u16> {
                    <$ty>::peek(self)
                }
                fn peek_back(&self) -> Option<u16> {
                    <$ty>::peek_back(self)
                }
            }
        };
    }
    impl_walk!(Iter<'_>);
    impl_walk!(IntoIter);

    /// Reference iterator over a sorted slice.
    struct ModelIter<'a> {
        v: &'a [u16],
        front: usize,
        back: usize,
    }

    impl<'a> ModelIter<'a> {
        fn new(v: &'a [u16]) -> Self {
            ModelIter {
                v,
                front: 0,
                back: v.len(),
            }
        }

        fn rest(&self) -> &'a [u16] {
            &self.v[self.front..self.back]
        }

        fn len(&self) -> usize {
            self.back - self.front
        }

        fn next(&mut self) -> Option<u16> {
            (self.front < self.back).then(|| {
                self.front += 1;
                self.v[self.front - 1]
            })
        }

        fn next_back(&mut self) -> Option<u16> {
            (self.front < self.back).then(|| {
                self.back -= 1;
                self.v[self.back]
            })
        }

        fn nth(&mut self, n: usize) -> Option<u16> {
            if n >= self.len() {
                self.front = self.back;
                return None;
            }
            self.front += n;
            self.next()
        }

        fn nth_back(&mut self, n: usize) -> Option<u16> {
            if n >= self.len() {
                self.back = self.front;
                return None;
            }
            self.back -= n;
            self.next_back()
        }

        fn advance_to(&mut self, n: u16) {
            while self.front < self.back && self.v[self.front] < n {
                self.front += 1;
            }
        }

        fn advance_back_to(&mut self, n: u16) {
            while self.back > self.front && self.v[self.back - 1] > n {
                self.back -= 1;
            }
        }

        fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
            let start = self.next()?;
            let mut end = start;
            while self.front < self.back && self.v[self.front] as u32 == end as u32 + 1 {
                end = self.v[self.front];
                self.front += 1;
            }
            Some(start..=end)
        }

        fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
            let end = self.next_back()?;
            let mut start = end;
            while self.back > self.front && self.v[self.back - 1] as u32 + 1 == start as u32 {
                start = self.v[self.back - 1];
                self.back -= 1;
            }
            Some(start..=end)
        }
    }

    /// A seek target: near the remaining values, at the edges or uniform.
    fn target(rng: &mut Rng, m: &ModelIter<'_>) -> u16 {
        let rest = m.rest();
        match rng.below(5) {
            0 if !rest.is_empty() => {
                let v = rest[rng.below(rest.len() as u64) as usize];
                v.wrapping_add(rng.below(3) as u16).wrapping_sub(1)
            }
            1 if !rest.is_empty() => rest[0].wrapping_add(rng.below(130) as u16),
            2 if !rest.is_empty() => rest[rest.len() - 1].wrapping_sub(rng.below(130) as u16),
            3 => [0, 1, 63, 64, 65471, 65472, 65534, 65535][rng.below(8) as usize],
            _ => rng.u16(),
        }
    }

    /// A skip count for `nth`: small, around the remaining length or beyond it.
    fn skip(rng: &mut Rng, len: usize) -> usize {
        match rng.below(5) {
            0 => 0,
            1 => rng.below(70) as usize,
            2 => rng.below(len as u64 + 1) as usize,
            3 => len.saturating_sub(rng.below(3) as usize),
            _ => len + rng.below(3) as usize,
        }
    }

    fn check_rest<I: Walk>(it: &I, m: &ModelIter<'_>) {
        let rest = m.rest();
        assert_eq!(it.len(), rest.len());
        assert_eq!(it.size_hint(), (rest.len(), Some(rest.len())));
        assert_eq!(it.peek(), rest.first().copied());
        assert_eq!(it.peek_back(), rest.last().copied());
        assert!(it.clone().eq(rest.iter().copied()));
        assert!(it.clone().rev().eq(rest.iter().rev().copied()));
        assert_eq!(it.clone().count(), rest.len());
        assert_eq!(it.clone().last(), rest.last().copied());
        assert_eq!(it.clone().min(), rest.first().copied());
        assert_eq!(it.clone().max(), rest.last().copied());
        let folded = it.clone().fold(Vec::new(), |mut acc, v| {
            acc.push(v);
            acc
        });
        assert_eq!(folded, rest);
        let rfolded = it.clone().rfold(Vec::new(), |mut acc, v| {
            acc.push(v);
            acc
        });
        assert!(rfolded.iter().eq(rest.iter().rev()));
    }

    /// Random interleaving of every iterator operation, checked against a reference iterator.
    pub(crate) fn random_walk<I: Walk>(rng: &mut Rng, expected: &[u16], make: impl Fn() -> I) {
        for _ in 0..cases(6) {
            let mut it = make();
            let mut m = ModelIter::new(expected);
            check_rest(&it, &m);
            for _ in 0..cases(160) {
                match rng.below(12) {
                    0 | 1 => assert_eq!(it.next(), m.next()),
                    2 | 3 => assert_eq!(it.next_back(), m.next_back()),
                    4 => {
                        let n = skip(rng, m.len());
                        assert_eq!(it.nth(n), m.nth(n), "nth({n})");
                    }
                    5 => {
                        let n = skip(rng, m.len());
                        assert_eq!(it.nth_back(n), m.nth_back(n), "nth_back({n})");
                    }
                    6 => {
                        let n = target(rng, &m);
                        it.advance_to(n);
                        m.advance_to(n);
                    }
                    7 => {
                        let n = target(rng, &m);
                        it.advance_back_to(n);
                        m.advance_back_to(n);
                    }
                    8 => assert_eq!(it.next_range(), m.next_range()),
                    9 => assert_eq!(it.next_range_back(), m.next_range_back()),
                    _ => check_rest(&it, &m),
                }
                assert_eq!(it.len(), m.len());
                if m.len() == 0 && rng.chance(25) {
                    break;
                }
            }
            check_rest(&it, &m);
            // Fused: exhausted iterators stay exhausted.
            let mut it = it;
            while it.next().is_some() {}
            assert_eq!(it.next(), None);
            assert_eq!(it.next_back(), None);
            assert_eq!(it.next_range(), None);
            assert_eq!(it.next_range_back(), None);
            assert_eq!(it.peek(), None);
            assert_eq!(it.len(), 0);
        }
    }

    fn values_of(model: &Model) -> Vec<u16> {
        model.iter().copied().collect()
    }

    /// Shapes that stress word boundaries: full words, runs crossing words, isolated bits at
    /// both ends, the full container.
    fn special_sets() -> Vec<Vec<u16>> {
        let mut sets = alloc::vec![
            Vec::new(),
            alloc::vec![0],
            alloc::vec![65535],
            alloc::vec![0, 65535],
            (0..64).collect(),
            (63..129).collect(),
        ];
        if cfg!(miri) {
            // Large sets are too slow to build and walk under Miri; a sparse set still
            // crosses many words.
            sets.push((0..20000).step_by(63).collect());
        } else {
            sets.extend([
                (60..200).chain(300..301).collect(),
                (0..=u16::MAX).collect(),
                (0..=u16::MAX).filter(|v| v % 64 != 17).collect(),
                (65400..=u16::MAX).collect(),
                (0..=u16::MAX).step_by(63).collect(),
                (1..=u16::MAX).collect(),
                (0..u16::MAX).collect(),
            ]);
        }
        sets
    }

    fn all_sets(seed: u64) -> Vec<Vec<u16>> {
        let mut rng = Rng::new(seed);
        let mut sets = special_sets();
        for _ in 0..cases(150) {
            sets.push(values_of(&store_model(&mut rng)));
        }
        sets
    }

    #[test]
    fn array_iterators() {
        let mut rng = Rng::new(0x1001);
        for values in all_sets(0x1002) {
            if values.len() > 4096 {
                continue;
            }
            random_walk(&mut rng, &values, || Iter::from_array(&values));
            let inline_or_heap = ArrayStore::from_sorted_unchecked(&values);
            random_walk(&mut rng, &values, || {
                IntoIter::from_array(inline_or_heap.clone())
            });
            let heap = ArrayStore::heap_from_slice(&values);
            random_walk(&mut rng, &values, || IntoIter::from_array(heap.clone()));
        }
    }

    #[test]
    fn bitmap_iterators() {
        let mut rng = Rng::new(0x1003);
        for values in all_sets(0x1004) {
            let bitmap = BitmapStore::from_sorted_unchecked(&values);
            random_walk(&mut rng, &values, || {
                Iter::from_bitmap(bitmap.as_words(), bitmap.len())
            });
            random_walk(&mut rng, &values, || IntoIter::from_bitmap(bitmap.clone()));
        }
    }

    #[test]
    fn interval_slice_iterators() {
        let mut rng = Rng::new(0x1005);
        for values in all_sets(0x1006) {
            let intervals = intervals_of(&values);
            random_walk(&mut rng, &values, || Iter::from_runs(&intervals));
        }
    }

    #[test]
    fn contiguous_length_helpers() {
        assert_eq!(first_run_len(&[5]), 1);
        assert_eq!(first_run_len(&[5, 6, 7, 9]), 3);
        assert_eq!(first_run_len(&[5, 7]), 1);
        assert_eq!(last_run_len(&[5]), 1);
        assert_eq!(last_run_len(&[1, 5, 6, 7]), 3);
        assert_eq!(last_run_len(&[1, 5]), 1);
        let full: Vec<u16> = (0..=u16::MAX).collect();
        assert_eq!(first_run_len(&full), 65536);
        assert_eq!(last_run_len(&full), 65536);
        for split in [1, 2, 3, 100, 4095, 65535] {
            let mut v = full.clone();
            v.remove(split);
            assert_eq!(first_run_len(&v), split);
            // Removing the last value leaves a single run of 65535 values.
            let last = if split == 65535 { 65535 } else { 65535 - split };
            assert_eq!(last_run_len(&v), last);
        }
    }

    #[test]
    fn roaring_documented_semantics() {
        // The examples of the `roaring` documentation.
        let values = [1u16, 2, 4, 5];
        let bitmap = BitmapStore::from_sorted_unchecked(&values);
        let intervals = intervals_of(&values);
        let iters = [
            Iter::from_array(&values),
            Iter::from_bitmap(bitmap.as_words(), bitmap.len()),
            Iter::from_runs(&intervals),
        ];
        for it in iters {
            let mut a = it.clone();
            assert_eq!(a.next_range(), Some(1..=2));
            assert_eq!(a.next(), Some(4));
            assert_eq!(a.next_range(), Some(5..=5));
            assert_eq!(a.next_range(), None);
            let mut b = it.clone();
            assert_eq!(b.next_range_back(), Some(4..=5));
            assert_eq!(b.next_back(), Some(2));
            assert_eq!(b.next_range_back(), Some(1..=1));
            let mut c = it.clone();
            c.advance_to(2);
            assert_eq!(c.next(), Some(2));
            c.advance_to(0);
            assert_eq!(c.next(), Some(4));
            let mut d = it.clone();
            d.advance_back_to(4);
            assert_eq!(d.next_back(), Some(4));
            d.advance_back_to(65535);
            assert_eq!(d.next_back(), Some(2));
            assert_eq!(d.len(), 1);
            let mut e = it;
            e.advance_to(3);
            e.advance_back_to(3);
            assert_eq!(e.len(), 0);
            assert_eq!(e.next(), None);
        }
    }

    #[test]
    fn owned_array_iterator_keeps_the_allocation() {
        let values: Vec<u16> = (0..1000).map(|v| v * 3).collect();
        let store = ArrayStore::from_sorted_unchecked(&values);
        let ptr = store.as_slice().as_ptr();
        let it = IntoIter::from_array(store);
        let Inner::Array(ArrayIter(owned)) = &it.0 else {
            panic!("not an array iterator")
        };
        assert_eq!(owned.store.as_slice().as_ptr(), ptr);
        assert!(it.eq(values.iter().copied()));
    }
}
