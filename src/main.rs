#![feature(variant_count)]
extern crate num_enum;

mod attack_table;
mod bitboard;
mod core;

use crate::attack_table::AttackTable;
use crate::bitboard::*;
use crate::core::*;

pub struct PieceBitBoards([BitBoard; Color::NUM * Piece::NUM]);

impl PieceBitBoards {
    fn get_index(color: Color, piece: Piece) -> usize {
        piece as usize * Color::NUM + color as usize
    }

    pub fn get(&self, color: Color, piece: Piece) -> BitBoard {
        self.0[PieceBitBoards::get_index(color, piece)]
    }

    pub fn get_mut(&mut self, color: Color, piece: Piece) -> &mut BitBoard {
        &mut self.0[PieceBitBoards::get_index(color, piece)]
    }
}

fn main() {
    let attacks = AttackTable::new();
    for square in 0..Square::NUM {
        for piece in 0..Piece::NUM {
            println!("{}", attacks.get(piece, square).repr_string());
        }
    }
}
