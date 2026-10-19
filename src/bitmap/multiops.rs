//! Many-way set operations.
//!
//! * Union and symmetric difference are k-way merges by key (a binary heap over the operands'
//!   key iterators, `O(N log k)` for `N` containers in total). Containers sharing a key are
//!   folded into the largest of them with the lazy container operations and repaired once. When
//!   one operand dwarfs all the others, they are folded into it with point updates instead.
//! * Intersection sorts the first operands by number of containers, starts from the smallest
//!   and stops as soon as the result is empty.
//! * Difference is sequential with the same early exit.

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Reverse;
use core::convert::Infallible;

use super::is_skewed_update;
use crate::container::Container;
use crate::{MultiOps, RoaringBitmap128};

/// Number of operands sorted up front by intersections when the iterator size is unknown.
const BASE_COLLECT: usize = 10;
/// Maximum number of operands sorted up front by intersections.
const MAX_COLLECT: usize = 50;

#[inline]
fn infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

impl<I> MultiOps<RoaringBitmap128> for I
where
    I: IntoIterator<Item = RoaringBitmap128>,
{
    type Output = RoaringBitmap128;

    fn union(self) -> Self::Output {
        infallible(try_union_owned(self.into_iter().map(Ok)))
    }

    fn intersection(self) -> Self::Output {
        infallible(try_intersection_owned(self.into_iter().map(Ok)))
    }

    fn difference(self) -> Self::Output {
        infallible(try_difference_owned(self.into_iter().map(Ok)))
    }

    fn symmetric_difference(self) -> Self::Output {
        infallible(try_symmetric_difference_owned(self.into_iter().map(Ok)))
    }
}

impl<I, E> MultiOps<Result<RoaringBitmap128, E>> for I
where
    I: IntoIterator<Item = Result<RoaringBitmap128, E>>,
{
    type Output = Result<RoaringBitmap128, E>;

    fn union(self) -> Self::Output {
        try_union_owned(self)
    }

    fn intersection(self) -> Self::Output {
        try_intersection_owned(self)
    }

    fn difference(self) -> Self::Output {
        try_difference_owned(self)
    }

    fn symmetric_difference(self) -> Self::Output {
        try_symmetric_difference_owned(self)
    }
}

impl<'a, I> MultiOps<&'a RoaringBitmap128> for I
where
    I: IntoIterator<Item = &'a RoaringBitmap128>,
{
    type Output = RoaringBitmap128;

    fn union(self) -> Self::Output {
        infallible(try_union_ref(self.into_iter().map(Ok)))
    }

    fn intersection(self) -> Self::Output {
        infallible(try_intersection_ref(self.into_iter().map(Ok)))
    }

    fn difference(self) -> Self::Output {
        infallible(try_difference_ref(self.into_iter().map(Ok)))
    }

    fn symmetric_difference(self) -> Self::Output {
        infallible(try_symmetric_difference_ref(self.into_iter().map(Ok)))
    }
}

impl<'a, I, E: 'a> MultiOps<Result<&'a RoaringBitmap128, E>> for I
where
    I: IntoIterator<Item = Result<&'a RoaringBitmap128, E>>,
{
    type Output = Result<RoaringBitmap128, E>;

    fn union(self) -> Self::Output {
        try_union_ref(self)
    }

    fn intersection(self) -> Self::Output {
        try_intersection_ref(self)
    }

    fn difference(self) -> Self::Output {
        try_difference_ref(self)
    }

    fn symmetric_difference(self) -> Self::Output {
        try_symmetric_difference_ref(self)
    }
}

// ---------------------------------------------------------------------------------------------
// Union and symmetric difference: k-way merges
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Union,
    SymmetricDifference,
}

/// Folds owned containers sharing a key. `group` is drained.
fn combine_owned(group: &mut Vec<Container>, op: Op) -> Container {
    if group.len() == 1 {
        return group.pop().expect("one container");
    }
    let largest = (0..group.len())
        .max_by_key(|&i| group[i].len())
        .expect("non empty group");
    let mut acc = group.swap_remove(largest);
    if group.len() == 1 {
        let other = group.pop().expect("one container");
        match op {
            Op::Union => acc.union_with_owned(other),
            Op::SymmetricDifference => acc.symmetric_difference_with_owned(other),
        }
        return acc;
    }
    for other in group.drain(..) {
        match op {
            Op::Union => acc.lazy_union_with(&other),
            Op::SymmetricDifference => acc.lazy_symmetric_difference_with(&other),
        }
    }
    acc.repair_lazy();
    acc
}

