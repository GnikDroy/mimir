use once_cell::sync::Lazy;
use rand::prelude::*;

use crate::bitboard::*;
use crate::core::*;

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
    white_pawn: [BitBoard; Square::NUM],
    black_pawn: [BitBoard; Square::NUM],
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
        let white_pawn = simple_pieces_moves(Self::attack_pawn);

        let sliding_pieces_moves =
            |move_generator: fn(BitBoard, BitBoard) -> BitBoard,
             mask_generator: fn(BitBoard) -> BitBoard| {
                let tables: [MagicTable; Square::NUM] = Square::all()
                    .map(|square| {
                        let board = BitBoard::on(square);
                        let mask = mask_generator(board) & !board;
                        let index_bits = mask.count_ones();
                        Self::find_magic(move_generator, mask, board, 64 - index_bits as u8)
                    })
                    .collect::<Vec<MagicTable>>()
                    .try_into()
                    .unwrap();
                tables
            };
        let rook = sliding_pieces_moves(Self::attack_rook, Self::mask_rook);
        let bishop = sliding_pieces_moves(Self::attack_bishop, Self::mask_bishop);

        let mut black_pawn = [BitBoard::EMPTY; 64];
        for square in Square::all() {
            black_pawn[square.flip_vertical() as usize] = white_pawn[square as usize].flip_ranks();
        }

        AttackTable {
            king,
            white_pawn,
            black_pawn,
            knight,
            bishop,
            rook,
        }
    }

    pub fn get_pawn(&self, square: Square, color: Color) -> BitBoard {
        match color {
            Color::White => self.white_pawn[square as usize],
            Color::Black => self.black_pawn[square as usize],
        }
    }

    pub fn get_king(&self, square: Square) -> BitBoard {
        self.king[square as usize]
    }

    pub fn get_knight(&self, square: Square) -> BitBoard {
        self.knight[square as usize]
    }

    pub fn get_bishop(&self, square: Square, blockers: BitBoard) -> BitBoard {
        let table = &self.bishop[square as usize];
        table.boards[Self::index_table(&table.entry, blockers)]
    }

    pub fn get_rook(&self, square: Square, blockers: BitBoard) -> BitBoard {
        let table = &self.rook[square as usize];
        table.boards[Self::index_table(&table.entry, blockers)]
    }

    pub fn get_queen(&self, square: Square, blockers: BitBoard) -> BitBoard {
        let rook_table = &self.rook[square as usize];
        let bishop_table = &self.bishop[square as usize];
        let rook_moves = rook_table.boards[Self::index_table(&rook_table.entry, blockers)];
        let bishop_moves = bishop_table.boards[Self::index_table(&bishop_table.entry, blockers)];
        rook_moves | bishop_moves
    }

    fn index_table(entry: &MagicEntry, blockers: BitBoard) -> usize {
        let blockers = blockers & entry.mask;
        let hash = blockers.wrapping_mul(entry.magic);
        (hash >> entry.shift) as usize
    }

    fn find_magic(
        move_generator: fn(BitBoard, BitBoard) -> BitBoard,
        mask: BitBoard,
        board: BitBoard,
        shift: u8,
    ) -> MagicTable {
        let index_bits = 64 - shift;
        let table_size = 1usize << index_bits;

        // Precompute all blocker subsets + move boards once.
        let mut occupancies = Vec::new();
        let mut moves = Vec::new();
        let mut blockers = BitBoard::EMPTY;

        loop {
            occupancies.push(blockers);
            moves.push(move_generator(board, blockers));
            blockers = blockers.wrapping_sub(mask) & mask;

            if blockers == BitBoard::EMPTY {
                break;
            }
        }

        let mut rng = rand::rng();
        // Reused buffers.
        let mut table = vec![BitBoard::EMPTY; table_size];
        let mut used = vec![0u32; table_size];
        let mut generation = 1u32;

        loop {
            // Sparse magics tend to work better.
            let magic = rng.random::<u64>() & rng.random::<u64>() & rng.random::<u64>();
            let entry = MagicEntry { mask, magic, shift };

            if Self::try_make_table(
                &entry,
                &occupancies,
                &moves,
                &mut table,
                &mut used,
                generation,
            ) {
                return MagicTable {
                    entry,
                    boards: table,
                };
            }

            generation = generation.wrapping_add(1);
        }
    }

    fn try_make_table(
        entry: &MagicEntry,
        occupancies: &[BitBoard],
        moves: &[BitBoard],
        table: &mut [BitBoard],
        used: &mut [u32],
        generation: u32,
    ) -> bool {
        for (&blockers, &moveset) in occupancies.iter().zip(moves) {
            let idx = Self::index_table(entry, blockers);
            if used[idx] != generation {
                used[idx] = generation;
                table[idx] = moveset;
            } else if table[idx] != moveset {
                return false;
            }
        }
        true
    }

    fn slide_north(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_north();
        let mut shift = board.shift_north() & !blockers;
        for _ in 0..Rank::NUM {
            result |= shift;
            shift = shift.shift_north() & !blockers;
        }
        result
    }

    fn slide_south(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_south();
        let mut shift = board.shift_south() & !blockers;
        for _ in 0..Rank::NUM {
            result |= shift;
            shift = shift.shift_south() & !blockers;
        }
        result
    }

    fn slide_east(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_east();
        let mut shift = board.shift_east() & !blockers;
        for _ in 0..File::NUM {
            result |= shift;
            shift = shift.shift_east() & !blockers;
        }
        result
    }

    fn slide_west(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_west();
        let mut shift = board.shift_west() & !blockers;
        for _ in 0..File::NUM {
            result |= shift;
            shift = shift.shift_west() & !blockers;
        }
        result
    }

    fn slide_north_east(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_north_east();
        let mut shift = board.shift_north_east() & !blockers;
        for _ in 0..Rank::NUM {
            result |= shift;
            shift = shift.shift_north_east() & !blockers;
        }
        result
    }

    fn slide_north_west(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_north_west();
        let mut shift = board.shift_north_west() & !blockers;
        for _ in 0..Rank::NUM {
            result |= shift;
            shift = shift.shift_north_west() & !blockers;
        }
        result
    }

    fn slide_south_east(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_south_east();
        let mut shift = board.shift_south_east() & !blockers;
        for _ in 0..Rank::NUM {
            result |= shift;
            shift = shift.shift_south_east() & !blockers;
        }
        result
    }

    fn slide_south_west(board: BitBoard, blockers: BitBoard) -> BitBoard {
        let mut result = BitBoard::EMPTY;
        let blockers = blockers.shift_south_west();
        let mut shift = board.shift_south_west() & !blockers;
        for _ in 0..Rank::NUM {
            result |= shift;
            shift = shift.shift_south_west() & !blockers;
        }
        result
    }

    fn attack_knight(board: BitBoard) -> BitBoard {
        (!Self::LAST_FILE & board) << (File::NUM * 2 + 1)
            | (!Self::LAST_TWO_FILES & board) << (File::NUM + 2)
            | (!Self::LAST_TWO_FILES & board) >> (File::NUM - 2)
            | (!Self::LAST_FILE & board) >> (File::NUM * 2 - 1)
            | (!Self::FIRST_FILE & board) << (File::NUM * 2 - 1)
            | (!Self::FIRST_TWO_FILES & board) << (File::NUM - 2)
            | (!Self::FIRST_TWO_FILES & board) >> (File::NUM + 2)
            | (!Self::FIRST_FILE & board) >> (File::NUM * 2 + 1)
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

    fn mask_rook(board: BitBoard) -> BitBoard {
        (Self::slide_north(board, BitBoard::EMPTY) & !Self::LAST_RANK)
            | (Self::slide_south(board, BitBoard::EMPTY) & !Self::FIRST_RANK)
            | (Self::slide_east(board, BitBoard::EMPTY) & !Self::LAST_FILE)
            | (Self::slide_west(board, BitBoard::EMPTY) & !Self::FIRST_FILE)
    }

    fn mask_bishop(board: BitBoard) -> BitBoard {
        Self::slide_north_east(board, BitBoard::EMPTY) & !(Self::LAST_RANK | Self::LAST_FILE)
            | Self::slide_north_west(board, BitBoard::EMPTY) & !(Self::LAST_RANK | Self::FIRST_FILE)
            | Self::slide_south_east(board, BitBoard::EMPTY) & !(Self::FIRST_RANK | Self::LAST_FILE)
            | Self::slide_south_west(board, BitBoard::EMPTY)
                & !(Self::FIRST_RANK | Self::FIRST_FILE)
    }
}

