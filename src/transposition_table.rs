//! Transposition table for caching search results keyed by Zobrist hash.
//!
//! The table is a fixed-size, power-of-two array of entry slots indexed by the
//! low bits of the Zobrist key. Collisions are resolved by a depth-preferred
//! replacement policy: a new entry overwrites an existing one unless the
//! existing entry is for the same position and was searched to a greater
//! depth.

use crate::core::Move;

/// Bound semantics of a stored score relative to the alpha-beta window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspositionFlag {
    /// Exact minimax score: the search returned a PV-node value strictly
    /// inside the `(alpha, beta)` window.
    Exact,
    /// Fail-high: the true score is at least `score` (beta cutoff).
    LowerBound,
    /// Fail-low: the true score is at most `score` (no move beat alpha).
    UpperBound,
}

/// A single transposition table record.
///
/// `key` is the full Zobrist hash and is checked on probe to guard against
/// index aliasing between distinct positions that collide on the same slot.
#[derive(Debug, Clone, Copy)]
pub struct TranspositionEntry {
    /// Full Zobrist key of the position; used to verify slot ownership.
    pub key: u64,
    /// Depth (in plies) to which this position was searched.
    pub depth: u8,
    /// Score returned by the search, interpreted according to [`flag`](Self::flag).
    pub score: i32,
    /// Bound type of [`score`](Self::score) with respect to the search window.
    pub flag: TranspositionFlag,
    /// Best move found at this position, if any (used for move ordering).
    pub best_move: Option<Move>,
}

/// Direct-mapped transposition table with depth-preferred replacement.
///
/// Capacity is always a power of two so that indexing reduces to a bitwise
/// mask on the Zobrist key.
pub struct TranspositionTable {
    entries: Vec<Option<TranspositionEntry>>,
}

impl TranspositionTable {
    /// Creates a table with the default capacity (~32 MiB worth of entries,
    /// rounded up to the next power of two).
    pub fn new() -> Self {
        const MEGABYTE: usize = 1024 * 1024;
        const SIZE_IN_MB: usize = 32 * MEGABYTE;
        const ENTRY_SIZE: usize = std::mem::size_of::<TranspositionEntry>();
        const DEFAULT_TABLE_SIZE: usize = (SIZE_IN_MB / ENTRY_SIZE).next_power_of_two();
        Self::with_capacity(DEFAULT_TABLE_SIZE)
    }

    /// Creates a table with the given number of slots.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is not a power of two; the indexing scheme relies
    /// on a bitmask of `capacity - 1`.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two());

        TranspositionTable {
            entries: vec![None; capacity],
        }
    }

    /// Maps a Zobrist key to a slot index via the low `log2(capacity)` bits.
    #[inline(always)]
    fn index(&self, key: u64) -> usize {
        (key as usize) & (self.entries.len() - 1)
    }

    /// Empties every slot. Call between independent searches (e.g.
    /// `ucinewgame`) when stale data must not influence move ordering.
    pub fn clear(&mut self) {
        self.entries.fill(None);
    }

    /// Looks up `key` in its mapped slot. Returns `Some` only when the slot
    /// is occupied **and** the stored entry's full key matches, ruling out
    /// index aliasing between distinct positions.
    pub fn probe(&self, key: u64) -> Option<TranspositionEntry> {
        self.entries[self.index(key)].filter(|entry| entry.key == key)
    }

    /// Inserts `entry`, preserving any existing entry for the same key that
    /// was searched to a strictly greater depth. All other cases (empty
    /// slot, different key, equal-or-shallower same-key entry) overwrite.
    pub fn store(&mut self, entry: TranspositionEntry) {
        let index = self.index(entry.key);

        match self.entries[index] {
            Some(existing) if existing.key == entry.key && existing.depth > entry.depth => {}
            _ => self.entries[index] = Some(entry),
        }
    }

    /// Reports the UCI `hashfull` value: the fraction of occupied slots in
    /// the first `SAMPLE_SIZE` entries scaled to the 0-1000 permill range.
    /// Sampling a fixed prefix keeps the call O(1) regardless of table size.
    pub fn hashfull(&self) -> u32 {
        const SAMPLE_SIZE: usize = 3000;
        const PERMILL_SCALE: usize = 1000;
        let sample = self.entries.len().min(SAMPLE_SIZE);
        if sample == 0 {
            return 0;
        }
        let filled = self.entries[..sample]
            .iter()
            .filter(|e| e.is_some())
            .count();
        ((filled * PERMILL_SCALE) / sample) as u32
    }
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::new()
    }
}
