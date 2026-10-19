//! Run-length encoded container: a sorted list of disjoint, non adjacent inclusive intervals.
//!
//! Besides the store itself this module owns every container-level operation in which at least
//! one operand is a run container (the `Container` methods in `mod.rs` delegate to the free
//! functions at the bottom of this file).
//!
//! Implementation notes:
//!
//! * Point and range updates locate the affected intervals with binary searches and edit the
//!   interval list in place.
//! * Run × run and run × array operations are single passes over interval streams (an array is
//!   viewed as the stream of its maximal runs of consecutive values), using galloping searches
//!   wherever one side can be skipped.
//! * Run × bitmap operations work on whole 64-bit words: an interval covers a masked first and
//!   last word and fully covered words in between, so fills, flips, clears, copies and masked
//!   popcounts touch only the covered words. These word kernels are compiled once per SIMD tier
//!   (AVX2 and AVX-512 on `x86_64`) and dispatched on [`crate::simd::backend`], so popcounts and
//!   fills use the widest instructions of the running CPU (NEON is the baseline on `aarch64`).

use core::mem::MaybeUninit;
use core::ops::{ControlFlow, RangeInclusive};

use super::array::ArrayStore;
use super::bitmap::BitmapStore;
use super::heap::HeapVec;
use super::{ARRAY_LIMIT, Container};
use crate::simd::scalar::{copy_tail, gallop};
use crate::simd::{self, BITMAP_WORDS, Words};

/// An inclusive interval `start..=end` with `start <= end`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub(crate) struct Interval {
    start: u16,
    end: u16,
}

#[allow(dead_code)]
impl Interval {
    /// `start <= end` must hold.
    #[inline]
    pub(crate) const fn new_unchecked(start: u16, end: u16) -> Interval {
        debug_assert!(start <= end);
        Interval { start, end }
    }

    #[inline]
    pub(crate) const fn start(&self) -> u16 {
        self.start
    }

    #[inline]
    pub(crate) const fn end(&self) -> u16 {
        self.end
    }

    /// Number of values in the interval (1..=65536).
    #[inline]
    pub(crate) const fn run_len(&self) -> u64 {
        self.end as u64 - self.start as u64 + 1
    }

    /// Interval from `u32` bounds (`start <= end <= u16::MAX`), used by the merge loops which
    /// compute `end + 1` without overflow.
    #[inline(always)]
    fn from_u32(start: u32, end: u32) -> Interval {
        debug_assert!(start <= end && end <= u16::MAX as u32);
        Interval {
            start: start as u16,
            end: end as u16,
        }
    }
}

/// A run container.
#[derive(Clone)]
pub(crate) struct RunStore(HeapVec<Interval>);

const _: () = assert!(core::mem::size_of::<RunStore>() == 8);

/// Serialized size of a run container with `runs` intervals, using the standard Roaring format
/// (a 2-byte run count followed by 4 bytes per run). Used for `optimize` decisions so that they
/// agree with other Roaring implementations.
pub(crate) const fn serialized_byte_size(runs: u64) -> usize {
    2 + 4 * runs as usize
}

#[allow(dead_code)]
impl RunStore {
    pub(crate) const fn new() -> RunStore {
        RunStore(HeapVec::new())
    }

    pub(crate) fn full() -> RunStore {
        RunStore::from_range(0..=u16::MAX)
    }

    /// The values of `range` as a single run (an empty store for an empty range).
    pub(crate) fn from_range(range: RangeInclusive<u16>) -> RunStore {
        if range.is_empty() {
            return RunStore::new();
        }
        let mut runs = HeapVec::with_capacity(1);
        runs.push(Interval::new_unchecked(*range.start(), *range.end()));
        RunStore(runs)
    }

    /// `intervals` must be sorted, disjoint and non adjacent.
    pub(crate) fn from_intervals_unchecked(intervals: &[Interval]) -> RunStore {
        debug_assert!(intervals.is_empty() || validate_intervals(intervals).is_ok());
        RunStore(HeapVec::from_slice(intervals))
    }