pub static ATTACK_TABLE: Lazy<AttackTable> = Lazy::new(|| AttackTable::new());

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
    fn slide_north_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_north(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . X . . .
                . . . . X . . .
                . . . . X . . .
                . . . . X . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );

        let result = AttackTable::slide_north(board, BitBoard::on(Square::E6));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . X . . .
                . . . . X . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_south_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_south(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . X . . .
                . . . . X . . .
                . . . . X . . .
            }
        );

        let result = AttackTable::slide_south(board, BitBoard::on(Square::E2));

        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . X . . .
                . . . . X . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_east_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_east(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . X X X
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );

        let result = AttackTable::slide_east(board, BitBoard::on(Square::G4));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . X X .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_west_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_west(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                X X X X . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );

        let result = AttackTable::slide_west(board, BitBoard::on(Square::C4));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . X X . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_north_east_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_north_east(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . X
                . . . . . . X .
                . . . . . X . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );

        let result = AttackTable::slide_north_east(board, BitBoard::on(Square::G6));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . X .
                . . . . . X . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_north_west_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_north_west(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                X . . . . . . .
                . X . . . . . .
                . . X . . . . .
                . . . X . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );

        let result = AttackTable::slide_north_west(board, BitBoard::on(Square::C6));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . X . . . . .
                . . . X . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_south_east_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_south_east(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . X . .
                . . . . . . X .
                . . . . . . . X
            }
        );

        let result = AttackTable::slide_south_east(board, BitBoard::on(Square::G2));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . X . .
                . . . . . . X .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn slide_south_west_test() {
        let board = BitBoard::on(Square::E4);
        let result = AttackTable::slide_south_west(board, BitBoard::EMPTY);
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . X . . . .
                . . X . . . . .
                . X . . . . . .
            }
        );

        let result = AttackTable::slide_south_west(board, BitBoard::on(Square::C2));
        assert_eq!(
            result,
            bitboard! {
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . . . . . .
                . . . X . . . .
                . . X . . . . .
                . . . . . . . .
            }
        );
    }

    #[test]
    fn attack_bishop() {
        attack_slider_generic(
            AttackTable::attack_bishop,
            BitBoard::on(Square::E4),
            BitBoard::on(Square::C6) | BitBoard::on(Square::G2),
            bitboard! {
            . . . . . . . .
            . . . . . . . X
            . . X . . . X .
            . . . X . X . .
            . . . . . . . .
            . . . X . X . .
            . . X . . . X .
            . X . . . . . .
            },
        );

        attack_slider_generic(
            AttackTable::attack_bishop,
            BitBoard::on(Square::E1),
            BitBoard::on(Square::H4) | BitBoard::on(Square::B4),
            bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . X . . . . . X
            . . X . . . X .
            . . . X . X . .
            . . . . . . . .
            },
        );

        attack_slider_generic(
            AttackTable::attack_bishop,
            BitBoard::on(Square::H8),
            BitBoard::on(Square::G7),
            bitboard! {
            . . . . . . . .
            . . . . . . X .
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
            . . . X . X X X
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
            . . . . X . . .
            . . . . X . . .
            . . . . X . . .
            . . . . X . . .
            X X X X . X X .
            },
        );

        attack_slider_generic(
            AttackTable::attack_rook,
            BitBoard::on(Square::H8),
            BitBoard::on(Square::G8) | BitBoard::on(Square::H7),
            bitboard! {
            . . . . . . X .
            . . . . . . . X
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
