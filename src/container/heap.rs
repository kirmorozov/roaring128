//! A thin (one pointer wide) growable vector for `Copy` element types.
//!
//! Layout: a single heap block `[Header { len: u32, cap: u32 }][T; cap]`. An empty vector with no
//! capacity points at a shared static header and owns no allocation; it is never written
//! through. Being one pointer wide lets [`super::Container`] stay 16 bytes.
//!
//! All unsafe code of the container layer that manages memory lives here and is exercised under
//! Miri by the tests in this module.

use alloc::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};
use core::marker::PhantomData;
use core::mem::{MaybeUninit, align_of, size_of};
use core::ops::Range;
use core::ptr::{self, NonNull};
use core::slice;

#[repr(C, align(8))]
pub(crate) struct Header {
    len: u32,
    cap: u32,
}

/// The header shared by every vector without allocation. Only ever read.
static EMPTY_HEADER: Header = Header { len: 0, cap: 0 };

/// Smallest non-zero capacity allocated by the amortized growth policy.
const MIN_CAPACITY: usize = 4;

/// Thin vector of `Copy` values. See the module documentation.
pub(crate) struct HeapVec<T: Copy> {
    ptr: NonNull<Header>,
    _marker: PhantomData<T>,
}

// SAFETY: HeapVec uniquely owns its allocation, like `Vec<T>`.
unsafe impl<T: Copy + Send> Send for HeapVec<T> {}
// SAFETY: shared access only hands out `&[T]`.
unsafe impl<T: Copy + Sync> Sync for HeapVec<T> {}

const _: () = assert!(size_of::<HeapVec<u16>>() == 8);

impl<T: Copy> HeapVec<T> {
    /// Offset of the first element from the start of the block.
    const DATA_OFFSET: usize = {
        assert!(
            align_of::<T>() <= align_of::<Header>(),
            "element alignment too large"
        );
        assert!(size_of::<T>() > 0, "zero sized elements are not supported");
        size_of::<Header>()
    };

    fn layout(cap: usize) -> Layout {
        assert!(cap <= u32::MAX as usize, "HeapVec capacity overflow");
        let size = size_of::<T>()
            .checked_mul(cap)
            .and_then(|bytes| bytes.checked_add(Self::DATA_OFFSET))
            .expect("HeapVec capacity overflow");
        Layout::from_size_align(size, align_of::<Header>()).expect("HeapVec capacity overflow")
    }

    /// An empty vector without allocation.
    pub(crate) const fn new() -> Self {
        // SAFETY: a pointer derived from a reference is never null. The static is only read.
        let ptr = unsafe { NonNull::new_unchecked(&EMPTY_HEADER as *const Header as *mut Header) };
        HeapVec {
            ptr,
            _marker: PhantomData,
        }
    }

    /// A vector with room for exactly `cap` elements.
    pub(crate) fn with_capacity(cap: usize) -> Self {
        if cap == 0 {
            return HeapVec::new();
        }
        let layout = Self::layout(cap);
        // SAFETY: the layout has a non-zero size (it includes the header).
        let raw = unsafe { alloc(layout) }.cast::<Header>();
        let Some(ptr) = NonNull::new(raw) else {
            handle_alloc_error(layout)
        };
        // SAFETY: the block is freshly allocated, aligned and large enough for the header.
        unsafe {
            ptr.as_ptr().write(Header {
                len: 0,
                cap: cap as u32,
            })
        };
        HeapVec {
            ptr,
            _marker: PhantomData,
        }
    }

    pub(crate) fn from_slice(values: &[T]) -> Self {
        let mut v = HeapVec::with_capacity(values.len());
        v.extend_from_slice(values);
        v
    }

    #[inline]
    fn header(&self) -> &Header {
        // SAFETY: `ptr` points either at the static empty header or at our allocation.
        unsafe { self.ptr.as_ref() }
    }

    /// Whether the vector owns an allocation (and may therefore be written).
    #[inline]
    fn is_allocated(&self) -> bool {
        self.header().cap != 0
    }

