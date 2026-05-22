#![feature(variant_count)]

extern crate num_enum;

mod attack_table;

#[macro_use]
mod bitboard;

pub mod core;
pub mod evaluation;
pub mod fen;
pub mod move_generator;
pub mod search;
pub mod state;

pub use attack_table::ATTACK_TABLE;
pub use bitboard::BitBoard;
pub use bitboard::BitBoardIterator;
pub use bitboard::BitBoardMethods;
pub use core::*;
pub use search::{SearchResult, Searcher};
pub use state::{GameState, UndoInfo};
