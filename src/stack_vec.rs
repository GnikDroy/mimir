//! Fixed-capacity stack-allocated vector.
//!
//! [`StackVec`] is a `Vec`-shaped container backed by an inline `[T; N]`
//! array. There is no heap allocation and no growth: pushing past `N`
//! panics. The element type must be `Copy + Default` so the backing
//! array can be initialized eagerly and `pop`/`retain` can shuffle
//! entries without `mem::replace`.
//!
//! Used by the move generator and search as `MoveList` to keep
//! per-ply scratch buffers off the heap and reusable across plies.

use std::iter::FusedIterator;
use std::ops::{Deref, DerefMut};

/// Stack-allocated vector of up to `N` elements of type `T`.
///
/// Implements [`Deref`]`<Target = [T]>`, so all slice methods
/// (`iter`, indexing, `split_first`, …) are available directly.
/// Elements past `len` are still valid `T::default()` instances in the
/// underlying array but are not visible through the slice view.
#[derive(Debug, Clone, Copy)]
pub struct StackVec<T: Copy + Default, const N: usize> {
    data: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> Default for StackVec<T, N> {
    fn default() -> Self {
        StackVec {
            data: [T::default(); N],
            len: 0,
        }
    }
}

impl<T: Copy + Default, const N: usize> Deref for StackVec<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.data[..self.len]
    }
}

impl<T: Copy + Default, const N: usize> DerefMut for StackVec<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data[..self.len]
    }
}

/// Owning iterator returned by [`StackVec::into_iter`].
///
/// Holds the original vector by value and walks it front-to-back.
/// Yields exactly `vec.len` items.
pub struct StackVecIntoIter<T: Copy + Default, const N: usize> {
    vec: StackVec<T, N>,
    index: usize,
}

impl<T: Copy + Default, const N: usize> Iterator for StackVecIntoIter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.vec.len {
            None
        } else {
            let item = self.vec.data[self.index];
            self.index += 1;
            Some(item)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.vec.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<T: Copy + Default, const N: usize> DoubleEndedIterator for StackVecIntoIter<T, N> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index >= self.vec.len {
            None
        } else {
            self.vec.len -= 1;
            Some(self.vec.data[self.vec.len])
        }
    }
}

impl<T: Copy + Default, const N: usize> ExactSizeIterator for StackVecIntoIter<T, N> {}

impl<T: Copy + Default, const N: usize> FusedIterator for StackVecIntoIter<T, N> {}

impl<T: Copy + Default, const N: usize> Extend<T> for StackVec<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }
}

impl<T: Copy + Default, const N: usize> IntoIterator for StackVec<T, N> {
    type Item = T;
    type IntoIter = StackVecIntoIter<T, N>;

    fn into_iter(self) -> Self::IntoIter {
        StackVecIntoIter {
            vec: self,
            index: 0,
        }
    }
}

impl<T: Copy + Default, const N: usize> StackVec<T, N> {
    /// Empty `StackVec` whose backing storage is left uninitialized.
    ///
    /// The `len` starts at zero, so the uninitialized bytes are never
    /// reachable through the `Deref<[T]>` view — `push` writes a slot
    /// before `len` advances over it. Skipping the zero-init saves the
    /// `N * size_of::<T>()` memset that `default()` performs, which
    /// matters when a fresh `StackVec` is allocated on the stack at
    /// every recursive node.
    ///
    /// # Safety
    /// `T` must accept any bit pattern as valid — i.e., integer types
    /// (`u32`, `i32`, …) or `Copy` newtypes over them. Calling this for
    /// types with validity invariants (`bool`, `NonZero*`, references,
    /// enums with niche fillers) is undefined behavior even if no
    /// uninitialized slot is ever observed.
    #[inline(always)]
    pub unsafe fn new_uninit() -> Self {
        Self {
            data: std::mem::MaybeUninit::uninit().assume_init(),
            len: 0,
        }
    }

    /// Appends `item`.
    /// Panics if the vector is already at capacity `N` in debug mode.
    /// Undefined behaviour in release mode.
    #[inline(always)]
    pub fn push(&mut self, item: T) {
        debug_assert!(self.len < N);
        self.data[self.len] = item;
        self.len += 1;
    }

    /// Removes and returns the last element, or [`None`] if empty. The
    /// backing slot is left untouched (still holds the old value) but
    /// becomes invisible through the slice view.
    #[inline(always)]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            Some(self.data[self.len])
        }
    }

    /// Resets length to zero in O(1). Does not drop the removed elements.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Returns `true` if the vector contains no elements.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Keeps only the elements for which `f` returns `true`, in their
    /// original order. Compacts in place with a single read/write
    /// cursor pair — O(len), no allocation.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut write = 0;

        for read in 0..self.len {
            if f(&self.data[read]) {
                if write != read {
                    self.data[write] = self.data[read];
                }
                write += 1;
            }
        }

        self.len = write;
    }
}