/// Folds borrowed containers sharing a key. `group` is cleared.
fn combine_ref(group: &mut Vec<&Container>, op: Op) -> Container {
    let result = match group.as_slice() {
        [single] => (*single).clone(),
        [a, b] => match op {
            Op::Union => a.union(b),
            Op::SymmetricDifference => a.symmetric_difference(b),
        },
        _ => {
            let largest = (0..group.len())
                .max_by_key(|&i| group[i].len())
                .expect("non empty group");
            let mut acc = group[largest].clone();
            for (i, other) in group.iter().enumerate() {
                if i != largest {
                    match op {
                        Op::Union => acc.lazy_union_with(other),
                        Op::SymmetricDifference => acc.lazy_symmetric_difference_with(other),
                    }
                }
            }
            acc.repair_lazy();
            acc
        }
    };
    group.clear();
    result
}

/// k-way merge of the operands' containers by key.
///
/// `heads[i]` is the current (smallest unconsumed) container of operand `i`, the heap orders the
/// operands by the key of their head.
fn kway_merge<T, It>(
    iters: Vec<It>,
    op: Op,
    mut combine: impl FnMut(&mut Vec<T>, Op) -> Container,
) -> RoaringBitmap128
where
    It: Iterator<Item = (u128, T)>,
{
    let mut iters = iters;
    let mut heads: Vec<Option<T>> = Vec::with_capacity(iters.len());
    let mut heap = BinaryHeap::with_capacity(iters.len());
    for (i, it) in iters.iter_mut().enumerate() {
        match it.next() {
            Some((key, container)) => {
                heads.push(Some(container));
                heap.push(Reverse((key, i)));
            }
            None => heads.push(None),
        }
    }

    let mut out = Vec::new();
    let mut len = 0;
    let mut group = Vec::with_capacity(iters.len());
    while let Some(Reverse((key, i))) = heap.pop() {
        let mut take = |i: usize, heap: &mut BinaryHeap<Reverse<(u128, usize)>>| {
            group.push(heads[i].take().expect("operand head"));
            if let Some((next_key, container)) = iters[i].next() {
                heads[i] = Some(container);
                heap.push(Reverse((next_key, i)));
            }
        };
        take(i, &mut heap);
        while let Some(&Reverse((next_key, j))) = heap.peek() {
            if next_key != key {
                break;
            }
            heap.pop();
            take(j, &mut heap);
        }
        let container = combine(&mut group, op);
        if !container.is_empty() {
            len += container.len();
            out.push((key, container));
        }
    }
    RoaringBitmap128::from_sorted_entries_with_len(out, len)
}

fn multi_owned(mut bitmaps: Vec<RoaringBitmap128>, op: Op) -> RoaringBitmap128 {
    bitmaps.retain(|b| !b.is_empty());
    if bitmaps.len() <= 1 {
        return bitmaps.pop().unwrap_or_default();
    }
    let biggest = (0..bitmaps.len())
        .max_by_key(|&i| bitmaps[i].map.len())
        .expect("non empty");
    let rest: usize =
        bitmaps.iter().map(|b| b.map.len()).sum::<usize>() - bitmaps[biggest].map.len();
    if is_skewed_update(rest, bitmaps[biggest].map.len()) {
        // Point updates into the dominant operand beat touching all of its containers.
        let mut acc = bitmaps.swap_remove(biggest);
        for bitmap in bitmaps {
            match op {
                Op::Union => acc.union_assign_owned(bitmap),
                Op::SymmetricDifference => acc.symmetric_difference_assign_owned(bitmap),
            }
        }
        return acc;
    }
    let iters = bitmaps.into_iter().map(|b| b.map.into_iter()).collect();
    kway_merge(iters, op, combine_owned)
}

fn multi_ref(mut bitmaps: Vec<&RoaringBitmap128>, op: Op) -> RoaringBitmap128 {
    bitmaps.retain(|b| !b.is_empty());
    if bitmaps.len() <= 1 {
        return bitmaps.pop().cloned().unwrap_or_default();
    }
    let biggest = (0..bitmaps.len())
        .max_by_key(|&i| bitmaps[i].map.len())
        .expect("non empty");
    let rest: usize =
        bitmaps.iter().map(|b| b.map.len()).sum::<usize>() - bitmaps[biggest].map.len();
    if is_skewed_update(rest, bitmaps[biggest].map.len()) {
        let mut acc = bitmaps.swap_remove(biggest).clone();
        for bitmap in bitmaps {
            match op {
                Op::Union => acc.union_assign_ref(bitmap),
                Op::SymmetricDifference => acc.symmetric_difference_assign_ref(bitmap),
            }
        }
        return acc;
    }
    let iters = bitmaps
        .into_iter()
        .map(|b| b.map.iter().map(|(&k, c)| (k, c)))
        .collect();
    kway_merge(iters, op, combine_ref)
}

