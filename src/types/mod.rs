//! Shared chess primitives used across move generation, search, and I/O.
//!
//! Each enum is `#[repr(u8)]` with contiguous discriminants starting at zero,
//! so the discriminant doubles as a stable array/lookup index. The
//! [`Square`] ordering is `A1 = 0` with files varying fastest (file then
//! rank), which is the layout assumed by every bitboard and PST in the
//! codebase.

pub mod color;
pub mod file;
pub mod moves;
pub mod piece;
pub mod promotion_piece;
pub mod rank;
pub mod square;

pub use color::Color;
pub use file::File;
pub use moves::{Move, MoveList, MoveMethods, MoveType, MAX_MOVE_COUNT};
pub use piece::Piece;
pub use promotion_piece::PromotionPiece;
pub use rank::Rank;
pub use square::Square;
