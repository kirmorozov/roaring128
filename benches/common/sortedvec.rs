//! A sorted `Vec<u128>`: the simplest ordered set, and for sparse random UUIDs the bar to clear
//! (16 bytes per value, binary-search lookups, linear merges).
//!
//! Lookups are binary searches. Binary operations merge into a new vector, except that when one
//! side is at least [`PROBE_RATIO`] times smaller its values are binary-searched in the other
//! (the same switch `std`'s `BTreeSet` and the wrapped baseline make). `&=` and `-=` filter in
//! place with `retain`; `|=` and `^=` merge into a new buffer and replace the old one (a sorted
//! vector cannot grow in place without shifting). Single-value inserts and removals shift the
//! tail (O(n)), so the benchmarks build it by sorting and do not time per-value updates.

use std::cmp::Ordering;
use std::io::{self, Read, Write};

/// When one side is at least this many times smaller, probe it into the other by binary search.
const PROBE_RATIO: usize = 16;

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct SortedVec(Vec<u128>);

#[derive(Clone, Copy)]
enum Keep {
    And,
    Or,
    Sub,
    Xor,
}

/// Linear merge of two sorted slices into a new vector.
fn merge(a: &[u128], b: &[u128], keep: Keep) -> Vec<u128> {
    let capacity = match keep {
        Keep::And => a.len().min(b.len()),
        Keep::Sub => a.len(),
        Keep::Or | Keep::Xor => a.len() + b.len(),
    };
    let mut out = Vec::with_capacity(capacity);
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => {
                if !matches!(keep, Keep::And) {
                    out.push(a[i]);
                }
                i += 1;
            }
            Ordering::Greater => {
                if matches!(keep, Keep::Or | Keep::Xor) {
                    out.push(b[j]);
                }
                j += 1;
            }
            Ordering::Equal => {
                if matches!(keep, Keep::And | Keep::Or) {
                    out.push(a[i]);
                }
                i += 1;
                j += 1;
            }
        }
    }
    if !matches!(keep, Keep::And) {
        out.extend_from_slice(&a[i..]);
    }
    if matches!(keep, Keep::Or | Keep::Xor) {
        out.extend_from_slice(&b[j..]);
    }
    out
}

fn contains(sorted: &[u128], v: u128) -> bool {
    sorted.binary_search(&v).is_ok()
}

fn skewed(small: &[u128], large: &[u128]) -> bool {
    small.len() * PROBE_RATIO <= large.len()
}

impl SortedVec {
    /// Wraps values that are already strictly increasing.
    pub fn from_sorted(sorted: Vec<u128>) -> Self {
        debug_assert!(sorted.windows(2).all(|w| w[0] < w[1]));
        SortedVec(sorted)
    }

    /// Sorts and deduplicates a copy of `values`.
    pub fn from_unsorted(values: &[u128]) -> Self {
        let mut v = values.to_vec();
        v.sort_unstable();
        v.dedup();
        SortedVec(v)
    }

