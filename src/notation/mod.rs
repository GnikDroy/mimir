//! Text-based chess notation formats.
//!
//! - [`fen`]: Forsyth–Edwards Notation — full position serialization.
//! - [`san`]: Standard Algebraic Notation — per-move human-readable encoding.
//! - [`pgn`]: Portable Game Notation — full game documents (header + SAN moves).
//! - [`epd`]: Extended Position Description — position + opcode operations.

pub mod epd;
pub mod fen;
pub mod pgn;
pub mod san;
