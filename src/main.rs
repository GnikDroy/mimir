#![feature(variant_count)]
extern crate num_enum;

mod attack_table;
#[macro_use]
mod bitboard;
mod core;
pub use bitboard::BitBoard;
mod fen;
mod move_generator;
mod state;

fn main() {}
