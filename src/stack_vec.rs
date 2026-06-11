use std::ops::{Deref, DerefMut};

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
    pub fn push(&mut self, item: T) {
        assert!(self.len < N);
        self.data[self.len] = item;
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            Some(self.data[self.len])
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

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