fn try_union_owned<E>(
    bitmaps: impl IntoIterator<Item = Result<RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let bitmaps = bitmaps.into_iter().collect::<Result<Vec<_>, E>>()?;
    Ok(multi_owned(bitmaps, Op::Union))
}

fn try_symmetric_difference_owned<E>(
    bitmaps: impl IntoIterator<Item = Result<RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let bitmaps = bitmaps.into_iter().collect::<Result<Vec<_>, E>>()?;
    Ok(multi_owned(bitmaps, Op::SymmetricDifference))
}

fn try_union_ref<'a, E>(
    bitmaps: impl IntoIterator<Item = Result<&'a RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let bitmaps = bitmaps.into_iter().collect::<Result<Vec<_>, E>>()?;
    Ok(multi_ref(bitmaps, Op::Union))
}

fn try_symmetric_difference_ref<'a, E>(
    bitmaps: impl IntoIterator<Item = Result<&'a RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let bitmaps = bitmaps.into_iter().collect::<Result<Vec<_>, E>>()?;
    Ok(multi_ref(bitmaps, Op::SymmetricDifference))
}

// ---------------------------------------------------------------------------------------------
// Intersection and difference: sequential with early exit
// ---------------------------------------------------------------------------------------------

/// Collects the first operands (all of them when the iterator announces at most
/// [`MAX_COLLECT`], [`BASE_COLLECT`] otherwise), propagating the first error.
fn collect_starting_elements<T, E>(
    iter: &mut impl Iterator<Item = Result<T, E>>,
) -> Result<Vec<T>, E> {
    let to_collect = match iter.size_hint().1 {
        Some(n) if n <= MAX_COLLECT => n,
        _ => BASE_COLLECT,
    };
    let mut collected = Vec::with_capacity(to_collect);
    for item in iter.take(to_collect) {
        collected.push(item?);
    }
    Ok(collected)
}

fn try_intersection_owned<E>(
    bitmaps: impl IntoIterator<Item = Result<RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let mut iter = bitmaps.into_iter();
    let mut start = collect_starting_elements(&mut iter)?;
    start.sort_unstable_by_key(|b| b.map.len());
    let mut operands = start.into_iter().map(Ok).chain(iter);
    let Some(first) = operands.next() else {
        return Ok(RoaringBitmap128::new());
    };
    let mut acc = first?;
    for rhs in operands {
        if acc.is_empty() {
            return Ok(acc);
        }
        acc.intersect_assign_owned(rhs?);
    }
    Ok(acc)
}

fn try_intersection_ref<'a, E>(
    bitmaps: impl IntoIterator<Item = Result<&'a RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let mut iter = bitmaps.into_iter();
    let mut start = collect_starting_elements(&mut iter)?;
    start.sort_unstable_by_key(|b| b.map.len());
    let mut operands = start.into_iter().map(Ok).chain(iter);
    let Some(first) = operands.next() else {
        return Ok(RoaringBitmap128::new());
    };
    let first = first?;
    if first.is_empty() {
        return Ok(RoaringBitmap128::new());
    }
    // `&a & &b` avoids cloning the first operand.
    let mut acc = match operands.next() {
        None => return Ok(first.clone()),
        Some(second) => first.intersection_ref(second?),
    };
    for rhs in operands {
        if acc.is_empty() {
            return Ok(acc);
        }
        acc.intersect_assign_ref(rhs?);
    }
    Ok(acc)
}

fn try_difference_owned<E>(
    bitmaps: impl IntoIterator<Item = Result<RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let mut iter = bitmaps.into_iter();
    let Some(first) = iter.next() else {
        return Ok(RoaringBitmap128::new());
    };
    let mut acc = first?;
    for rhs in iter {
        if acc.is_empty() {
            return Ok(acc);
        }
        acc.difference_assign_ref(&rhs?);
    }
    Ok(acc)
}

fn try_difference_ref<'a, E>(
    bitmaps: impl IntoIterator<Item = Result<&'a RoaringBitmap128, E>>,
) -> Result<RoaringBitmap128, E> {
    let mut iter = bitmaps.into_iter();
    let Some(first) = iter.next() else {
        return Ok(RoaringBitmap128::new());
    };
    let first = first?;
    if first.is_empty() {
        return Ok(RoaringBitmap128::new());
    }
    // `&a - &b` avoids cloning the containers of the first operand that disappear.
    let mut acc = match iter.next() {
        None => return Ok(first.clone()),
        Some(second) => first.difference_ref(second?),
    };
    for rhs in iter {
        if acc.is_empty() {
            return Ok(acc);
        }
        acc.difference_assign_ref(rhs?);
    }
    Ok(acc)
}