    #[inline]
    fn data_ptr(&self) -> *mut T {
        // SAFETY: the data starts right after the header. For the static header this is the
        // one-past-the-end pointer, which is valid (and suitably aligned) for empty slices.
        unsafe {
            self.ptr
                .as_ptr()
                .cast::<u8>()
                .add(Self::DATA_OFFSET)
                .cast::<T>()
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.header().len as usize
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.header().cap as usize
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[T] {
        // SAFETY: the first `len` elements are initialized; `data_ptr` is non-null and aligned.
        unsafe { slice::from_raw_parts(self.data_ptr(), self.len()) }
    }

    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        if !self.is_allocated() {
            return &mut [];
        }
        // SAFETY: we own the allocation and the first `len` elements are initialized.
        unsafe { slice::from_raw_parts_mut(self.data_ptr(), self.len()) }
    }

    /// The uninitialized tail `len..capacity`.
    pub(crate) fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<T>] {
        if !self.is_allocated() {
            return &mut [];
        }
        let len = self.len();
        let spare = self.capacity() - len;
        // SAFETY: elements `len..cap` lie inside our allocation; `MaybeUninit` needs no init.
        unsafe { slice::from_raw_parts_mut(self.data_ptr().add(len).cast(), spare) }
    }

    /// # Safety
    /// `new_len <= capacity()` and the first `new_len` elements are initialized.
    #[inline]
    pub(crate) unsafe fn set_len(&mut self, new_len: usize) {
        debug_assert!(new_len <= self.capacity());
        if self.is_allocated() {
            // SAFETY: we own the allocation; `new_len <= cap <= u32::MAX`.
            unsafe { (*self.ptr.as_ptr()).len = new_len as u32 };
        } else {
            debug_assert_eq!(new_len, 0);
        }
    }

    /// Reallocates to exactly `new_cap` elements (`new_cap >= len`).
    fn set_capacity(&mut self, new_cap: usize) {
        debug_assert!(new_cap >= self.len());
        let old_cap = self.capacity();
        if new_cap == old_cap {
            return;
        }
        if old_cap == 0 {
            *self = HeapVec::with_capacity(new_cap);
            return;
        }
        if new_cap == 0 {
            // len == 0 here, drop the allocation entirely.
            *self = HeapVec::new();
            return;
        }
        let old_layout = Self::layout(old_cap);
        let new_layout = Self::layout(new_cap);
        // SAFETY: `ptr` was allocated with `old_layout`; the new size is non-zero.
        let raw = unsafe { realloc(self.ptr.as_ptr().cast(), old_layout, new_layout.size()) };
        let Some(ptr) = NonNull::new(raw.cast::<Header>()) else {
            handle_alloc_error(new_layout)
        };
        self.ptr = ptr;
        // SAFETY: the reallocated block keeps the header and the first `len` elements.
        unsafe { (*self.ptr.as_ptr()).cap = new_cap as u32 };
    }

    /// Ensures room for `additional` more elements (amortized doubling).
    pub(crate) fn reserve(&mut self, additional: usize) {
        let needed = self
            .len()
            .checked_add(additional)
            .expect("HeapVec capacity overflow");
        let cap = self.capacity();
        if needed <= cap {
            return;
        }
        let new_cap = needed.max(cap.saturating_mul(2)).max(MIN_CAPACITY);
        self.set_capacity(new_cap);
    }

    /// Ensures room for exactly `additional` more elements.
    pub(crate) fn reserve_exact(&mut self, additional: usize) {
        let needed = self
            .len()
            .checked_add(additional)
            .expect("HeapVec capacity overflow");
        if needed > self.capacity() {
            self.set_capacity(needed);
        }
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        let len = self.len();
        if len < self.capacity() {
            self.set_capacity(len);
        }
    }

    pub(crate) fn push(&mut self, value: T) {
        self.reserve(1);
        let len = self.len();
        // SAFETY: `len < cap` after the reservation.
        unsafe {
            self.data_ptr().add(len).write(value);
            self.set_len(len + 1);
        }
    }

    pub(crate) fn insert(&mut self, index: usize, value: T) {
        let len = self.len();
        assert!(index <= len, "insertion index out of bounds");
        self.reserve(1);
        // SAFETY: `len < cap`; the tail `index..len` moves up by one inside the allocation.
        unsafe {
            let p = self.data_ptr().add(index);
            ptr::copy(p, p.add(1), len - index);
            p.write(value);
            self.set_len(len + 1);
        }
    }

    pub(crate) fn remove(&mut self, index: usize) -> T {
        let len = self.len();
        assert!(index < len, "removal index out of bounds");
        // SAFETY: `index < len`; the tail `index + 1..len` moves down by one.
        unsafe {
            let p = self.data_ptr().add(index);
            let value = p.read();
            ptr::copy(p.add(1), p, len - index - 1);
            self.set_len(len - 1);
            value
        }
    }

