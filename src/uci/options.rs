//! Configurable engine options exposed over UCI.
//!
//! [`EngineOptions`] is the engine-wide settings struct: the UCI layer
//! advertises each field as a `setoption` parameter during the `uci`
//! handshake and mutates them when the GUI sends `setoption name <...>`.
//! Other modules read the current values when they construct dependent
//! components (transposition table, time control, ...).
//!
//! Option names are matched case-insensitively to match common GUI
//! behavior; unknown names and out-of-range values are silently ignored
//! per the UCI spec.

use std::io::{self, Write};
use std::time::Duration;

/// `Hash` (MB) — transposition table size advertised to the GUI.
pub const HASH_DEFAULT_MB: usize = 32;
pub const HASH_MIN_MB: usize = 1;
pub const HASH_MAX_MB: usize = 4096;

/// `Move Overhead` (ms) — safety buffer subtracted from each move's
/// time budget to absorb UCI / GUI round-trip latency.
pub const MOVE_OVERHEAD_DEFAULT_MS: u64 = 50;
pub const MOVE_OVERHEAD_MIN_MS: u64 = 0;
pub const MOVE_OVERHEAD_MAX_MS: u64 = 5000;

/// Runtime-configurable engine settings.
#[derive(Debug, Clone, Copy)]
pub struct EngineOptions {
    /// Requested transposition table size in mebibytes.
    pub hash_mb: usize,
    /// Time deducted from every move's budget as a UCI safety buffer.
    pub move_overhead: Duration,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineOptions {
    pub fn new() -> Self {
        Self {
            hash_mb: HASH_DEFAULT_MB,
            move_overhead: Duration::from_millis(MOVE_OVERHEAD_DEFAULT_MS),
        }
    }

    /// Applies a `setoption` update.
    ///
    /// Returns `true` if `name` matched a known option and `value`
    /// parsed within range. Unknown names, missing values, unparseable
    /// numbers, and out-of-range values all return `false` and leave
    /// the struct unchanged.
    pub fn set(&mut self, name: &str, value: Option<&str>) -> bool {
        let value = match value {
            Some(v) => v.trim(),
            None => return false,
        };

        match name.trim().to_ascii_lowercase().as_str() {
            "hash" => match value.parse::<usize>() {
                Ok(mb) if (HASH_MIN_MB..=HASH_MAX_MB).contains(&mb) => {
                    self.hash_mb = mb;
                    true
                }
                _ => false,
            },
            "move overhead" => match value.parse::<u64>() {
                Ok(ms) if (MOVE_OVERHEAD_MIN_MS..=MOVE_OVERHEAD_MAX_MS).contains(&ms) => {
                    self.move_overhead = Duration::from_millis(ms);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Writes the `option` advertisement lines emitted as part of the
    /// `uci` handshake reply.
    pub fn write_uci_advertisements<W: Write>(writer: &mut W) -> io::Result<()> {
        writeln!(
            writer,
            "option name Hash type spin default {} min {} max {}",
            HASH_DEFAULT_MB, HASH_MIN_MB, HASH_MAX_MB
        )?;
        writeln!(
            writer,
            "option name Move Overhead type spin default {} min {} max {}",
            MOVE_OVERHEAD_DEFAULT_MS, MOVE_OVERHEAD_MIN_MS, MOVE_OVERHEAD_MAX_MS
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let opts = EngineOptions::new();
        assert_eq!(opts.hash_mb, HASH_DEFAULT_MB);
        assert_eq!(
            opts.move_overhead,
            Duration::from_millis(MOVE_OVERHEAD_DEFAULT_MS)
        );
    }

    #[test]
    fn test_set_hash() {
        let mut opts = EngineOptions::new();
        assert!(opts.set("Hash", Some("128")));
        assert_eq!(opts.hash_mb, 128);
    }

    #[test]
    fn test_set_hash_case_insensitive() {
        let mut opts = EngineOptions::new();
        assert!(opts.set("hash", Some("64")));
        assert_eq!(opts.hash_mb, 64);
    }

    #[test]
    fn test_set_move_overhead() {
        let mut opts = EngineOptions::new();
        assert!(opts.set("Move Overhead", Some("200")));
        assert_eq!(opts.move_overhead, Duration::from_millis(200));
    }

    #[test]
    fn test_set_rejects_out_of_range() {
        let mut opts = EngineOptions::new();
        assert!(!opts.set("Hash", Some("0")));
        assert!(!opts.set("Hash", Some("99999999")));
        assert_eq!(opts.hash_mb, HASH_DEFAULT_MB);
    }

    #[test]
    fn test_set_rejects_unparseable() {
        let mut opts = EngineOptions::new();
        assert!(!opts.set("Hash", Some("abc")));
        assert!(!opts.set("Move Overhead", Some("")));
    }

    #[test]
    fn test_set_rejects_unknown_name() {
        let mut opts = EngineOptions::new();
        assert!(!opts.set("Ponder", Some("true")));
    }

    #[test]
    fn test_set_rejects_missing_value() {
        let mut opts = EngineOptions::new();
        assert!(!opts.set("Hash", None));
    }

    #[test]
    fn test_write_uci_advertisements() {
        let mut buf = Vec::new();
        EngineOptions::write_uci_advertisements(&mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("option name Hash type spin"));
        assert!(out.contains("option name Move Overhead type spin"));
    }
}
