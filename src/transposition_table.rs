use crate::core::Move;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspositionFlag {
    Exact,
    LowerBound,
    UpperBound,
}

#[derive(Debug, Clone, Copy)]
pub struct TranspositionEntry {
    pub key: u64,
    pub depth: u8,
    pub score: i32,
    pub flag: TranspositionFlag,
    pub best_move: Option<Move>,
}

pub struct TranspositionTable {
    entries: Vec<Option<TranspositionEntry>>,
}

impl TranspositionTable {
    pub fn new() -> Self {
        const SIZE_IN_MB: usize = 16;
        const ENTRY_SIZE: usize = std::mem::size_of::<TranspositionEntry>();
        const DEFAULT_TABLE_SIZE: usize =
            ((SIZE_IN_MB * 1024 * 1024) / ENTRY_SIZE).next_power_of_two();
        Self::with_capacity(DEFAULT_TABLE_SIZE)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two());

        TranspositionTable {
            entries: vec![None; capacity],
        }
    }

    #[inline(always)]
    fn index(&self, key: u64) -> usize {
        (key as usize) & (self.entries.len() - 1)
    }

    pub fn clear(&mut self) {
        self.entries.fill(None);
    }

    pub fn probe(&self, key: u64) -> Option<TranspositionEntry> {
        self.entries[self.index(key)].filter(|entry| entry.key == key)
    }

    pub fn store(&mut self, entry: TranspositionEntry) {
        let index = self.index(entry.key);

        match self.entries[index] {
            Some(existing) if existing.key == entry.key && existing.depth > entry.depth => {}
            _ => self.entries[index] = Some(entry),
        }
    }
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::new()
    }
}