    /// Removes `range` from the vector, shifting the tail down.
    pub(crate) fn remove_range(&mut self, range: Range<usize>) {
        let len = self.len();
        assert!(
            range.start <= range.end && range.end <= len,
            "range out of bounds"
        );
        if range.is_empty() {
            return;
        }
        // SAFETY: both the source `range.end..len` and destination lie inside the elements.
        unsafe {
            let base = self.data_ptr();
            ptr::copy(base.add(range.end), base.add(range.start), len - range.end);
            self.set_len(len - range.len());
        }
    }

    /// Replaces `range` with `values`, growing or shrinking the vector as needed.
    pub(crate) fn splice(&mut self, range: Range<usize>, values: &[T]) {
        let len = self.len();
        assert!(
            range.start <= range.end && range.end <= len,
            "range out of bounds"
        );
        let new_len = len - range.len() + values.len();
        if new_len > self.capacity() {
            self.reserve(new_len - len);
        }
        if new_len == 0 {
            // SAFETY: zero elements are trivially initialized.
            unsafe { self.set_len(0) };
            return;
        }
        // SAFETY: capacity >= new_len, so the moved tail and the copied values stay inside the
        // allocation; `values` cannot alias our buffer because we hold `&mut self`.
        unsafe {
            let base = self.data_ptr();
            ptr::copy(
                base.add(range.end),
                base.add(range.start + values.len()),
                len - range.end,
            );
            ptr::copy_nonoverlapping(values.as_ptr(), base.add(range.start), values.len());
            self.set_len(new_len);
        }
    }

    /// Replaces `range` with the items of `values`, which must yield exactly `values.len()`
    /// items (panics otherwise, leaving the vector with its old length).
    pub(crate) fn splice_iter<I: ExactSizeIterator<Item = T>>(
        &mut self,
        range: Range<usize>,
        values: I,
    ) {
        let len = self.len();
        assert!(
            range.start <= range.end && range.end <= len,
            "range out of bounds"
        );
        let count = values.len();
        let new_len = len - range.len() + count;
        if new_len > self.capacity() {
            self.reserve(new_len - len);
        }
        if new_len == 0 {
            // SAFETY: zero elements are trivially initialized.
            unsafe { self.set_len(0) };
            return;
        }
        let base = self.data_ptr();
        // SAFETY: capacity >= new_len, so the moved tail stays inside the allocation.
        unsafe {
            ptr::copy(
                base.add(range.end),
                base.add(range.start + count),
                len - range.end,
            )
        };
        // Until `set_len`, the length is still `len`: every element below it stays initialized
        // (with old or moved values), so a panicking iterator cannot expose uninitialized data.
        let mut written = 0;
        for value in values.take(count) {
            // SAFETY: `range.start + written < range.start + count <= new_len <= capacity`.
            unsafe { base.add(range.start + written).write(value) };
            written += 1;
        }
        assert_eq!(written, count, "ExactSizeIterator reported a wrong length");
        // SAFETY: the gap was filled and the tail moved, so `new_len` elements are initialized.
        unsafe { self.set_len(new_len) };
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.len() {
            // SAFETY: shrinking keeps a prefix of initialized elements.
            unsafe { self.set_len(len) };
        }
    }

    pub(crate) fn clear(&mut self) {
        self.truncate(0)
    }

    pub(crate) fn extend_from_slice(&mut self, values: &[T]) {
        if values.is_empty() {
            return;
        }
        self.reserve(values.len());
        let len = self.len();
        // SAFETY: room for `values.len()` more elements was reserved; no aliasing (`&mut self`).
        unsafe {
            ptr::copy_nonoverlapping(values.as_ptr(), self.data_ptr().add(len), values.len());
            self.set_len(len + values.len());
        }
    }

    pub(crate) fn retain(&mut self, mut f: impl FnMut(&T) -> bool) {
        let slice = self.as_mut_slice();
        let mut kept = 0;
        for read in 0..slice.len() {
            let value = slice[read];
            if f(&value) {
                slice[kept] = value;
                kept += 1;
            }
        }
        self.truncate(kept);
    }

    /// Bytes owned on the heap (header + capacity), 0 without allocation.
    pub(crate) fn heap_bytes(&self) -> usize {
        if self.is_allocated() {
            Self::layout(self.capacity()).size()
        } else {
            0
        }
    }
}

impl<T: Copy> Default for HeapVec<T> {
    fn default() -> Self {
        HeapVec::new()
    }
}

impl<T: Copy> Clone for HeapVec<T> {
    fn clone(&self) -> Self {
        HeapVec::from_slice(self.as_slice())
    }

    fn clone_from(&mut self, source: &Self) {
        self.clear();
        self.extend_from_slice(source.as_slice());
    }
}