    pub fn as_slice(&self) -> &[u128] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, v: u128) -> bool {
        contains(&self.0, v)
    }

    /// O(n): shifts the tail (O(1) when appending past the maximum).
    pub fn insert(&mut self, v: u128) -> bool {
        match self.0.binary_search(&v) {
            Ok(_) => false,
            Err(i) => {
                self.0.insert(i, v);
                true
            }
        }
    }

    /// O(n): shifts the tail.
    pub fn remove(&mut self, v: u128) -> bool {
        match self.0.binary_search(&v) {
            Ok(i) => {
                self.0.remove(i);
                true
            }
            Err(_) => false,
        }
    }

    /// The values in `lo..=hi`.
    pub fn range(&self, lo: u128, hi: u128) -> &[u128] {
        let a = self.0.partition_point(|&v| v < lo);
        let b = self.0.partition_point(|&v| v <= hi);
        &self.0[a..b.max(a)]
    }

    /// Number of values `<= v`.
    pub fn rank(&self, v: u128) -> u64 {
        self.0.partition_point(|&x| x <= v) as u64
    }

    pub fn select(&self, n: u64) -> Option<u128> {
        usize::try_from(n).ok().and_then(|n| self.0.get(n).copied())
    }

    pub fn and(&self, o: &Self) -> Self {
        let (small, large) = self.by_size(o);
        if skewed(small, large) {
            return SortedVec(
                small
                    .iter()
                    .copied()
                    .filter(|&v| contains(large, v))
                    .collect(),
            );
        }
        SortedVec(merge(&self.0, &o.0, Keep::And))
    }

    pub fn or(&self, o: &Self) -> Self {
        SortedVec(merge(&self.0, &o.0, Keep::Or))
    }

    pub fn sub(&self, o: &Self) -> Self {
        if skewed(&self.0, &o.0) {
            return SortedVec(
                self.0
                    .iter()
                    .copied()
                    .filter(|&v| !contains(&o.0, v))
                    .collect(),
            );
        }
        SortedVec(merge(&self.0, &o.0, Keep::Sub))
    }

    pub fn xor(&self, o: &Self) -> Self {
        SortedVec(merge(&self.0, &o.0, Keep::Xor))
    }

    pub fn intersection_len(&self, o: &Self) -> u64 {
        let (small, large) = self.by_size(o);
        if skewed(small, large) {
            return small.iter().filter(|&&v| contains(large, v)).count() as u64;
        }
        let (a, b) = (&self.0, &o.0);
        let (mut i, mut j, mut n) = (0, 0, 0);
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                Ordering::Less => i += 1,
                Ordering::Greater => j += 1,
                Ordering::Equal => {
                    n += 1;
                    i += 1;
                    j += 1;
                }
            }
        }
        n
    }

    pub fn is_subset(&self, o: &Self) -> bool {
        if self.len() > o.len() {
            return false;
        }
        if skewed(&self.0, &o.0) {
            return self.0.iter().all(|&v| contains(&o.0, v));
        }
        let mut j = 0;
        for &v in &self.0 {
            while j < o.0.len() && o.0[j] < v {
                j += 1;
            }
            if j == o.0.len() || o.0[j] != v {
                return false;
            }
            j += 1;
        }
        true
    }

    pub fn and_assign(&mut self, o: &Self) {
        if skewed(&o.0, &self.0) {
            *self = self.and(o);
        } else if skewed(&self.0, &o.0) {
            self.0.retain(|&v| contains(&o.0, v));
        } else {
            let mut j = 0;
            self.0.retain(|&v| {
                while j < o.0.len() && o.0[j] < v {
                    j += 1;
                }
                j < o.0.len() && o.0[j] == v
            });
        }
    }

    pub fn sub_assign(&mut self, o: &Self) {
        if skewed(&self.0, &o.0) {
            self.0.retain(|&v| !contains(&o.0, v));
        } else {
            let mut j = 0;
            self.0.retain(|&v| {
                while j < o.0.len() && o.0[j] < v {
                    j += 1;
                }
                j == o.0.len() || o.0[j] != v
            });
        }
    }

    pub fn or_assign(&mut self, o: &Self) {
        *self = self.or(o);
    }

    pub fn xor_assign(&mut self, o: &Self) {
        *self = self.xor(o);
    }

    /// Union of many sets: fold with merges.
    pub fn union_many(sets: &[&Self]) -> Self {
        let mut acc = SortedVec::default();
        for s in sets {
            acc = acc.or(s);
        }
        acc
    }

    /// Intersection of many sets: smallest first, then fold.
    pub fn intersection_many(sets: &[&Self]) -> Self {
        let mut sorted: Vec<&Self> = sets.to_vec();
        sorted.sort_by_key(|s| s.len());
        let Some((first, rest)) = sorted.split_first() else {
            return SortedVec::default();
        };
        let mut acc = match rest.first() {
            Some(second) => first.and(second),
            None => (*first).clone(),
        };
        for s in rest.iter().skip(1) {
            if acc.is_empty() {
                break;
            }
            acc.and_assign(s);
        }
        acc
    }

    fn by_size<'a>(&'a self, o: &'a Self) -> (&'a [u128], &'a [u128]) {
        if self.len() <= o.len() {
            (&self.0, &o.0)
        } else {
            (&o.0, &self.0)
        }
    }

    /// Count (u64 LE), then the values (u128 LE): a raw dump.
    pub fn serialized_size(&self) -> usize {
        8 + 16 * self.0.len()
    }

    pub fn serialize_into<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(&(self.0.len() as u64).to_le_bytes())?;
        let mut buf = Vec::with_capacity(16 * self.0.len().min(4096));
        for chunk in self.0.chunks(4096) {
            buf.clear();
            for v in chunk {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            writer.write_all(&buf)?;
        }
        Ok(())
    }

    /// Reads the dump back, checking that the values are strictly increasing.
    pub fn deserialize_from<R: Read>(mut reader: R) -> io::Result<Self> {
        let mut len = [0u8; 8];
        reader.read_exact(&mut len)?;
        let n = u64::from_le_bytes(len) as usize;
        let mut bytes = vec![0u8; n.checked_mul(16).ok_or(io::ErrorKind::InvalidData)?];
        reader.read_exact(&mut bytes)?;
        let values: Vec<u128> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|&c| u128::from_le_bytes(c))
            .collect();
        if values.windows(2).any(|w| w[0] >= w[1]) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "values are not strictly increasing",
            ));
        }
        Ok(SortedVec(values))
    }
}
