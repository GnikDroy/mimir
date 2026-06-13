//! Triangular principal-variation table.
//!
//! At each ply the search records the best line discovered so far in a row
//! of moves. [`update`](PvTable::update) writes a new best move into the
//! row and copies the child ply's row in after it; the root PV is read
//! from row 0 via [`root`](PvTable::root).
//!
//! Move storage and per-row lengths are kept as parallel arrays so the
//! move grid stays contiguous: `update` does the row copy with a single
//! `copy_from_slice`, and `clear` only has to wipe the length array.

use crate::core::Move;

pub struct PvTable<const N: usize> {
    table: Box<[[Move; N]; N]>,
    length: Box<[u8; N]>,
}

impl<const N: usize> PvTable<N> {
    pub fn new() -> Self {
        Self {
            table: Box::new([[0u32; N]; N]),
            length: Box::new([0u8; N]),
        }
    }

    /// Drop every row's recorded PV. The move array isn't touched —
    /// `length` controls which slots are read.
    pub fn clear(&mut self) {
        self.length.fill(0);
    }

    /// Drop row `ply`. Callers should do this on entry to a node so early
    /// returns leave an empty PV the parent can see and truncate against.
    pub fn clear_ply(&mut self, ply: usize) {
        self.length[ply] = 0;
    }

    /// Write `mv` followed by row `ply + 1` into row `ply`.
    pub fn update(&mut self, ply: usize, mv: Move) {
        if ply + 1 >= N {
            self.table[ply][0] = mv;
            self.length[ply] = 1;
            return;
        }
        let child_len = self.length[ply + 1] as usize;
        let (lo, hi) = self.table.split_at_mut(ply + 1);
        lo[ply][0] = mv;
        lo[ply][1..=child_len].copy_from_slice(&hi[0][..child_len]);
        self.length[ply] = (child_len + 1) as u8;
    }

    /// The PV recorded at ply 0.
    pub fn root(&self) -> &[Move] {
        let len = self.length[0] as usize;
        &self.table[0][..len]
    }
}

impl<const N: usize> Default for PvTable<N> {
    fn default() -> Self {
        Self::new()
    }
}
