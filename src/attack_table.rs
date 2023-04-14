use crate::bitboard::*;
use crate::core::*;

use std::cmp::{max, min};

pub struct AttackTable([BitBoard; Piece::NUM * Square::NUM]);

impl AttackTable {
    const FIRST_RANK: BitBoard = 0x00000000000000ff;
    const LAST_RANK: BitBoard = 0xff00000000000000;
    const FIRST_FILE: BitBoard = 0x0101010101010101;
    const LAST_FILE: BitBoard = 0x8080808080808080;
    const FIRST_TWO_FILES: BitBoard = 0x0303030303030303;
    const LAST_TWO_FILES: BitBoard = 0xc0c0c0c0c0c0c0c0;

    pub fn new() -> AttackTable {
        let mut table: [BitBoard; Piece::NUM * Square::NUM] = [0; Piece::NUM * Square::NUM];
        use Piece::*;
        for piece in 0..Piece::NUM {
            for square in 0..Square::NUM {
                let board = BitBoard::on_square(Square::try_from(square).unwrap());
                let board = match Piece::try_from(piece).unwrap() {
                    King => Self::attack_king(board),
                    Queen => Self::attack_queen(board),
                    Rook => Self::attack_rook(board),
                    Bishop => Self::attack_bishop(board),
                    Knight => Self::attack_knight(board),
                    Pawn => Self::attack_pawn(board),
                };
                let idx = Self::get_index(piece, square);
                table[idx] = board;
            }
        }
        AttackTable(table)
    }

    pub fn get(&self, piece: usize, square: usize) -> BitBoard {
        let idx = Self::get_index(piece, square);
        self.0[idx]
    }

    fn get_index(piece: usize, square: usize) -> usize {
        piece * Square::NUM + square
    }

    fn attack_rook(board: BitBoard) -> BitBoard {
        let mut north_attacks = board;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            north_attacks = north_attacks | north_attacks.shift_north();
        }

        let mut south_attacks = board;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            south_attacks = south_attacks | south_attacks.shift_south();
        }

        let mut east_attacks = board;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            east_attacks = east_attacks | (!Self::LAST_FILE & east_attacks).shift_east();
        }

        let mut west_attacks = board;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            west_attacks = west_attacks | (!Self::FIRST_FILE & west_attacks).shift_west();
        }
        north_attacks | south_attacks | east_attacks | west_attacks
    }

    fn attack_bishop(board: BitBoard) -> BitBoard {
        let mut north_east_attacks = board;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            north_east_attacks = north_east_attacks
                | (!(Self::LAST_RANK | Self::LAST_FILE) & north_east_attacks).shift_north_east();
        }

        let mut north_west_attacks = board;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            north_west_attacks = north_west_attacks
                | (!(Self::LAST_RANK | Self::FIRST_FILE) & north_west_attacks).shift_north_west();
        }

        let mut south_east_attacks = board;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            south_east_attacks = south_east_attacks
                | (!(Self::FIRST_RANK | Self::LAST_FILE) & south_east_attacks).shift_south_east();
        }

        let mut south_west_attacks = board;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            south_west_attacks = south_west_attacks
                | (!(Self::FIRST_RANK | Self::FIRST_FILE) & south_west_attacks).shift_south_west();
        }
        north_east_attacks | north_west_attacks | south_east_attacks | south_west_attacks
    }

    fn attack_queen(board: BitBoard) -> BitBoard {
        Self::attack_rook(board) | Self::attack_bishop(board)
    }

    fn attack_knight(board: BitBoard) -> BitBoard {
        (!Self::LAST_FILE & board) << (NUM_FILES * 2 + 1)
            | (!Self::LAST_TWO_FILES & board) << (NUM_FILES + 2)
            | (!Self::LAST_TWO_FILES & board) >> (NUM_FILES - 2)
            | (!Self::LAST_FILE & board) >> (NUM_FILES * 2 - 1)
            | (!Self::FIRST_FILE & board) << (NUM_FILES * 2 - 1)
            | (!Self::FIRST_TWO_FILES & board) << (NUM_FILES - 2)
            | (!Self::FIRST_TWO_FILES & board) >> (NUM_FILES + 2)
            | (!Self::FIRST_FILE & board) >> (NUM_FILES * 2 + 1)
    }

    fn attack_pawn(board: BitBoard) -> BitBoard {
        (!(Self::LAST_FILE | Self::LAST_RANK) & board).shift_north_east()
            | (!(Self::FIRST_FILE | Self::LAST_RANK) & board).shift_north_west()
    }

    fn attack_king(board: BitBoard) -> BitBoard {
        (!Self::LAST_RANK & board).shift_north()
            | (!Self::FIRST_RANK & board).shift_south()
            | (!Self::LAST_FILE & board).shift_east()
            | (!Self::FIRST_FILE & board).shift_west()
            | (!(Self::LAST_RANK | Self::FIRST_FILE) & board).shift_north_west()
            | (!(Self::LAST_RANK | Self::LAST_FILE) & board).shift_north_east()
            | (!(Self::FIRST_RANK | Self::FIRST_FILE) & board).shift_south_west()
            | (!(Self::FIRST_RANK | Self::LAST_FILE) & board).shift_south_east()
    }
}
