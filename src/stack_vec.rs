//! Fixed-capacity stack-allocated vector.
//!
//! [`StackVec`] is a `Vec`-shaped container backed by an inline
//! `[MaybeUninit<T>; N]` array. There is no heap allocation and no
//! growth: pushing past `N` panics in debug, undefined behavior in
//! release. Slots past `len` are uninitialized and never observed
//! through the `Deref<[T]>` view — construction skips the
//! `N * size_of::<T>()` memset that an eager `[T::default(); N]` would
//! perform, which matters when a fresh `StackVec` is allocated on the
//! stack at every recursive node.
//!
//! Used by the move generator and search as `MoveList` to keep
//! per-ply scratch buffers off the heap and reusable across plies.

use std::fmt;
use std::iter::FusedIterator;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};

/// Stack-allocated vector of up to `N` elements of type `T`.
///
/// Implements [`Deref`]`<Target = [T]>`, so all slice methods
/// (`iter`, indexing, `split_first`, …) are available directly.
#[derive(Clone, Copy)]
pub struct StackVec<T: Copy, const N: usize> {
    data: [MaybeUninit<T>; N],
    len: usize,
}

impl<T: Copy, const N: usize> Default for StackVec<T, N> {
    #[inline(always)]
    fn default() -> Self {
        StackVec {
            data: [MaybeUninit::uninit(); N],
            len: 0,
        }
    }
}

impl<T: Copy, const N: usize> Deref for StackVec<T, N> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        // SAFETY: data[..len] is fully initialized — push writes a slot
        // before len advances over it, and len never grows without a
        // matching write. MaybeUninit<T> has the same layout as T.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const T, self.len) }
    }
}

impl<T: Copy, const N: usize> DerefMut for StackVec<T, N> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: see Deref::deref.
        unsafe { std::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut T, self.len) }
    }
}

impl<T: Copy + fmt::Debug, const N: usize> fmt::Debug for StackVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Owning iterator returned by [`StackVec::into_iter`].
///
/// Holds the original vector by value and walks it front-to-back.
/// Yields exactly `vec.len` items.
pub struct StackVecIntoIter<T: Copy, const N: usize> {
    vec: StackVec<T, N>,
    index: usize,
}

impl<T: Copy, const N: usize> Iterator for StackVecIntoIter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.vec.len {
            None
        } else {
            // SAFETY: index < len, so data[index] was initialized by push.
            let item = unsafe { self.vec.data[self.index].assume_init() };
            self.index += 1;
            Some(item)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.vec.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<T: Copy, const N: usize> DoubleEndedIterator for StackVecIntoIter<T, N> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index >= self.vec.len {
            None
        } else {
            self.vec.len -= 1;
            // SAFETY: index < old len, so data[len-1] was initialized.
            Some(unsafe { self.vec.data[self.vec.len].assume_init() })
        }
    }
}

impl<T: Copy, const N: usize> ExactSizeIterator for StackVecIntoIter<T, N> {}

impl<T: Copy, const N: usize> FusedIterator for StackVecIntoIter<T, N> {}

impl<T: Copy, const N: usize> Extend<T> for StackVec<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }
}

impl<T: Copy, const N: usize> IntoIterator for StackVec<T, N> {
    type Item = T;
    type IntoIter = StackVecIntoIter<T, N>;

    fn into_iter(self) -> Self::IntoIter {
        StackVecIntoIter {
            vec: self,
            index: 0,
        }
    }
}

impl<T: Copy, const N: usize> StackVec<T, N> {
    /// Appends `item`.
    /// Panics if the vector is already at capacity `N` in debug mode.
    /// Undefined behaviour in release mode.
    #[inline(always)]
    pub fn push(&mut self, item: T) {
        debug_assert!(self.len < N);
        self.data[self.len].write(item);
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
            // SAFETY: len was > 0, so data[len] was initialized by push.
            Some(unsafe { self.data[self.len].assume_init() })
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
            // SAFETY: read < self.len, so data[read] is initialized.
            let item = unsafe { self.data[read].assume_init() };
            if f(&item) {
                if write != read {
                    self.data[write].write(item);
                }
                write += 1;
            }
        }

        self.len = write;
    }
}
