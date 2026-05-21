#![feature(variant_count)]
extern crate num_enum;

mod attack_table;
#[macro_use]
mod bitboard;
mod core;
pub use bitboard::BitBoard;
mod board;
mod move_generator;

use move_generator::MoveGenerator;

fn main() {
    MoveGenerator::test_perft();
}
