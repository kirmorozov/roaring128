//! The primary baseline: a 128-bit set built the way a user of the `roaring` crate would build
//! it, by wrapping `roaring::RoaringTreemap` (64-bit) in an outer ordered map keyed by the high
//! 64 bits of each value.
//!
//! The implementation is deliberately "natural but reasonably efficient": every operation
//! delegates to the treemap per outer key, binary operations merge-join the outer maps (or probe
//! the larger map when one side is much smaller, like `BTreeSet::intersection`), results are
//! collected from sorted iterators (bulk B-tree build), and in-place operations walk the
//! right-hand side. There is no cached cardinality: `len` sums the treemaps, which is what a
//! wrapper written in a few lines does.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::btree_map::{self, Entry};
use std::io::{self, Read, Write};
use std::iter::Peekable;
use std::ops::{
    BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, RangeInclusive, Sub, SubAssign,
};

use roaring::RoaringTreemap;

#[inline]
fn split(value: u128) -> (u64, u64) {
    ((value >> 64) as u64, value as u64)
}

#[inline]
fn join(hi: u64, lo: u64) -> u128 {
    ((hi as u128) << 64) | lo as u128
}

/// When one outer map is at least this many times smaller than the other, intersections probe
/// the larger map instead of merge-joining (the same heuristic as `std`'s `BTreeSet`).
const PROBE_RATIO: usize = 16;

/// `BTreeMap<u64, RoaringTreemap>`: high 64 bits -> treemap of the low 64 bits.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Wrapped64 {
    map: BTreeMap<u64, RoaringTreemap>,
}

/// Merge-join of two outer maps: yields every key with the treemaps of either side.
struct MergeJoin<'a> {
    a: Peekable<btree_map::Iter<'a, u64, RoaringTreemap>>,
    b: Peekable<btree_map::Iter<'a, u64, RoaringTreemap>>,
}

type Joined<'a> = (u64, Option<&'a RoaringTreemap>, Option<&'a RoaringTreemap>);

impl<'a> MergeJoin<'a> {
    fn new(a: &'a Wrapped64, b: &'a Wrapped64) -> Self {
        MergeJoin {
            a: a.map.iter().peekable(),
            b: b.map.iter().peekable(),
        }
    }
}

impl<'a> Iterator for MergeJoin<'a> {
    type Item = Joined<'a>;

    fn next(&mut self) -> Option<Joined<'a>> {
        let order = match (self.a.peek(), self.b.peek()) {
            (None, None) => return None,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some((ka, _)), Some((kb, _))) => ka.cmp(kb),
        };
        Some(match order {
            Ordering::Less => {
                let (&k, a) = self.a.next().unwrap();
                (k, Some(a), None)
            }
            Ordering::Greater => {
                let (&k, b) = self.b.next().unwrap();
                (k, None, Some(b))
            }
            Ordering::Equal => {
                let (&k, a) = self.a.next().unwrap();
                let (_, b) = self.b.next().unwrap();
                (k, Some(a), Some(b))
            }
        })
    }
}

fn non_empty(key: u64, t: RoaringTreemap) -> Option<(u64, RoaringTreemap)> {
    (!t.is_empty()).then_some((key, t))
}

impl Wrapped64 {
    pub fn new() -> Self {
        Wrapped64::default()
    }

    pub fn insert(&mut self, value: u128) -> bool {
        let (hi, lo) = split(value);
        self.map.entry(hi).or_default().insert(lo)
    }

    pub fn contains(&self, value: u128) -> bool {
        let (hi, lo) = split(value);
        self.map.get(&hi).is_some_and(|t| t.contains(lo))
    }

    pub fn remove(&mut self, value: u128) -> bool {
        let (hi, lo) = split(value);
        match self.map.entry(hi) {
            Entry::Occupied(mut e) => {
                let removed = e.get_mut().remove(lo);
                if e.get().is_empty() {
                    e.remove();
                }
                removed
            }
            Entry::Vacant(_) => false,
        }
    }

