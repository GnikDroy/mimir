#![feature(variant_count)]

extern crate num_enum;

#[macro_use]
mod bitboard;

mod attack_table;

pub mod core;
pub mod state;

pub mod fen;
pub mod pgn;

pub mod evaluation;
pub mod move_generator;
pub mod search;
pub mod zobrist;

pub use attack_table::ATTACK_TABLE;
pub use bitboard::BitBoard;
pub use bitboard::BitBoardIterator;
pub use bitboard::BitBoardMethods;
pub use core::*;
pub use search::{SearchResult, Searcher};
pub use state::{GameState, UndoInfo};