    /// Validating constructor used by deserialization: at least one interval, every interval
    /// non empty (`start <= end`), sorted, non overlapping and non adjacent.
    pub(crate) fn try_from_intervals(intervals: &[Interval]) -> Result<RunStore, &'static str> {
        validate_intervals(intervals)?;
        Ok(RunStore(HeapVec::from_slice(intervals)))
    }

    pub(crate) fn from_array(array: &ArrayStore) -> RunStore {
        let values = array.as_slice();
        let Some((&first, rest)) = values.split_first() else {
            return RunStore::new();
        };
        // Exact allocation: one run starts at every value that does not extend its predecessor.
        let runs = 1 + values
            .windows(2)
            .filter(|w| w[1] != w[0].wrapping_add(1))
            .count();
        let mut out = HeapVec::with_capacity(runs);
        let (mut start, mut prev) = (first, first);
        for &v in rest {
            if v != prev.wrapping_add(1) {
                out.push(Interval::new_unchecked(start, prev));
                start = v;
            }
            prev = v;
        }
        out.push(Interval::new_unchecked(start, prev));
        RunStore(out)
    }

    pub(crate) fn from_bitmap(bitmap: &BitmapStore) -> RunStore {
        let words = bitmap.as_words();
        let runs = simd::bitmap_count_runs(words) as usize;
        let mut out = HeapVec::with_capacity(runs);
        if runs != 0 {
            runs_from_words(words, &mut out);
        }
        debug_assert_eq!(out.len(), runs);
        RunStore(out)
    }

    pub(crate) fn intervals(&self) -> &[Interval] {
        self.0.as_slice()
    }

    /// Number of intervals.
    pub(crate) fn run_amount(&self) -> u64 {
        self.0.len() as u64
    }

    /// Cardinality (computed from the intervals, `O(runs)`).
    pub(crate) fn len(&self) -> u64 {
        sum_len(self.intervals())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn insert(&mut self, index: u16) -> bool {
        let ivs = self.0.as_mut_slice();
        // `ivs[..i]` end before `index`; `ivs[i]` (if any) ends at or after it.
        let i = ivs.partition_point(|iv| iv.end < index);
        let touches_next = match ivs.get(i) {
            Some(next) if next.start <= index => return false,
            // `index < next.start`, so `index + 1` does not overflow.
            Some(next) => next.start == index + 1,
            None => false,
        };
        // `prev.end < index`, so `prev.end + 1` does not overflow.
        let touches_prev = i > 0 && ivs[i - 1].end + 1 == index;
        match (touches_prev, touches_next) {
            (true, true) => {
                ivs[i - 1].end = ivs[i].end;
                self.0.remove(i);
            }
            (true, false) => ivs[i - 1].end = index,
            (false, true) => ivs[i].start = index,
            (false, false) => self.0.insert(i, Interval::new_unchecked(index, index)),
        }
        true
    }

    /// Inserts the values of `range`, returns the number of values that were not present.
    pub(crate) fn insert_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        if range.is_empty() {
            return 0;
        }
        let (s, e) = (*range.start() as u32, *range.end() as u32);
        let ivs = self.0.as_slice();
        // The intervals `lo..hi` overlap or touch `s..=e` and merge with it.
        let lo = ivs.partition_point(|iv| iv.end as u32 + 1 < s);
        let hi = lo + ivs[lo..].partition_point(|iv| iv.start as u32 <= e + 1);
        if lo == hi {
            self.0.insert(lo, Interval::from_u32(s, e));
            return (e - s + 1) as u64;
        }
        let start = s.min(ivs[lo].start as u32);
        let end = e.max(ivs[hi - 1].end as u32);
        let covered = sum_len(&ivs[lo..hi]);
        self.0.as_mut_slice()[lo] = Interval::from_u32(start, end);
        self.0.remove_range(lo + 1..hi);
        (end - start + 1) as u64 - covered
    }

    /// Appends `index` if it is greater than the current maximum (or the store is empty).
    pub(crate) fn push(&mut self, index: u16) -> bool {
        if let Some(last) = self.0.as_mut_slice().last_mut() {
            if last.end >= index {
                return false;
            }
            // `last.end < index`, so `last.end + 1` does not overflow.
            if last.end + 1 == index {
                last.end = index;
                return true;
            }
        }
        self.0.push(Interval::new_unchecked(index, index));
        true
    }

    pub(crate) fn remove(&mut self, index: u16) -> bool {
        let ivs = self.0.as_mut_slice();
        let i = ivs.partition_point(|iv| iv.end < index);
        let Some(iv) = ivs.get_mut(i) else {
            return false;
        };
        if iv.start > index {
            return false;
        }
        let Interval { start, end } = *iv;
        if start == end {
            self.0.remove(i);
        } else if index == start {
            iv.start = index + 1;
        } else if index == end {
            iv.end = index - 1;
        } else {
            iv.end = index - 1;
            self.0
                .insert(i + 1, Interval::new_unchecked(index + 1, end));
        }
        true
    }

    /// Removes the values of `range`, returns the number of values that were present.
    pub(crate) fn remove_range(&mut self, range: RangeInclusive<u16>) -> u64 {
        if range.is_empty() {
            return 0;
        }
        let (s, e) = (*range.start(), *range.end());
        let ivs = self.0.as_slice();
        // The intervals `lo..hi` intersect `s..=e`.
        let lo = ivs.partition_point(|iv| iv.end < s);
        let hi = lo + ivs[lo..].partition_point(|iv| iv.start <= e);
        if lo == hi {
            return 0;
        }
        let (first, last) = (ivs[lo], ivs[hi - 1]);
        let mut removed = sum_len(&ivs[lo..hi]);
        let mut pieces = [Interval::default(); 2];
        let mut kept = 0;
        if first.start < s {
            pieces[kept] = Interval::new_unchecked(first.start, s - 1);
            kept += 1;
            removed -= (s - first.start) as u64;
        }
        if last.end > e {
            pieces[kept] = Interval::new_unchecked(e + 1, last.end);
            kept += 1;
            removed -= (last.end - e) as u64;
        }
        self.0.splice(lo..hi, &pieces[..kept]);
        removed
    }

    /// Removes the `n` smallest values (all values if `n >= len()`).
    pub(crate) fn remove_smallest(&mut self, n: u64) {
        let ivs = self.0.as_mut_slice();
        let mut left = n;
        let mut drop = 0;
        while left > 0 && drop < ivs.len() {
            let len = ivs[drop].run_len();
            if len <= left {
                left -= len;
                drop += 1;
            } else {
                ivs[drop].start += left as u16;
                left = 0;
            }
        }
        self.0.remove_range(0..drop);
    }

    /// Removes the `n` biggest values (all values if `n >= len()`).
    pub(crate) fn remove_biggest(&mut self, n: u64) {
        let ivs = self.0.as_mut_slice();
        let mut left = n;
        let mut keep = ivs.len();
        while left > 0 && keep > 0 {
            let len = ivs[keep - 1].run_len();
            if len <= left {
                left -= len;
                keep -= 1;
            } else {
                ivs[keep - 1].end -= left as u16;
                left = 0;
            }
        }
        self.0.truncate(keep);
    }

    pub(crate) fn contains(&self, index: u16) -> bool {
        let ivs = self.intervals();
        let i = ivs.partition_point(|iv| iv.end < index);
        ivs.get(i).is_some_and(|iv| iv.start <= index)
    }

    /// Whether every value of `range` is present (`true` for an empty range).
    pub(crate) fn contains_range(&self, range: RangeInclusive<u16>) -> bool {
        if range.is_empty() {
            return true;
        }
        let (s, e) = (*range.start(), *range.end());
        let ivs = self.intervals();
        let i = ivs.partition_point(|iv| iv.end < s);
        ivs.get(i).is_some_and(|iv| iv.start <= s && e <= iv.end)
    }

    pub(crate) fn min(&self) -> Option<u16> {
        self.intervals().first().map(Interval::start)
    }

    pub(crate) fn max(&self) -> Option<u16> {
        self.intervals().last().map(Interval::end)
    }

    /// Number of values `<= index`.
    pub(crate) fn rank(&self, index: u16) -> u64 {
        let ivs = self.intervals();
        let full = ivs.partition_point(|iv| iv.end <= index);
        let mut rank = sum_len(&ivs[..full]);
        if let Some(iv) = ivs.get(full)
            && iv.start <= index
        {
            rank += (index - iv.start) as u64 + 1;
        }
        rank
    }

    /// The `n`-th smallest value (0 based).
    pub(crate) fn select(&self, n: u16) -> Option<u16> {
        let mut left = n as u64;
        for iv in self.intervals() {
            let len = iv.run_len();
            if left < len {
                return Some(iv.start + left as u16);
            }
            left -= len;
        }
        None
    }

    /// Number of values inside `range` (0 for an empty range).
    pub(crate) fn range_cardinality(&self, range: RangeInclusive<u16>) -> u64 {
        if range.is_empty() {
            return 0;
        }
        let (s, e) = (*range.start(), *range.end());
        let ivs = self.intervals();
        let lo = ivs.partition_point(|iv| iv.end < s);
        let hi = lo + ivs[lo..].partition_point(|iv| iv.start <= e);
        if lo == hi {
            return 0;
        }
        let mut n = sum_len(&ivs[lo..hi]);
        n -= s.saturating_sub(ivs[lo].start) as u64;
        n -= ivs[hi - 1].end.saturating_sub(e) as u64;
        n
    }

    /// The values as a sorted array (of any size; the caller decides the representation).
    pub(crate) fn to_array(&self) -> ArrayStore {
        let ivs = self.intervals();
        ArrayStore::build(self.len() as usize, |out| expand_runs(ivs, out))
    }

    pub(crate) fn to_bitmap(&self) -> BitmapStore {
        let mut bitmap = BitmapStore::new();
        set_words(self.intervals(), bitmap.as_words_mut());
        bitmap.set_len(self.len());
        bitmap
    }

    /// Sets every bit covered by the runs (does not touch any cached cardinality).
    pub(crate) fn or_into_words(&self, words: &mut Words) {
        set_words(self.intervals(), words);
    }

    /// Flips every bit covered by the runs (does not touch any cached cardinality).
    pub(crate) fn xor_into_words(&self, words: &mut Words) {
        flip_words(self.intervals(), words);
    }

    /// Serialized payload size of this container in the standard Roaring format
    /// (`2 + 4 * runs`), see [`serialized_byte_size`].
    pub(crate) fn byte_size(&self) -> usize {
        serialized_byte_size(self.run_amount())
    }

    /// Heap bytes owned by the store (allocation header + interval capacity, 0 without
    /// allocation). The 8-byte handle itself lives inline in the container slot.
    pub(crate) fn mem_size(&self) -> usize {
        self.0.heap_bytes()
    }

    /// Checks the store invariants. Like the `roaring` crate, an empty run container is
    /// reported as invalid (the bitmap never stores empty containers).
    pub(crate) fn internal_validate(&self) -> Result<(), &'static str> {
        validate_intervals(self.intervals())
    }
}

