//! A thin (one pointer wide) growable vector for `Copy` element types.
//!
//! Layout: a single heap block `[Header { len: u32, cap: u32 }][T; cap]`. An empty vector with no
//! capacity points at a shared static header and owns no allocation; it must never be written
//! through. Being one pointer wide lets [`super::Container`] stay 16 bytes.
//!
//! All unsafe code of the container layer that manages memory lives here and is exercised under
//! Miri by the tests in this module.

use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;

#[repr(C)]
pub(crate) struct Header {
    len: u32,
    cap: u32,
}

/// Thin vector of `Copy` values. See the module documentation.
pub(crate) struct HeapVec<T: Copy> {
    ptr: NonNull<Header>,
    _marker: PhantomData<T>,
}

// SAFETY: HeapVec uniquely owns its allocation, like `Vec<T>`.
unsafe impl<T: Copy + Send> Send for HeapVec<T> {}
// SAFETY: shared access only hands out `&[T]`.
unsafe impl<T: Copy + Sync> Sync for HeapVec<T> {}

const _: () = assert!(core::mem::size_of::<HeapVec<u16>>() == 8);

#[allow(dead_code)]
impl<T: Copy> HeapVec<T> {
    /// An empty vector without allocation.
    pub(crate) const fn new() -> Self {
        todo!()
    }

    pub(crate) fn with_capacity(cap: usize) -> Self {
        todo!("{cap}")
    }

    pub(crate) fn from_slice(values: &[T]) -> Self {
        todo!("{}", values.len())
    }

    pub(crate) fn len(&self) -> usize {
        todo!()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn capacity(&self) -> usize {
        todo!()
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        todo!()
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        todo!()
    }

    /// The uninitialized tail `len..capacity`.
    pub(crate) fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<T>] {
        todo!()
    }

    /// # Safety
    /// `new_len <= capacity()` and the first `new_len` elements are initialized.
    pub(crate) unsafe fn set_len(&mut self, new_len: usize) {
        todo!("{new_len}")
    }

    /// Ensures room for `additional` more elements (amortized doubling).
    pub(crate) fn reserve(&mut self, additional: usize) {
        todo!("{additional}")
    }

    /// Ensures room for exactly `additional` more elements.
    pub(crate) fn reserve_exact(&mut self, additional: usize) {
        todo!("{additional}")
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        todo!()
    }

    pub(crate) fn push(&mut self, value: T) {
        let _ = value;
        todo!()
    }

    pub(crate) fn insert(&mut self, index: usize, value: T) {
        let _ = value;
        todo!("{index}")
    }

    pub(crate) fn remove(&mut self, index: usize) -> T {
        todo!("{index}")
    }

    /// Removes `range` from the vector, shifting the tail down.
    pub(crate) fn remove_range(&mut self, range: core::ops::Range<usize>) {
        todo!("{range:?}")
    }

    /// Replaces `range` with `values`, growing or shrinking the vector as needed.
    pub(crate) fn splice(&mut self, range: core::ops::Range<usize>, values: &[T]) {
        todo!("{range:?} {}", values.len())
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        todo!("{len}")
    }

    pub(crate) fn clear(&mut self) {
        self.truncate(0)
    }

    pub(crate) fn extend_from_slice(&mut self, values: &[T]) {
        todo!("{}", values.len())
    }

    pub(crate) fn retain(&mut self, f: impl FnMut(&T) -> bool) {
        let _ = f;
        todo!()
    }

    /// Bytes owned on the heap (header + capacity), 0 without allocation.
    pub(crate) fn heap_bytes(&self) -> usize {
        todo!()
    }
}

impl<T: Copy> Clone for HeapVec<T> {
    fn clone(&self) -> Self {
        HeapVec::from_slice(self.as_slice())
    }
}

impl<T: Copy> Drop for HeapVec<T> {
    fn drop(&mut self) {
        // Implementation must free the allocation if `capacity() > 0`.
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