    pub fn len(&self) -> u64 {
        self.map.values().map(RoaringTreemap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = u128> + '_ {
        self.map
            .iter()
            .flat_map(|(&hi, t)| t.iter().map(move |lo| join(hi, lo)))
    }

    /// Values in `range`, in increasing order.
    pub fn range(&self, range: RangeInclusive<u128>) -> impl Iterator<Item = u128> + '_ {
        let (start_hi, start_lo) = split(*range.start());
        let (end_hi, end_lo) = split(*range.end());
        self.map.range(start_hi..=end_hi).flat_map(move |(&hi, t)| {
            let mut it = t.iter();
            if hi == start_hi {
                it.advance_to(start_lo);
            }
            let last = if hi == end_hi { end_lo } else { u64::MAX };
            it.take_while(move |&lo| lo <= last)
                .map(move |lo| join(hi, lo))
        })
    }

    /// Builds the set from strictly increasing values: one `RoaringTreemap::from_sorted_iter`
    /// per run of equal high halves, the outer map is bulk-built from the sorted entries.
    ///
    /// (Per-value `RoaringTreemap::try_push` would be up to 4x slower on dense data: `roaring`
    /// 0.11 recomputes the maximum of a bitmap container on every call.)
    ///
    /// Returns `Err(n)` if the input is not strictly increasing after `n` values.
    pub fn from_sorted_iter<I: IntoIterator<Item = u128>>(values: I) -> Result<Self, u64> {
        let mut values = values.into_iter().peekable();
        let mut entries: Vec<(u64, RoaringTreemap)> = Vec::new();
        let mut count = 0u64;
        while let Some(&first) = values.peek() {
            let hi = split(first).0;
            if entries.last().is_some_and(|&(k, _)| hi <= k) {
                return Err(count);
            }
            let lows =
                std::iter::from_fn(|| values.next_if(|&v| split(v).0 == hi)).map(|v| split(v).1);
            let t = RoaringTreemap::from_sorted_iter(lows).map_err(|e| count + e.valid_until())?;
            count += t.len();
            entries.push((hi, t));
        }
        Ok(Wrapped64 {
            map: entries.into_iter().collect(),
        })
    }

    /// Builds the set from values in any order: sort a copy, then [`Wrapped64::from_sorted_iter`]
    /// (what a user does for a batch rather than inserting value by value).
    pub fn from_unsorted(values: &[u128]) -> Self {
        let mut v = values.to_vec();
        v.sort_unstable();
        v.dedup();
        Wrapped64::from_sorted_iter(v).expect("sorted")
    }

    pub fn optimize(&mut self) -> bool {
        let mut changed = false;
        for t in self.map.values_mut() {
            changed |= t.optimize();
        }
        changed
    }

    pub fn rank(&self, value: u128) -> u64 {
        let (hi, lo) = split(value);
        let below: u64 = self.map.range(..hi).map(|(_, t)| t.len()).sum();
        below + self.map.get(&hi).map_or(0, |t| t.rank(lo))
    }

    pub fn select(&self, mut n: u64) -> Option<u128> {
        for (&hi, t) in &self.map {
            let len = t.len();
            if n < len {
                return t.select(n).map(|lo| join(hi, lo));
            }
            n -= len;
        }
        None
    }

    pub fn intersection_len(&self, other: &Wrapped64) -> u64 {
        let (small, large) = if self.map.len() <= other.map.len() {
            (self, other)
        } else {
            (other, self)
        };
        if small.map.len() * PROBE_RATIO <= large.map.len() {
            small
                .map
                .iter()
                .filter_map(|(k, a)| large.map.get(k).map(|b| a.intersection_len(b)))
                .sum()
        } else {
            MergeJoin::new(self, other)
                .map(|(_, a, b)| match (a, b) {
                    (Some(a), Some(b)) => a.intersection_len(b),
                    _ => 0,
                })
                .sum()
        }
    }

    pub fn union_len(&self, other: &Wrapped64) -> u64 {
        self.len() + other.len() - self.intersection_len(other)
    }