impl Default for RunStore {
    fn default() -> Self {
        RunStore::new()
    }
}

impl core::fmt::Debug for RunStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.intervals().iter().map(|iv| iv.start..=iv.end))
            .finish()
    }
}

fn validate_intervals(intervals: &[Interval]) -> Result<(), &'static str> {
    if intervals.is_empty() {
        return Err("run container with zero runs");
    }
    if intervals.iter().any(|iv| iv.start > iv.end) {
        return Err("run with start greater than end");
    }
    for pair in intervals.windows(2) {
        let (prev, next) = (pair[0], pair[1]);
        if prev.end >= next.start {
            return Err("overlapping or unordered runs");
        }
        // `prev.end < next.start`, so `prev.end + 1` does not overflow.
        if prev.end + 1 == next.start {
            return Err("adjacent runs");
        }
    }
    Ok(())
}

/// Total number of values of disjoint intervals.
#[inline]
fn sum_len(intervals: &[Interval]) -> u64 {
    // The intervals are disjoint, so the sum of `end - start` is at most 65535 and fits the
    // u32 lanes the compiler vectorizes this with.
    let spans: u32 = intervals.iter().map(|iv| (iv.end - iv.start) as u32).sum();
    spans as u64 + intervals.len() as u64
}

/// Writes the values of the runs in increasing order, returns their number.
fn expand_runs(intervals: &[Interval], out: &mut [MaybeUninit<u16>]) -> usize {
    let mut k = 0;
    for iv in intervals {
        let n = iv.run_len() as usize;
        for (i, slot) in out[k..k + n].iter_mut().enumerate() {
            slot.write(iv.start.wrapping_add(i as u16));
        }
        k += n;
    }
    k
}

/// Index of the first interval of `runs[from..]` with `end >= value` (exponential search).
#[inline]
fn gallop_end(runs: &[Interval], from: usize, value: u16) -> usize {
    let len = runs.len();
    if from >= len || runs[from].end >= value {
        return from;
    }
    // Invariant: runs[lo].end < value.
    let mut lo = from;
    let mut step = 1;
    let mut hi = from + 1;
    while hi < len && runs[hi].end < value {
        lo = hi;
        step <<= 1;
        hi = from + step;
    }
    let hi = hi.min(len);
    lo + 1 + runs[lo + 1..hi].partition_point(|iv| iv.end < value)
}

/// Length of the longest prefix of the (non empty, strictly increasing) `values` made of
/// consecutive integers, found by exponential search.
#[inline]
fn consecutive_prefix(values: &[u16]) -> usize {
    let first = values[0] as usize;
    // Monotone for strictly increasing values: once a gap appears it never closes.
    let in_run = |i: usize| values[i] as usize - first == i;
    let n = values.len();
    let mut lo = 0;
    let mut step = 1;
    let mut hi = 1;
    while hi < n && in_run(hi) {
        lo = hi;
        step <<= 1;
        hi = lo + step;
    }
    // in_run(lo) holds; the answer lies in lo + 1..=min(hi, n).
    let (mut l, mut h) = (lo + 1, hi.min(n));
    while l < h {
        let mid = l + (h - l) / 2;
        if in_run(mid) {
            l = mid + 1;
        } else {
            h = mid;
        }
    }
    l
}

/// The maximal runs of consecutive values of a sorted array, as a stream of intervals.
#[derive(Clone)]
struct ArrayRuns<'a>(&'a [u16]);

impl Iterator for ArrayRuns<'_> {
    type Item = Interval;

    #[inline]
    fn next(&mut self) -> Option<Interval> {
        let values = self.0;
        let &first = values.first()?;
        let n = consecutive_prefix(values);
        self.0 = &values[n..];
        Some(Interval::new_unchecked(first, first + (n - 1) as u16))
    }
}

