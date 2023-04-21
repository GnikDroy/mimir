use rand::RngCore;

use crate::bitboard::*;
use crate::core::*;

use std::cmp::{max, min};

#[derive(Debug, Default)]
struct MagicEntry {
    mask: BitBoard,
    magic: u64,
    shift: u8,
}

#[derive(Debug, Default)]
struct MagicTable {
    entry: MagicEntry,
    boards: Vec<BitBoard>,
}

pub struct AttackTable {
    king: [BitBoard; Square::NUM],
    pawn: [BitBoard; Square::NUM],
    knight: [BitBoard; Square::NUM],
    bishop: [MagicTable; Square::NUM],
    rook: [MagicTable; Square::NUM],
}

impl AttackTable {
    const FIRST_RANK: BitBoard = 0x00000000000000ff;
    const LAST_RANK: BitBoard = 0xff00000000000000;
    const FIRST_FILE: BitBoard = 0x0101010101010101;
    const LAST_FILE: BitBoard = 0x8080808080808080;
    const FIRST_TWO_FILES: BitBoard = 0x0303030303030303;
    const LAST_TWO_FILES: BitBoard = 0xc0c0c0c0c0c0c0c0;

    pub fn new() -> AttackTable {
        let simple_pieces_moves =
            |move_generator: fn(BitBoard) -> BitBoard| -> [BitBoard; Square::NUM] {
                let mut piece: [BitBoard; Square::NUM] = [0; Square::NUM];
                for square in Square::all() {
                    let board = move_generator(BitBoard::on(square));
                    piece[square as usize] = board;
                }
                piece
            };

        let king = simple_pieces_moves(Self::attack_king);
        let knight = simple_pieces_moves(Self::attack_knight);
        let pawn = simple_pieces_moves(Self::attack_pawn);

        let sliding_pieces_moves = |move_generator: fn(BitBoard, BitBoard) -> BitBoard| {
            let tables: [MagicTable; Square::NUM] = Square::all()
                .map(|square| {
                    let board = BitBoard::on(square);
                    let border_mask =
                        Self::FIRST_FILE | Self::LAST_FILE | Self::FIRST_RANK | Self::LAST_RANK;
                    let mask = move_generator(board, BitBoard::EMPTY) & !border_mask;
                    let index_bits = mask.count_ones();
                    Self::find_magic(move_generator, board, 64 - index_bits as u8)
                })
                .collect::<Vec<MagicTable>>()
                .try_into()
                .unwrap();
            tables
        };
        let rook = sliding_pieces_moves(Self::attack_rook);
        let bishop = sliding_pieces_moves(Self::attack_bishop);

        AttackTable {
            king,
            pawn,
            knight,
            bishop,
            rook,
        }
    }

    pub fn get(&self, piece: Piece, square: Square, blockers: BitBoard) -> BitBoard {
        match piece {
            Piece::King => self.king[square as usize] & !blockers,
            Piece::Pawn => self.pawn[square as usize] & !blockers,
            Piece::Knight => self.knight[square as usize] & !blockers,
            Piece::Rook => {
                let table = &self.rook[square as usize];
                table.boards[Self::magic_index(&table.entry, blockers)]
            }
            Piece::Bishop => {
                let table = &self.bishop[square as usize];
                table.boards[Self::magic_index(&table.entry, blockers)]
            }
            Piece::Queen => {
                let rook_table = &self.rook[square as usize];
                let bishop_table = &self.bishop[square as usize];
                let rook_moves = rook_table.boards[Self::magic_index(&rook_table.entry, blockers)];
                let bishop_moves =
                    bishop_table.boards[Self::magic_index(&bishop_table.entry, blockers)];
                rook_moves | bishop_moves
            }
        }
    }

    fn magic_index(entry: &MagicEntry, blockers: BitBoard) -> usize {
        let blockers = blockers & entry.mask;
        let hash = blockers.wrapping_mul(entry.magic);
        (hash >> entry.shift) as usize
    }

    fn find_magic(
        move_generator: fn(BitBoard, BitBoard) -> BitBoard,
        board: BitBoard,
        shift: u8,
    ) -> MagicTable {
        let mut rng = rand::thread_rng();
        let border_mask = Self::FIRST_FILE | Self::LAST_FILE | Self::FIRST_RANK | Self::LAST_RANK;
        let mask = move_generator(board, BitBoard::EMPTY) & !border_mask & !board & !board;
        loop {
            // low number of enabled bits is preferred
            let magic = rng.next_u64() & rng.next_u64() & rng.next_u64();
            let entry = MagicEntry { mask, magic, shift };
            if let Some(boards) = Self::try_make_table(move_generator, board, &entry) {
                return MagicTable { entry, boards };
            }
        }
    }

