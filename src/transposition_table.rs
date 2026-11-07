//! Transposition table for caching search results keyed by Zobrist hash.
//!
//! Fixed-size array of slots. Replacement is depth-preferred: a new
//! entry overwrites unless the slot holds the same position searched
//! to a greater depth.

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
#[derive(Debug, Clone, Copy)]
pub struct TranspositionEntry {
    /// Full Zobrist key of the position. Used to verify slot ownership.
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
pub struct TranspositionTable {
    entries: Vec<Option<TranspositionEntry>>,
}

impl TranspositionTable {
    /// Creates a table sized to roughly 32 MiB of entries.
    pub fn new() -> Self {
        Self::with_size_mb(32)
    }

    /// Creates a table whose entry array fits in exactly `size_mb`
    /// mebibytes (rounded down to whole entries).
    pub fn with_size_mb(size_mb: usize) -> Self {
        const MEGABYTE: usize = 1024 * 1024;
        const ENTRY_SIZE: usize = std::mem::size_of::<TranspositionEntry>();

        let entries = size_mb.saturating_mul(MEGABYTE) / ENTRY_SIZE.max(1);
        Self::with_capacity(entries.max(2))
    }

    /// Reallocates the table for `size_mb` mebibytes, discarding all
    /// existing entries.
    pub fn resize(&mut self, size_mb: usize) {
        *self = Self::with_size_mb(size_mb);
    }

    /// Creates a table with the given number of slots. Any positive
    /// `capacity` works: indexing is multiplication-based, not a mask.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0);

        TranspositionTable {
            entries: vec![None; capacity],
        }
    }

    /// Maps a 64-bit Zobrist key to a slot index in `[0, capacity)` via
    /// the high 64 bits of the 128-bit product `key * capacity`.
    #[inline(always)]
    fn index(&self, key: u64) -> usize {
        ((key as u128 * self.entries.len() as u128) >> 64) as usize
    }

    /// Empties every slot. Call between independent searches (e.g.
    /// `ucinewgame`) when stale data must not influence move ordering.
    pub fn clear(&mut self) {
        self.entries.fill(None);
    }

    /// Looks up `key`. Returns `Some` only when the slot is occupied and
    /// the full key matches, so slot collisions never alias positions.
    pub fn probe(&self, key: u64) -> Option<TranspositionEntry> {
        self.entries[self.index(key)].filter(|entry| entry.key == key)
    }

    /// Inserts `entry`. Keeps an existing same-key entry searched to a
    /// strictly greater depth. Every other case overwrites.
    pub fn store(&mut self, mut entry: TranspositionEntry) {
        let index = self.index(entry.key);

        match self.entries[index] {
            Some(existing) if existing.key == entry.key && existing.depth > entry.depth => {}
            existing => {
                if entry.best_move.is_none() {
                    if let Some(old) = existing.filter(|e| e.key == entry.key) {
                        entry.best_move = old.best_move;
                    }
                }
                self.entries[index] = Some(entry);
            }
        }
    }

    /// Reports the UCI `hashfull` permill (0-1000), estimated from a
    /// fixed-size sample prefix so the call stays O(1).
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