/// Builds a canonical run list from intervals sorted by start, merging overlapping and
/// adjacent ones.
struct RunBuilder {
    out: HeapVec<Interval>,
    start: u32,
    end: u32,
    open: bool,
}

impl RunBuilder {
    fn with_capacity(capacity: usize) -> RunBuilder {
        RunBuilder {
            out: HeapVec::with_capacity(capacity),
            start: 0,
            end: 0,
            open: false,
        }
    }

    #[inline(always)]
    fn add(&mut self, start: u32, end: u32) {
        debug_assert!(start <= end && end <= u16::MAX as u32);
        debug_assert!(!self.open || start >= self.start);
        if self.open && start <= self.end + 1 {
            self.end = self.end.max(end);
            return;
        }
        if self.open {
            self.out.push(Interval::from_u32(self.start, self.end));
        }
        self.start = start;
        self.end = end;
        self.open = true;
    }

    fn finish(mut self) -> RunStore {
        if self.open {
            self.out.push(Interval::from_u32(self.start, self.end));
        }
        if self.out.len() < self.out.capacity() / 2 {
            self.out.shrink_to_fit();
        }
        RunStore(self.out)
    }
}

/// Interval boundaries of a canonical interval stream as the strictly increasing sequence
/// `start0, end0 + 1, start1, end1 + 1, ...` (as `u32`, so `end + 1` may be 65536).
struct Boundaries<I> {
    intervals: I,
    pending_end: Option<u32>,
}

impl<I: Iterator<Item = Interval>> Iterator for Boundaries<I> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        if let Some(end) = self.pending_end.take() {
            return Some(end);
        }
        let iv = self.intervals.next()?;
        self.pending_end = Some(iv.end as u32 + 1);
        Some(iv.start as u32)
    }
}

fn union_streams(
    mut a: impl Iterator<Item = Interval>,
    mut b: impl Iterator<Item = Interval>,
    capacity: usize,
) -> RunStore {
    let mut out = RunBuilder::with_capacity(capacity);
    let (mut x, mut y) = (a.next(), b.next());
    loop {
        let iv = match (x, y) {
            (Some(p), Some(q)) => {
                if p.start <= q.start {
                    x = a.next();
                    p
                } else {
                    y = b.next();
                    q
                }
            }
            (Some(p), None) => {
                x = a.next();
                p
            }
            (None, Some(q)) => {
                y = b.next();
                q
            }
            (None, None) => break,
        };
        out.add(iv.start as u32, iv.end as u32);
    }
    out.finish()
}

/// `a - b`.
fn difference_streams(
    a: impl Iterator<Item = Interval>,
    mut b: impl Iterator<Item = Interval>,
    capacity: usize,
) -> RunStore {
    let mut out = RunBuilder::with_capacity(capacity);
    let mut y = b.next();
    for p in a {
        let (mut s, e) = (p.start as u32, p.end as u32);
        while let Some(q) = y {
            let (qs, qe) = (q.start as u32, q.end as u32);
            if qe < s {
                y = b.next();
                continue;
            }
            if qs > e {
                break;
            }
            if qs > s {
                out.add(s, qs - 1);
            }
            if qe >= e {
                // `q` covers the rest of `p` and may overlap the next interval of `a`.
                s = e + 1;
                break;
            }
            s = qe + 1;
            y = b.next();
        }
        if s <= e {
            out.add(s, e);
        }
    }
    out.finish()
}

/// `a ^ b`: membership toggles at every interval boundary of either stream; boundaries shared
/// by both streams cancel out.
fn symmetric_difference_streams(
    a: impl Iterator<Item = Interval>,
    b: impl Iterator<Item = Interval>,
    capacity: usize,
) -> RunStore {
    let mut out = RunBuilder::with_capacity(capacity);
    let mut ba = Boundaries {
        intervals: a,
        pending_end: None,
    };
    let mut bb = Boundaries {
        intervals: b,
        pending_end: None,
    };
    let (mut x, mut y) = (ba.next(), bb.next());
    let mut open: Option<u32> = None;
    loop {
        let p = match (x, y) {
            (Some(p), Some(q)) if p == q => {
                x = ba.next();
                y = bb.next();
                continue;
            }
            (Some(p), Some(q)) => {
                if p < q {
                    x = ba.next();
                    p
                } else {
                    y = bb.next();
                    q
                }
            }
            (Some(p), None) => {
                x = ba.next();
                p
            }
            (None, Some(q)) => {
                y = bb.next();
                q
            }
            (None, None) => break,
        };
        match open.take() {
            None => open = Some(p),
            Some(start) => {
                if p > start {
                    out.add(start, p - 1);
                }
            }
        }
    }
    debug_assert!(open.is_none());
    out.finish()
}

/// Walks two run lists over their overlapping interval pairs, skipping non overlapping parts
/// with galloping searches. `f` receives each overlap `(start, end)`.
#[inline(always)]
fn for_each_overlap<B>(
    a: &[Interval],
    b: &[Interval],
    mut f: impl FnMut(u16, u16) -> ControlFlow<B>,
) -> ControlFlow<B> {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (p, q) = (a[i], b[j]);
        if p.end < q.start {
            i = gallop_end(a, i + 1, q.start);
            continue;
        }
        if q.end < p.start {
            j = gallop_end(b, j + 1, p.start);
            continue;
        }
        f(p.start.max(q.start), p.end.min(q.end))?;
        if p.end <= q.end {
            i += 1;
        }
        if q.end <= p.end {
            j += 1;
        }
    }
    ControlFlow::Continue(())
}

fn intersection_runs(a: &[Interval], b: &[Interval]) -> RunStore {
    let mut out = RunBuilder::with_capacity(a.len().min(b.len()));
    let _ = for_each_overlap(a, b, |s, e| {
        out.add(s as u32, e as u32);
        ControlFlow::<()>::Continue(())
    });
    out.finish()
}

fn intersection_len_runs(a: &[Interval], b: &[Interval]) -> u64 {
    let mut n = 0;
    let _ = for_each_overlap(a, b, |s, e| {
        n += (e - s) as u64 + 1;
        ControlFlow::<()>::Continue(())
    });
    n
}

fn is_disjoint_runs(a: &[Interval], b: &[Interval]) -> bool {
    for_each_overlap(a, b, |_, _| ControlFlow::Break(())).is_continue()
}