impl<T: Copy> Drop for HeapVec<T> {
    fn drop(&mut self) {
        if self.is_allocated() {
            // SAFETY: the block was allocated with exactly this layout.
            unsafe { dealloc(self.ptr.as_ptr().cast(), Self::layout(self.capacity())) };
        }
    }
}

impl<T: Copy + PartialEq> PartialEq for HeapVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + core::fmt::Debug> core::fmt::Debug for HeapVec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::HeapVec;
    use alloc::vec::Vec;

    #[test]
    fn empty_has_no_allocation() {
        let mut v = HeapVec::<u16>::new();
        assert_eq!(v.len(), 0);
        assert_eq!(v.capacity(), 0);
        assert_eq!(v.heap_bytes(), 0);
        assert!(v.as_slice().is_empty());
        assert!(v.as_mut_slice().is_empty());
        assert!(v.spare_capacity_mut().is_empty());
        v.truncate(0);
        v.clear();
        v.shrink_to_fit();
        v.retain(|_| true);
        v.extend_from_slice(&[]);
        v.remove_range(0..0);
        v.splice(0..0, &[]);
        assert_eq!(v.capacity(), 0);
        let c = v.clone();
        assert_eq!(c, v);
    }

    #[test]
    fn push_insert_remove_against_vec() {
        let mut v = HeapVec::<u16>::new();
        let mut model = Vec::new();
        for i in 0..300u16 {
            v.push(i * 3);
            model.push(i * 3);
        }
        assert_eq!(v.as_slice(), &model[..]);
        for (k, i) in [0usize, 5, 299, 150, 300].into_iter().enumerate() {
            v.insert(i, 1000 + k as u16);
            model.insert(i, 1000 + k as u16);
        }
        assert_eq!(v.as_slice(), &model[..]);
        // 305 elements before the removals: the last index is 301 after three removals.
        for i in [0usize, 10, 200, 301] {
            assert_eq!(v.remove(i), model.remove(i));
        }
        assert_eq!(v.as_slice(), &model[..]);
        v.remove_range(10..50);
        model.drain(10..50);
        assert_eq!(v.as_slice(), &model[..]);
        v.splice(3..7, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        model.splice(3..7, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(v.as_slice(), &model[..]);
        v.splice(0..20, &[42]);
        model.splice(0..20, [42]);
        assert_eq!(v.as_slice(), &model[..]);
        let len = v.len();
        v.splice(0..len, &[]);
        assert!(v.is_empty());
    }

    #[test]
    fn capacity_management() {
        let mut v = HeapVec::<u16>::with_capacity(10);
        assert_eq!(v.capacity(), 10);
        assert_eq!(v.spare_capacity_mut().len(), 10);
        for slot in v.spare_capacity_mut().iter_mut().take(4) {
            slot.write(7);
        }
        // SAFETY: four elements were written above.
        unsafe { v.set_len(4) };
        assert_eq!(v.as_slice(), &[7, 7, 7, 7]);
        v.shrink_to_fit();
        assert_eq!(v.capacity(), 4);
        v.reserve_exact(1);
        assert_eq!(v.capacity(), 5);
        // Like `Vec::reserve`: room for at least `len + additional` = 104 elements.
        v.reserve(100);
        assert!(v.capacity() >= 104);
        assert_eq!(v.capacity(), 104);
        v.clear();
        v.shrink_to_fit();
        assert_eq!(v.capacity(), 0);
        v.push(1);
        assert_eq!(v.as_slice(), &[1]);
    }

    #[test]
    fn retain_and_clone() {
        let mut v = HeapVec::from_slice(&(0..100u16).collect::<Vec<_>>());
        v.retain(|x| x % 3 == 0);
        let model: Vec<u16> = (0..100).filter(|x| x % 3 == 0).collect();
        assert_eq!(v.as_slice(), &model[..]);
        let c = v.clone();
        assert_eq!(c.as_slice(), &model[..]);
        assert_eq!(c.capacity(), model.len());
        let mut d = HeapVec::<u16>::new();
        d.clone_from(&v);
        assert_eq!(d, v);
    }

    #[test]
    fn multi_byte_elements() {
        #[derive(Clone, Copy, Debug, PartialEq)]
        struct Pair(u16, u16);
        let mut v = HeapVec::<Pair>::new();
        for i in 0..50 {
            v.push(Pair(i, i + 1));
        }
        v.insert(0, Pair(9, 9));
        assert_eq!(v.as_slice()[0], Pair(9, 9));
        assert_eq!(v.as_slice()[50], Pair(49, 50));
        assert_eq!(v.remove(1), Pair(0, 1));
        assert_eq!(v.len(), 50);
    }
}