    pub fn difference_len(&self, other: &Wrapped64) -> u64 {
        self.len() - self.intersection_len(other)
    }

    pub fn symmetric_difference_len(&self, other: &Wrapped64) -> u64 {
        self.len() + other.len() - 2 * self.intersection_len(other)
    }

    pub fn is_subset(&self, other: &Wrapped64) -> bool {
        self.map
            .iter()
            .all(|(k, a)| other.map.get(k).is_some_and(|b| a.is_subset(b)))
    }

    /// Union of many sets: fold into an accumulator with `|=`.
    pub fn union_many<'a, I: IntoIterator<Item = &'a Wrapped64>>(sets: I) -> Wrapped64 {
        let mut acc = Wrapped64::new();
        for s in sets {
            acc |= s;
        }
        acc
    }

    /// Intersection of many sets: `&a & &b`, then fold with `&=`.
    pub fn intersection_many<'a, I: IntoIterator<Item = &'a Wrapped64>>(sets: I) -> Wrapped64 {
        let mut it = sets.into_iter();
        let Some(first) = it.next() else {
            return Wrapped64::new();
        };
        let mut acc = match it.next() {
            Some(second) => first & second,
            None => first.clone(),
        };
        for s in it {
            if acc.is_empty() {
                break;
            }
            acc &= s;
        }
        acc
    }

    /// `8 + Σ (8 + treemap serialized size)`.
    pub fn serialized_size(&self) -> usize {
        8 + self
            .map
            .values()
            .map(|t| 8 + t.serialized_size())
            .sum::<usize>()
    }

    /// Outer count (u64 LE), then per entry: high half (u64 LE) + `RoaringTreemap` format.
    pub fn serialize_into<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(&(self.map.len() as u64).to_le_bytes())?;
        for (k, t) in &self.map {
            writer.write_all(&k.to_le_bytes())?;
            t.serialize_into(&mut writer)?;
        }
        Ok(())
    }

    pub fn deserialize_from<R: Read>(mut reader: R) -> io::Result<Wrapped64> {
        fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
            let mut buf = [0u8; 8];
            r.read_exact(&mut buf)?;
            Ok(u64::from_le_bytes(buf))
        }
        let n = read_u64(&mut reader)?;
        let mut entries = Vec::with_capacity(n.min(1 << 16) as usize);
        let mut prev: Option<u64> = None;
        for _ in 0..n {
            let k = read_u64(&mut reader)?;
            if prev.is_some_and(|p| k <= p) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "outer keys are not strictly increasing",
                ));
            }
            prev = Some(k);
            let t = RoaringTreemap::deserialize_from(&mut reader)?;
            entries.push((k, t));
        }
        Ok(Wrapped64 {
            map: entries.into_iter().collect(),
        })
    }
}

impl BitAnd<&Wrapped64> for &Wrapped64 {
    type Output = Wrapped64;

    fn bitand(self, rhs: &Wrapped64) -> Wrapped64 {
        let (small, large) = if self.map.len() <= rhs.map.len() {
            (self, rhs)
        } else {
            (rhs, self)
        };
        let map = if small.map.len() * PROBE_RATIO <= large.map.len() {
            small
                .map
                .iter()
                .filter_map(|(&k, a)| large.map.get(&k).and_then(|b| non_empty(k, a & b)))
                .collect()
        } else {
            MergeJoin::new(self, rhs)
                .filter_map(|(k, a, b)| match (a, b) {
                    (Some(a), Some(b)) => non_empty(k, a & b),
                    _ => None,
                })
                .collect()
        };
        Wrapped64 { map }
    }
}

impl BitOr<&Wrapped64> for &Wrapped64 {
    type Output = Wrapped64;

    fn bitor(self, rhs: &Wrapped64) -> Wrapped64 {
        let map = MergeJoin::new(self, rhs)
            .map(|(k, a, b)| match (a, b) {
                (Some(a), Some(b)) => (k, a | b),
                (Some(t), None) | (None, Some(t)) => (k, t.clone()),
                (None, None) => unreachable!(),
            })
            .collect();
        Wrapped64 { map }
    }
}