/// Whether `a ⊆ b`.
fn is_subset_runs(a: &[Interval], b: &[Interval]) -> bool {
    let mut j = 0;
    for p in a {
        j = gallop_end(b, j, p.start);
        match b.get(j) {
            Some(q) if q.start <= p.start && p.end <= q.end => {}
            _ => return false,
        }
    }
    true
}

/// Splits the sorted `values` into consecutive segments that lie entirely inside (`true`) or
/// entirely outside (`false`) the runs and passes them to `f` in order. Both sides are skipped
/// with galloping searches, so the cost adapts to the smaller of the two inputs.
#[inline(always)]
fn walk_array<B>(
    values: &[u16],
    runs: &[Interval],
    mut f: impl FnMut(bool, &[u16]) -> ControlFlow<B>,
) -> ControlFlow<B> {
    let n = values.len();
    let (mut i, mut j) = (0, 0);
    while i < n {
        j = gallop_end(runs, j, values[i]);
        let Some(&iv) = runs.get(j) else {
            return f(false, &values[i..]);
        };
        if values[i] < iv.start {
            let k = gallop(values, i, iv.start);
            f(false, &values[i..k])?;
            i = k;
        } else {
            // iv.start <= values[i] <= iv.end
            let k = if iv.end == u16::MAX {
                n
            } else {
                gallop(values, i, iv.end + 1)
            };
            f(true, &values[i..k])?;
            i = k;
            j += 1;
        }
    }
    ControlFlow::Continue(())
}

/// The values of the array that are inside (`inside == true`) or outside the runs.
fn filter_array(values: &[u16], runs: &[Interval], inside: bool) -> ArrayStore {
    ArrayStore::build(values.len(), |out| {
        let mut k = 0;
        let _ = walk_array(values, runs, |is_inside, segment| {
            if is_inside == inside {
                k = copy_tail(segment, out, k);
            }
            ControlFlow::<()>::Continue(())
        });
        k
    })
}

fn intersection_len_array(values: &[u16], runs: &[Interval]) -> u64 {
    let mut n = 0;
    let _ = walk_array(values, runs, |inside, segment| {
        if inside {
            n += segment.len() as u64;
        }
        ControlFlow::<()>::Continue(())
    });
    n
}

