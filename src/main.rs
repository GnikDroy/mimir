#![feature(variant_count)]
extern crate num_enum;

mod attack_table;
#[macro_use]
mod bitboard;
mod core;
pub use bitboard::BitBoard;
mod board;
mod move_generator;

use board::GameState;
use move_generator::MoveGenerator;

fn main() {
    let state = GameState::starting_position();
    let gen = MoveGenerator::new();

    let moves = gen.generate_moves(&state);

    println!("Starting position: {} pseudo-legal moves", moves.len());
}