impl Sub<&Wrapped64> for &Wrapped64 {
    type Output = Wrapped64;

    fn sub(self, rhs: &Wrapped64) -> Wrapped64 {
        let map = MergeJoin::new(self, rhs)
            .filter_map(|(k, a, b)| match (a, b) {
                (Some(a), Some(b)) => non_empty(k, a - b),
                (Some(a), None) => Some((k, a.clone())),
                _ => None,
            })
            .collect();
        Wrapped64 { map }
    }
}

impl BitXor<&Wrapped64> for &Wrapped64 {
    type Output = Wrapped64;

    fn bitxor(self, rhs: &Wrapped64) -> Wrapped64 {
        let map = MergeJoin::new(self, rhs)
            .filter_map(|(k, a, b)| match (a, b) {
                (Some(a), Some(b)) => non_empty(k, a ^ b),
                (Some(t), None) | (None, Some(t)) => Some((k, t.clone())),
                (None, None) => unreachable!(),
            })
            .collect();
        Wrapped64 { map }
    }
}

impl BitAndAssign<&Wrapped64> for Wrapped64 {
    fn bitand_assign(&mut self, rhs: &Wrapped64) {
        if rhs.map.len() * PROBE_RATIO <= self.map.len() {
            // Much smaller right-hand side: rebuild from its keys instead of visiting ours.
            *self = &*self & rhs;
            return;
        }
        self.map.retain(|k, a| match rhs.map.get(k) {
            Some(b) => {
                *a &= b;
                !a.is_empty()
            }
            None => false,
        });
    }
}

impl BitOrAssign<&Wrapped64> for Wrapped64 {
    fn bitor_assign(&mut self, rhs: &Wrapped64) {
        for (&k, b) in &rhs.map {
            match self.map.entry(k) {
                Entry::Occupied(mut e) => *e.get_mut() |= b,
                Entry::Vacant(e) => {
                    e.insert(b.clone());
                }
            }
        }
    }
}

impl SubAssign<&Wrapped64> for Wrapped64 {
    fn sub_assign(&mut self, rhs: &Wrapped64) {
        if self.map.len() < rhs.map.len() {
            // Visit the smaller side: our keys, probing the right-hand side.
            self.map.retain(|k, a| match rhs.map.get(k) {
                Some(b) => {
                    *a -= b;
                    !a.is_empty()
                }
                None => true,
            });
            return;
        }
        for (&k, b) in &rhs.map {
            if let Entry::Occupied(mut e) = self.map.entry(k) {
                *e.get_mut() -= b;
                if e.get().is_empty() {
                    e.remove();
                }
            }
        }
    }
}

impl BitXorAssign<&Wrapped64> for Wrapped64 {
    fn bitxor_assign(&mut self, rhs: &Wrapped64) {
        for (&k, b) in &rhs.map {
            match self.map.entry(k) {
                Entry::Occupied(mut e) => {
                    *e.get_mut() ^= b;
                    if e.get().is_empty() {
                        e.remove();
                    }
                }
                Entry::Vacant(e) => {
                    e.insert(b.clone());
                }
            }
        }
    }
}

macro_rules! owned_forms {
    ($($op:ident :: $f:ident => $assign:ident :: $af:ident),*) => {$(
        impl $op<&Wrapped64> for Wrapped64 {
            type Output = Wrapped64;

            fn $f(mut self, rhs: &Wrapped64) -> Wrapped64 {
                $assign::$af(&mut self, rhs);
                self
            }
        }
    )*};
}

owned_forms!(
    BitAnd::bitand => BitAndAssign::bitand_assign,
    BitOr::bitor => BitOrAssign::bitor_assign,
    Sub::sub => SubAssign::sub_assign,
    BitXor::bitxor => BitXorAssign::bitxor_assign
);