/// Whether some value of the array is inside (`inside == true`) or outside the runs.
fn array_has(values: &[u16], runs: &[Interval], inside: bool) -> bool {
    walk_array(values, runs, |is_inside, _| {
        if is_inside == inside {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .is_break()
}

/// Whether every value of the runs is in the array.
fn runs_subset_of_array(runs: &[Interval], values: &[u16]) -> bool {
    let mut i = 0;
    for iv in runs {
        i = gallop(values, i, iv.start);
        // Strictly increasing values: `start` at `i` and `end` at `i + end - start` means that
        // every value in between is present.
        let last = i + (iv.end - iv.start) as usize;
        if last >= values.len() || values[i] != iv.start || values[last] != iv.end {
            return false;
        }
        i = last + 1;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Word kernels (run × bitmap). Two intervals may share their boundary word, so kernels that
// write into a zeroed destination accumulate the edge words with `|=`.
// ---------------------------------------------------------------------------------------------

/// The words covered by an interval: `first..=last`, with the masks of the edge words
/// (`first_mask` already includes `last_mask` when `first == last`).
#[derive(Clone, Copy)]
struct Span {
    first: usize,
    last: usize,
    first_mask: u64,
    last_mask: u64,
}

impl Span {
    #[inline(always)]
    fn new(iv: Interval) -> Span {
        let (s, e) = (iv.start as usize, iv.end as usize);
        let (first, last) = (s / 64, e / 64);
        let first_mask = u64::MAX << (s % 64);
        let last_mask = u64::MAX >> (63 - e % 64);
        Span {
            first,
            last,
            first_mask: if first == last {
                first_mask & last_mask
            } else {
                first_mask
            },
            last_mask,
        }
    }

    #[inline(always)]
    fn single(&self) -> bool {
        self.first == self.last
    }

    /// The fully covered words between the edge words.
    #[inline(always)]
    fn middle(&self) -> core::ops::Range<usize> {
        self.first + 1..self.last
    }
}

#[inline(always)]
fn popcount(words: &[u64]) -> u64 {
    words.iter().map(|w| w.count_ones() as u64).sum()
}

/// Writes the indices of the set bits of `bits` (word `word`) from `out[k..]`.
#[inline(always)]
fn extract_bits(mut bits: u64, word: usize, out: &mut [MaybeUninit<u16>], mut k: usize) -> usize {
    let base = (word * 64) as u16;
    while bits != 0 {
        out[k].write(base + bits.trailing_zeros() as u16);
        k += 1;
        bits &= bits - 1;
    }
    k
}

/// Defines word kernels once and instantiates them for every SIMD tier of the target: the
/// wrapper dispatches on [`simd::backend`] (`x86_64`) so that popcounts, masks and fills are
/// compiled with the instructions of the running CPU. Miri and `aarch64` (NEON baseline) use
/// the plain instantiation.
macro_rules! word_kernels {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block
    )*) => {$(
        $(#[$meta])*
        #[inline]
        fn $name($($arg: $ty),*) $(-> $ret)? {
            #[inline(always)]
            fn generic($($arg: $ty),*) $(-> $ret)? $body

            #[cfg(all(target_arch = "x86_64", not(miri)))]
            {
                #[target_feature(enable = "avx2,bmi1,bmi2,popcnt,sse4.2")]
                unsafe fn avx2($($arg: $ty),*) $(-> $ret)? {
                    generic($($arg),*)
                }

                #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512dq,avx512vpopcntdq,avx512bitalg,avx512vbmi2,avx2,bmi1,bmi2,popcnt,sse4.2")]
                unsafe fn avx512($($arg: $ty),*) $(-> $ret)? {
                    generic($($arg),*)
                }

                match simd::backend() {
                    // SAFETY: `backend()` only reports tiers supported by the running CPU.
                    simd::SimdBackend::Avx512 => unsafe { avx512($($arg),*) },
                    // SAFETY: as above; AVX2 is the minimum tier on x86_64.
                    simd::SimdBackend::Avx2 => unsafe { avx2($($arg),*) },
                    simd::SimdBackend::Neon => unreachable!("NEON backend on x86_64"),
                }
            }
            #[cfg(not(all(target_arch = "x86_64", not(miri))))]
            {
                generic($($arg),*)
            }
        }
    )*};
}

word_kernels! {
    /// Number of set bits of `words` covered by the runs.
    fn card_and(runs: &[Interval], words: &Words) -> u64 {
        let mut n = 0u64;
        for &iv in runs {
            let sp = Span::new(iv);
            n += (words[sp.first] & sp.first_mask).count_ones() as u64;
            if !sp.single() {
                n += popcount(&words[sp.middle()]);
                n += (words[sp.last] & sp.last_mask).count_ones() as u64;
            }
        }
        n
    }

    /// Whether some bit of `words` covered by the runs is set.
    fn intersects(runs: &[Interval], words: &Words) -> bool {
        runs.iter().any(|&iv| {
            let sp = Span::new(iv);
            let mut acc = words[sp.first] & sp.first_mask;
            if !sp.single() {
                acc |= words[sp.last] & sp.last_mask;
                acc |= words[sp.middle()].iter().fold(0, |acc, &w| acc | w);
            }
            acc != 0
        })
    }

    /// Whether every bit covered by the runs is set in `words`.
    fn covers(runs: &[Interval], words: &Words) -> bool {
        runs.iter().all(|&iv| {
            let sp = Span::new(iv);
            if sp.single() {
                return words[sp.first] & sp.first_mask == sp.first_mask;
            }
            words[sp.first] & sp.first_mask == sp.first_mask
                && words[sp.last] & sp.last_mask == sp.last_mask
                && words[sp.middle()].iter().fold(u64::MAX, |acc, &w| acc & w) == u64::MAX
        })
    }

    /// Sets the bits covered by the runs.
    fn set_words(runs: &[Interval], words: &mut Words) {
        for &iv in runs {
            let sp = Span::new(iv);
            words[sp.first] |= sp.first_mask;
            if !sp.single() {
                words[sp.middle()].fill(u64::MAX);
                words[sp.last] |= sp.last_mask;
            }
        }
    }

    /// Flips the bits covered by the runs.
    fn flip_words(runs: &[Interval], words: &mut Words) {
        for &iv in runs {
            let sp = Span::new(iv);
            words[sp.first] ^= sp.first_mask;
            if !sp.single() {
                for w in &mut words[sp.middle()] {
                    *w = !*w;
                }
                words[sp.last] ^= sp.last_mask;
            }
        }
    }

    /// Sets the bits covered by the runs, returns the number of newly set bits.
    fn set_count(runs: &[Interval], words: &mut Words) -> u64 {
        let mut added = 0u64;
        for &iv in runs {
            let sp = Span::new(iv);
            added += (sp.first_mask & !words[sp.first]).count_ones() as u64;
            words[sp.first] |= sp.first_mask;
            if !sp.single() {
                for w in &mut words[sp.middle()] {
                    added += (!*w).count_ones() as u64;
                    *w = u64::MAX;
                }
                added += (sp.last_mask & !words[sp.last]).count_ones() as u64;
                words[sp.last] |= sp.last_mask;
            }
        }
        added
    }

    /// Flips the bits covered by the runs, returns how many of them were set before.
    fn flip_count(runs: &[Interval], words: &mut Words) -> u64 {
        let mut was_set = 0u64;
        for &iv in runs {
            let sp = Span::new(iv);
            was_set += (words[sp.first] & sp.first_mask).count_ones() as u64;
            words[sp.first] ^= sp.first_mask;
            if !sp.single() {
                for w in &mut words[sp.middle()] {
                    was_set += w.count_ones() as u64;
                    *w = !*w;
                }
                was_set += (words[sp.last] & sp.last_mask).count_ones() as u64;
                words[sp.last] ^= sp.last_mask;
            }
        }
        was_set
    }

    /// Clears the bits covered by the runs, returns how many of them were set.
    fn clear_count(runs: &[Interval], words: &mut Words) -> u64 {
        let mut removed = 0u64;
        for &iv in runs {
            let sp = Span::new(iv);
            removed += (words[sp.first] & sp.first_mask).count_ones() as u64;
            words[sp.first] &= !sp.first_mask;
            if !sp.single() {
                for w in &mut words[sp.middle()] {
                    removed += w.count_ones() as u64;
                    *w = 0;
                }
                removed += (words[sp.last] & sp.last_mask).count_ones() as u64;
                words[sp.last] &= !sp.last_mask;
            }
        }
        removed
    }

    /// `dst |= src & runs` (`dst` is typically zeroed).
    fn copy_and(runs: &[Interval], src: &Words, dst: &mut Words) {
        for &iv in runs {
            let sp = Span::new(iv);
            dst[sp.first] |= src[sp.first] & sp.first_mask;
            if !sp.single() {
                dst[sp.middle()].copy_from_slice(&src[sp.middle()]);
                dst[sp.last] |= src[sp.last] & sp.last_mask;
            }
        }
    }

    /// `dst |= runs & !src` (`dst` is typically zeroed).
    fn copy_andnot(runs: &[Interval], src: &Words, dst: &mut Words) {
        for &iv in runs {
            let sp = Span::new(iv);
            dst[sp.first] |= !src[sp.first] & sp.first_mask;
            if !sp.single() {
                for (d, &s) in dst[sp.middle()].iter_mut().zip(&src[sp.middle()]) {
                    *d = !s;
                }
                dst[sp.last] |= !src[sp.last] & sp.last_mask;
            }
        }
    }

    /// Writes the set bits of `words` covered by the runs in increasing order, returns their
    /// number. `out` must hold `card_and(runs, words)` values.
    fn extract_and(runs: &[Interval], words: &Words, out: &mut [MaybeUninit<u16>]) -> usize {
        let mut k = 0;
        for &iv in runs {
            let sp = Span::new(iv);
            k = extract_bits(words[sp.first] & sp.first_mask, sp.first, out, k);
            if !sp.single() {
                for (i, &w) in words[sp.middle()].iter().enumerate() {
                    k = extract_bits(w, sp.first + 1 + i, out, k);
                }
                k = extract_bits(words[sp.last] & sp.last_mask, sp.last, out, k);
            }
        }
        k
    }

    /// Writes the clear bits of `words` covered by the runs in increasing order, returns their
    /// number. `out` must hold `len(runs) - card_and(runs, words)` values.
    fn extract_andnot(runs: &[Interval], words: &Words, out: &mut [MaybeUninit<u16>]) -> usize {
        let mut k = 0;
        for &iv in runs {
            let sp = Span::new(iv);
            k = extract_bits(!words[sp.first] & sp.first_mask, sp.first, out, k);
            if !sp.single() {
                for (i, &w) in words[sp.middle()].iter().enumerate() {
                    k = extract_bits(!w, sp.first + 1 + i, out, k);
                }
                k = extract_bits(!words[sp.last] & sp.last_mask, sp.last, out, k);
            }
        }
        k
    }

    /// Appends the maximal runs of set bits of `words` (which must not be all zero) to `out`.
    fn runs_from_words(words: &Words, out: &mut HeapVec<Interval>) {
        let mut i = 0;
        let mut w = words[0];
        loop {
            while w == 0 {
                i += 1;
                if i == BITMAP_WORDS {
                    return;
                }
                w = words[i];
            }
            let start = i * 64 + w.trailing_zeros() as usize;
            // Fill the bits below the run start, so the run is a block of trailing ones.
            w |= w - 1;
            while w == u64::MAX {
                i += 1;
                if i == BITMAP_WORDS {
                    out.push(Interval::new_unchecked(start as u16, u16::MAX));
                    return;
                }
                w = words[i];
            }
            // One past the last bit of the run.
            let end = i * 64 + (!w).trailing_zeros() as usize;
            out.push(Interval::new_unchecked(start as u16, (end - 1) as u16));
            // Clear the run (the trailing ones).
            w &= w + 1;
        }
    }
}

/// A bitmap result as a container: arrays up to [`ARRAY_LIMIT`] values, bitmaps above.
fn bitmap_result(bitmap: BitmapStore) -> Container {
    let len = bitmap.len();
    if len <= ARRAY_LIMIT {
        Container::Array(ArrayStore::build(len as usize, |out| {
            simd::bitmap_to_array(bitmap.as_words(), out)
        }))
    } else {
        Container::Bitmap(bitmap)
    }
}

/// `runs & bitmap` as a container (array up to [`ARRAY_LIMIT`] values).
fn intersection_bitmap(runs: &[Interval], bitmap: &BitmapStore) -> Container {
    let words = bitmap.as_words();
    let card = card_and(runs, words);
    if card <= ARRAY_LIMIT {
        return Container::Array(ArrayStore::build(card as usize, |out| {
            extract_and(runs, words, out)
        }));
    }
    let mut out = BitmapStore::new();
    copy_and(runs, words, out.as_words_mut());
    out.set_len(card);
    Container::Bitmap(out)
}

/// `runs - bitmap` as a container (array up to [`ARRAY_LIMIT`] values).
fn difference_run_bitmap(runs: &RunStore, bitmap: &BitmapStore) -> Container {
    let (ivs, words) = (runs.intervals(), bitmap.as_words());
    let card = runs.len() - card_and(ivs, words);
    if card <= ARRAY_LIMIT {
        return Container::Array(ArrayStore::build(card as usize, |out| {
            extract_andnot(ivs, words, out)
        }));
    }
    let mut out = BitmapStore::new();
    copy_andnot(ivs, words, out.as_words_mut());
    out.set_len(card);
    Container::Bitmap(out)
}

// ---------------------------------------------------------------------------------------------
// Container operations with at least one run operand.
//
// Representation choices of the results follow the `roaring` crate (0.11, `store/mod.rs`) and
// must satisfy the `Container` invariants (arrays <= ARRAY_LIMIT < bitmaps; runs unrestricted):
//
// | operation | run ∘ run | run ∘ array | array ∘ run | run ∘ bitmap, bitmap ∘ run |
// |-----------|-----------|-------------|-------------|----------------------------|
// | union     | run       | run         | run         | bitmap                     |
// | inter.    | run       | array       | array       | bitmap, array if <= 4096   |
// | diff.     | run       | run         | array       | bitmap, array if <= 4096   |
// | sym. diff | run       | run         | run         | bitmap, array if <= 4096   |
// ---------------------------------------------------------------------------------------------

#[cold]
#[inline(never)]
fn no_run_operand() -> ! {
    unreachable!("run container operation called without a run operand")
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn union(a: &Container, b: &Container) -> Container {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => Run(union_streams(
            x.intervals().iter().copied(),
            y.intervals().iter().copied(),
            x.0.len() + y.0.len(),
        )),
        (Run(r), Array(v)) | (Array(v), Run(r)) => Run(union_streams(
            r.intervals().iter().copied(),
            ArrayRuns(v.as_slice()),
            r.0.len() + v.as_slice().len(),
        )),
        (Run(r), Bitmap(m)) | (Bitmap(m), Run(r)) => {
            let mut out = m.clone();
            let added = set_count(r.intervals(), out.as_words_mut());
            out.set_len(m.len() + added);
            Bitmap(out)
        }
        _ => no_run_operand(),
    }
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn intersection(a: &Container, b: &Container) -> Container {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => Run(intersection_runs(x.intervals(), y.intervals())),
        (Run(r), Array(v)) | (Array(v), Run(r)) => {
            Array(filter_array(v.as_slice(), r.intervals(), true))
        }
        (Run(r), Bitmap(m)) | (Bitmap(m), Run(r)) => intersection_bitmap(r.intervals(), m),
        _ => no_run_operand(),
    }
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn difference(a: &Container, b: &Container) -> Container {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => Run(difference_streams(
            x.intervals().iter().copied(),
            y.intervals().iter().copied(),
            x.0.len() + y.0.len(),
        )),
        (Run(r), Array(v)) => Run(difference_streams(
            r.intervals().iter().copied(),
            ArrayRuns(v.as_slice()),
            r.0.len() + v.as_slice().len(),
        )),
        (Array(v), Run(r)) => Array(filter_array(v.as_slice(), r.intervals(), false)),
        (Run(r), Bitmap(m)) => difference_run_bitmap(r, m),
        (Bitmap(m), Run(r)) => {
            let mut out = m.clone();
            let removed = clear_count(r.intervals(), out.as_words_mut());
            out.set_len(m.len() - removed);
            bitmap_result(out)
        }
        _ => no_run_operand(),
    }
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn symmetric_difference(a: &Container, b: &Container) -> Container {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => Run(symmetric_difference_streams(
            x.intervals().iter().copied(),
            y.intervals().iter().copied(),
            x.0.len() + y.0.len(),
        )),
        (Run(r), Array(v)) | (Array(v), Run(r)) => Run(symmetric_difference_streams(
            r.intervals().iter().copied(),
            ArrayRuns(v.as_slice()),
            r.0.len() + v.as_slice().len(),
        )),
        (Run(r), Bitmap(m)) | (Bitmap(m), Run(r)) => {
            let mut out = m.clone();
            let was_set = flip_count(r.intervals(), out.as_words_mut());
            out.set_len(m.len() + r.len() - 2 * was_set);
            bitmap_result(out)
        }
        _ => no_run_operand(),
    }
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn intersection_len(a: &Container, b: &Container) -> u64 {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => intersection_len_runs(x.intervals(), y.intervals()),
        (Run(r), Array(v)) | (Array(v), Run(r)) => {
            intersection_len_array(v.as_slice(), r.intervals())
        }
        (Run(r), Bitmap(m)) | (Bitmap(m), Run(r)) => card_and(r.intervals(), m.as_words()),
        _ => no_run_operand(),
    }
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn is_disjoint(a: &Container, b: &Container) -> bool {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => is_disjoint_runs(x.intervals(), y.intervals()),
        (Run(r), Array(v)) | (Array(v), Run(r)) => !array_has(v.as_slice(), r.intervals(), true),
        (Run(r), Bitmap(m)) | (Bitmap(m), Run(r)) => !intersects(r.intervals(), m.as_words()),
        _ => no_run_operand(),
    }
}

/// Whether `a ⊆ b`.
#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn is_subset(a: &Container, b: &Container) -> bool {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        (Run(x), Run(y)) => is_subset_runs(x.intervals(), y.intervals()),
        (Run(r), Array(v)) => runs_subset_of_array(r.intervals(), v.as_slice()),
        (Array(v), Run(r)) => !array_has(v.as_slice(), r.intervals(), false),
        (Run(r), Bitmap(m)) => covers(r.intervals(), m.as_words()),
        (Bitmap(m), Run(r)) => {
            m.len() <= r.len() && card_and(r.intervals(), m.as_words()) == m.len()
        }
        _ => no_run_operand(),
    }
}

#[allow(dead_code)] // Called by `Container` (mod.rs).
pub(crate) fn eq_values(a: &Container, b: &Container) -> bool {
    use Container::{Array, Bitmap, Run};
    match (a, b) {
        // Both are canonical (maximal runs), so equal sets have equal interval lists.
        (Run(x), Run(y)) => x.intervals() == y.intervals(),
        (Run(r), Array(v)) | (Array(v), Run(r)) => {
            r.run_amount() <= v.len()
                && r.len() == v.len()
                && runs_subset_of_array(r.intervals(), v.as_slice())
        }
        (Run(r), Bitmap(m)) | (Bitmap(m), Run(r)) => {
            r.len() == m.len() && covers(r.intervals(), m.as_words())
        }
        _ => no_run_operand(),
    }
}

// ---------------------------------------------------------------------------------------------
// In-place variants: `*a = op(a, b)` with exactly the same result representation as the
// functions above, but updating a bitmap target in place (no 8 KiB clone) and editing a run
// target in place when the other operand is small. At least one operand must be a run.
// ---------------------------------------------------------------------------------------------

/// Right-hand sides with at most this many intervals (or array values) are applied to a run
/// target by in-place interval edits instead of a full merge into a new allocation.
const SMALL_OPERAND: usize = 8;

#[allow(dead_code)]
pub(crate) fn union_with(a: &mut Container, b: &Container) {
    use Container::{Array, Bitmap, Run};
    match (&mut *a, b) {
        (Bitmap(m), Run(r)) => {
            let added = set_count(r.intervals(), m.as_words_mut());
            m.set_len(m.len() + added);
        }
        (Run(x), Run(y)) if y.0.len() <= SMALL_OPERAND => {
            for iv in y.intervals() {
                x.insert_range(iv.start..=iv.end);
            }
        }
        (Run(x), Array(v)) if v.as_slice().len() <= SMALL_OPERAND => {
            for &value in v.as_slice() {
                x.insert(value);
            }
        }
        _ => *a = union(a, b),
    }
}

#[allow(dead_code)]
pub(crate) fn intersect_with(a: &mut Container, b: &Container) {
    *a = intersection(a, b);
}

#[allow(dead_code)]
pub(crate) fn difference_with(a: &mut Container, b: &Container) {
    use Container::{Array, Bitmap, Run};
    match (&mut *a, b) {
        (Bitmap(m), Run(r)) => {
            let removed = clear_count(r.intervals(), m.as_words_mut());
            let len = m.len() - removed;
            m.set_len(len);
            if len <= ARRAY_LIMIT {
                let words = m.as_words();
                let array =
                    ArrayStore::build(len as usize, |out| simd::bitmap_to_array(words, out));
                *a = Array(array);
            }
        }
        (Run(x), Run(y)) if y.0.len() <= SMALL_OPERAND => {
            for iv in y.intervals() {
                x.remove_range(iv.start..=iv.end);
            }
        }
        (Run(x), Array(v)) if v.as_slice().len() <= SMALL_OPERAND => {
            for &value in v.as_slice() {
                x.remove(value);
            }
        }
        _ => *a = difference(a, b),
    }
}

#[allow(dead_code)]
pub(crate) fn symmetric_difference_with(a: &mut Container, b: &Container) {
    use Container::{Array, Bitmap, Run};
    match (&mut *a, b) {
        (Bitmap(m), Run(r)) => {
            let was_set = flip_count(r.intervals(), m.as_words_mut());
            let len = m.len() + r.len() - 2 * was_set;
            m.set_len(len);
            if len <= ARRAY_LIMIT {
                let words = m.as_words();
                let array =
                    ArrayStore::build(len as usize, |out| simd::bitmap_to_array(words, out));
                *a = Array(array);
            }
        }
        (Run(x), Array(v)) if v.as_slice().len() <= SMALL_OPERAND => {
            for &value in v.as_slice() {
                if !x.remove(value) {
                    x.insert(value);
                }
            }
        }
        _ => *a = symmetric_difference(a, b),
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