    fn try_make_table(
        move_generator: fn(BitBoard, BitBoard) -> BitBoard,
        board: BitBoard,
        entry: &MagicEntry,
    ) -> Option<Vec<BitBoard>> {
        let index_bits = 64 - entry.shift;
        let mut table = vec![BitBoard::EMPTY; 1 << index_bits];
        let mut blockers = BitBoard::EMPTY;
        loop {
            let moves = move_generator(board, blockers);
            let table_entry = &mut table[Self::magic_index(&entry, blockers)];
            if *table_entry == BitBoard::EMPTY {
                *table_entry = moves;
            } else if *table_entry != moves {
                return None;
            }

            // https://www.chessprogramming.org/Traversing_Subsets_of_a_Set#All_Subsets_of_any_Set
            blockers = blockers.wrapping_sub(entry.mask) & entry.mask;
            if blockers == BitBoard::EMPTY {
                break;
            }
        }
        Some(table)
    }

    fn slide_north(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut north_attacks = board & !blockers;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            north_attacks = north_attacks | north_attacks.shift_north() & !blockers;
        }
        north_attacks
    }

    fn slide_south(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut south_attacks = board & !blockers;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            south_attacks = south_attacks | south_attacks.shift_south() & !blockers;
        }
        south_attacks
    }

    fn slide_east(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut east_attacks = board & !blockers;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            east_attacks =
                east_attacks | (!Self::LAST_FILE & east_attacks).shift_east() & !blockers;
        }
        east_attacks
    }

    fn slide_west(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut west_attacks = board & !blockers;
        for _ in 1..max(NUM_FILES, NUM_RANKS) {
            west_attacks =
                west_attacks | (!Self::FIRST_FILE & west_attacks).shift_west() & !blockers;
        }
        west_attacks
    }

    fn slide_north_east(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut north_east_attacks = board & !blockers;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            north_east_attacks = north_east_attacks
                | (!(Self::LAST_RANK | Self::LAST_FILE) & north_east_attacks).shift_north_east()
                    & !blockers;
        }
        north_east_attacks
    }

    fn slide_north_west(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut north_west_attacks = board & !blockers;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            north_west_attacks = north_west_attacks
                | (!(Self::LAST_RANK | Self::FIRST_FILE) & north_west_attacks).shift_north_west()
                    & !blockers;
        }
        north_west_attacks
    }

    fn slide_south_east(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut south_east_attacks = board & !blockers;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            south_east_attacks = south_east_attacks
                | (!(Self::FIRST_RANK | Self::LAST_FILE) & south_east_attacks).shift_south_east()
                    & !blockers;
        }
        south_east_attacks
    }

    fn slide_south_west(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut south_west_attacks = board & !blockers;
        for _ in 1..min(NUM_FILES, NUM_RANKS) {
            south_west_attacks = south_west_attacks
                | (!(Self::FIRST_RANK | Self::FIRST_FILE) & south_west_attacks).shift_south_west()
                    & !blockers;
        }
        south_west_attacks
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

    fn attack_bishop(board: BitBoard, blockers: BitBoard) -> BitBoard {
        Self::slide_north_east(board, blockers)
            | Self::slide_north_west(board, blockers)
            | Self::slide_south_east(board, blockers)
            | Self::slide_south_west(board, blockers)
    }

    fn attack_rook(board: BitBoard, blockers: BitBoard) -> BitBoard {
        Self::slide_north(board, blockers)
            | Self::slide_south(board, blockers)
            | Self::slide_east(board, blockers)
            | Self::slide_west(board, blockers)
    }
}

#[cfg(test)]
mod tests {
    use super::AttackTable;
    use crate::bitboard;
    use crate::bitboard::*;
    use crate::core::*;

    #[test]
    fn attack_sliders_degenerates() {
        for square in Square::all() {
            let board = BitBoard::on(square);
            for slider in [AttackTable::attack_rook, AttackTable::attack_bishop] {
                let blockers = BitBoard::on(square);
                let attacks = slider(board, blockers);
                assert_eq!(attacks, BitBoard::EMPTY);
            }
        }
    }

    fn attack_slider_generic(
        generator: fn(BitBoard, BitBoard) -> BitBoard,
        board: BitBoard,
        blockers: BitBoard,
        expected: BitBoard,
    ) {
        let attacks = generator(board, blockers);
        assert_eq!(attacks, expected);

        let attacks = generator(board.flip_files(), blockers.flip_files());
        assert_eq!(attacks, expected.flip_files());

        let attacks = generator(board.flip_ranks(), blockers.flip_ranks());
        assert_eq!(attacks, expected.flip_ranks());

        let attacks = generator(
            board.flip_files().flip_ranks(),
            blockers.flip_files().flip_ranks(),
        );
        assert_eq!(attacks, expected.flip_files().flip_ranks());
    }

    fn attack_simple_generic(
        generator: fn(BitBoard) -> BitBoard,
        board: BitBoard,
        expected: BitBoard,
    ) {
        let attacks = generator(board);
        assert_eq!(attacks, expected);

        let attacks = generator(board.flip_files());
        assert_eq!(attacks, expected.flip_files());

        let attacks = generator(board.flip_ranks());
        assert_eq!(attacks, expected.flip_ranks());

        let attacks = generator(board.flip_files().flip_ranks());
        assert_eq!(attacks, expected.flip_files().flip_ranks());
    }

    #[test]
    fn attack_king() {
        attack_simple_generic(
            AttackTable::attack_king,
            BitBoard::on(Square::E4),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . X X X . .
            . . . X . X . .
            . . . X X X . .
            . . . . . . . .
            . . . . . . . .
            },
        );

        attack_simple_generic(
            AttackTable::attack_king,
            BitBoard::on(Square::A1),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            X X . . . . . .
            . X . . . . . .
            },
        );

        attack_simple_generic(
            AttackTable::attack_king,
            BitBoard::on(Square::E1),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . X X X . .
            . . . X . X . .
            },
        );
    }

    #[test]
    fn attack_knight() {
        attack_simple_generic(
            AttackTable::attack_knight,
            BitBoard::on(Square::E4),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . X . X . .
            . . X . . . X .
            . . . . . . . .
            . . X . . . X .
            . . . X . X . .
            . . . . . . . .
            },
        );

        attack_simple_generic(
            AttackTable::attack_knight,
            BitBoard::on(Square::A1),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . X . . . . . .
            . . X . . . . .
            . . . . . . . .
            },
        );

        attack_simple_generic(
            AttackTable::attack_knight,
            BitBoard::on(Square::E1),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . X . X . .
            . . X . . . X .
            . . . . . . . .
            },
        );
    }

    #[test]
    fn attack_pawn() {
        let attack_simple_generic =
            |generator: fn(BitBoard) -> BitBoard, board: BitBoard, expected: BitBoard| {
                let attacks = generator(board);
                assert_eq!(attacks, expected);

                let attacks = generator(board.flip_files());
                assert_eq!(attacks, expected.flip_files());
            };

        attack_simple_generic(
            AttackTable::attack_pawn,
            BitBoard::on(Square::E5),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . X . X . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            },
        );

        attack_simple_generic(
            AttackTable::attack_pawn,
            BitBoard::on(Square::A2),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . X . . . . . .
            . . . . . . . .
            . . . . . . . .
            },
        );

        attack_simple_generic(
            AttackTable::attack_pawn,
            BitBoard::on(Square::E7),
            bitboard! {
            . . . X . X . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            },
        );
    }

    #[test]
    fn attack_bishop() {
        attack_slider_generic(
            AttackTable::attack_bishop,
            BitBoard::on(Square::E4),
            BitBoard::on(Square::C6) | BitBoard::on(Square::H1),
            bitboard! {
            . . . . . . . .
            . . . . . . . X
            . . . . . . X .
            . . . X . X . .
            . . . . X . . .
            . . . X . X . .
            . . X . . . X .
            . X . . . . . .
            },
        );

        attack_slider_generic(
            AttackTable::attack_bishop,
            BitBoard::on(Square::E1),
            BitBoard::on(Square::H4) | BitBoard::on(Square::A5),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . X . . . . . .
            . . X . . . X .
            . . . X . X . .
            . . . . X . . .
            },
        );

        attack_slider_generic(
            AttackTable::attack_bishop,
            BitBoard::on(Square::H8),
            BitBoard::on(Square::G7),
            bitboard! {
            . . . . . . . X
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            },
        );
    }

    #[test]
    fn attack_rook() {
        attack_slider_generic(
            AttackTable::attack_rook,
            BitBoard::on(Square::E4),
            BitBoard::on(Square::D4),
            bitboard! {
            . . . . X . . .
            . . . . X . . .
            . . . . X . . .
            . . . . X . . .
            . . . . X X X X
            . . . . X . . .
            . . . . X . . .
            . . . . X . . .
            },
        );

        attack_slider_generic(
            AttackTable::attack_rook,
            BitBoard::on(Square::E1),
            BitBoard::on(Square::G1) | BitBoard::on(Square::E5),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . X . . .
            . . . . X . . .
            . . . . X . . .
            X X X X X X . .
            },
        );

        attack_slider_generic(
            AttackTable::attack_rook,
            BitBoard::on(Square::H8),
            BitBoard::on(Square::G8) | BitBoard::on(Square::H7),
            bitboard! {
            . . . . . . . X
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            },
        );
    }
}
