//! Precomputed attack tables for every piece type.
//!
//! Non-sliding pieces (king, knight, pawn) get a flat `[BitBoard; 64]`
//! keyed by source square. Sliding pieces (rook, bishop, queen) use
//! **magic bitboards**: for each square we store a relevant-blocker mask,
//! a magic multiplier, and a right-shift that together hash any blocker
//! subset into a dense move-board lookup table.
//!
//! Magics are searched at startup with [`AttackTable::find_magic`] using
//! sparse random candidates. The global [`ATTACK_TABLE`] is a
//! [`Lazy`] singleton.

use once_cell::sync::Lazy;
use rand::prelude::*;

use crate::bitboard::*;
use crate::core::*;

/// Per-square magic-bitboard parameters for a sliding piece.
#[derive(Debug, Default)]
struct MagicEntry {
    /// Relevant-blocker mask: squares that can affect this slider's
    /// attacks from the source square (excludes the source itself and
    /// the board edges along each ray).
    mask: BitBoard,
    /// 64-bit multiplier that distributes masked blocker subsets into
    /// distinct upper bits.
    magic: u64,
    /// Right-shift applied to `blockers * magic` to extract the index.
    /// Always `64 - mask.count_ones()`.
    shift: u8,
}

/// A magic table for one sliding-piece source square.
///
/// `boards` is dense and indexed by the hash computed from
/// [`AttackTable::index_table`]; collisions are allowed only when the
/// stored move-board already matches (verified during magic search).
#[derive(Debug, Default)]
struct MagicTable {
    /// Magic parameters used to index `boards`.
    entry: MagicEntry,
    /// Precomputed attack boards, one per masked-blocker hash slot.
    boards: Vec<BitBoard>,
}

/// Lookup tables for every piece's attack set from every square.
///
/// All fields are private; callers use the [`get_pawn`](Self::get_pawn),
/// [`get_king`](Self::get_king), [`get_knight`](Self::get_knight),
/// [`get_bishop`](Self::get_bishop), [`get_rook`](Self::get_rook), and
/// [`get_queen`](Self::get_queen) accessors. The non-sliding arrays are
/// indexed directly by [`Square`]; the sliding arrays go through a
/// magic-bitboard hash that also incorporates the current `blockers`.
pub struct AttackTable {
    king: [BitBoard; Square::NUM],
    white_pawn: [BitBoard; Square::NUM],
    black_pawn: [BitBoard; Square::NUM],
    knight: [BitBoard; Square::NUM],
    bishop: [MagicTable; Square::NUM],
    rook: [MagicTable; Square::NUM],
    /// `line[a][b]`: the full rank, file, or diagonal containing both
    /// squares (including `a` and `b`), or `0` if they don't share a ray.
    /// Used for pin-ray lookup during legal move generation.
    line: [[BitBoard; Square::NUM]; Square::NUM],
    /// `between[a][b]`: squares strictly between `a` and `b` on a shared
    /// ray (exclusive of both endpoints), or `0` if they don't share a
    /// ray. Used to build the check-mask for slider checks.
    between: [[BitBoard; Square::NUM]; Square::NUM],
}

impl AttackTable {
    /// Rank 1 (`a1..h1`) — used to clip southbound shifts and pawn pushes.
    const FIRST_RANK: BitBoard = 0x00000000000000ff;
    /// Rank 8 — used to clip northbound shifts and pawn pushes.
    const LAST_RANK: BitBoard = 0xff00000000000000;
    /// The A-file — used to clip westbound shifts so they don't wrap.
    const FIRST_FILE: BitBoard = 0x0101010101010101;
    /// The H-file — used to clip eastbound shifts so they don't wrap.
    const LAST_FILE: BitBoard = 0x8080808080808080;
    /// Files A+B — needed for knight jumps that move two files west.
    const FIRST_TWO_FILES: BitBoard = 0x0303030303030303;
    /// Files G+H — needed for knight jumps that move two files east.
    const LAST_TWO_FILES: BitBoard = 0xc0c0c0c0c0c0c0c0;

    /// Builds all attack tables from scratch.
    ///
    /// Non-sliding pieces are populated by running their attack function
    /// against every single-square bitboard. The sliding tables call
    /// [`find_magic`](Self::find_magic) per source square.
    ///
    /// This is expensive (milliseconds) — call once via [`ATTACK_TABLE`].
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

        // Black pawn attacks are derived from white pawn attacks by
        // vertically flipping both the source square and the resulting
        // board, which avoids a second symbolic computation.
        let mut black_pawn = [BitBoard::EMPTY; 64];
        for square in Square::all() {
            black_pawn[square.flip_vertical() as usize] = white_pawn[square as usize].flip_ranks();
        }

        let (line, between) = Self::compute_line_and_between();

        AttackTable {
            king,
            white_pawn,
            black_pawn,
            knight,
            bishop,
            rook,
            line,
            between,
        }
    }

    /// Returns the squares a pawn of `color` on `square` attacks.
    ///
    /// Only diagonal capture targets are returned — single and double
    /// pushes are handled separately in move generation.
    pub fn get_pawn(&self, square: Square, color: Color) -> BitBoard {
        match color {
            Color::White => self.white_pawn[square as usize],
            Color::Black => self.black_pawn[square as usize],
        }
    }

    /// Returns the squares a king on `square` attacks (the 8-neighborhood
    /// clipped to the board).
    pub fn get_king(&self, square: Square) -> BitBoard {
        self.king[square as usize]
    }

    /// Returns the squares a knight on `square` attacks.
    pub fn get_knight(&self, square: Square) -> BitBoard {
        self.knight[square as usize]
    }

    /// Returns the squares a bishop on `square` attacks given `blockers`.
    ///
    /// `blockers` should be the full occupancy bitboard (both colors);
    /// the magic table masks it down to the relevant squares internally.
    /// Friendly-piece filtering happens at the move-generation layer.
    pub fn get_bishop(&self, square: Square, blockers: BitBoard) -> BitBoard {
        let table = &self.bishop[square as usize];
        table.boards[Self::index_table(&table.entry, blockers)]
    }

    /// Returns the squares a rook on `square` attacks given `blockers`.
    ///
    /// See [`get_bishop`](Self::get_bishop) for the `blockers` contract.
    pub fn get_rook(&self, square: Square, blockers: BitBoard) -> BitBoard {
        let table = &self.rook[square as usize];
        table.boards[Self::index_table(&table.entry, blockers)]
    }

    /// Returns the squares a queen on `square` attacks given `blockers`.
    ///
    /// Computed as the union of the rook and bishop lookups — there is
    /// no separate queen magic table.
    pub fn get_queen(&self, square: Square, blockers: BitBoard) -> BitBoard {
        let rook_table = &self.rook[square as usize];
        let bishop_table = &self.bishop[square as usize];
        let rook_moves = rook_table.boards[Self::index_table(&rook_table.entry, blockers)];
        let bishop_moves = bishop_table.boards[Self::index_table(&bishop_table.entry, blockers)];
        rook_moves | bishop_moves
    }

    /// Returns the full rank, file, or diagonal containing `a` and `b`
    /// (both endpoints included), or `0` when the two squares do not
    /// share a queen-style ray. Symmetric: `get_line(a, b) == get_line(b, a)`.
    pub fn get_line(&self, a: Square, b: Square) -> BitBoard {
        self.line[a as usize][b as usize]
    }

    /// Returns the squares strictly between `a` and `b` on their shared
    /// ray (both endpoints excluded), or `0` when they do not share one.
    /// Symmetric: `get_between(a, b) == get_between(b, a)`.
    pub fn get_between(&self, a: Square, b: Square) -> BitBoard {
        self.between[a as usize][b as usize]
    }

    /// Hashes `blockers` into a [`MagicTable::boards`] index.
    ///
    /// Steps: mask down to relevant blockers, multiply by the magic, and
    /// keep the top `64 - shift` bits as the index.
    fn index_table(entry: &MagicEntry, blockers: BitBoard) -> usize {
        let blockers = blockers & entry.mask;
        let hash = blockers.wrapping_mul(entry.magic);
        (hash >> entry.shift) as usize
    }

    /// Searches for a magic multiplier and builds the lookup table for
    /// one source square.
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

    /// Attempts to fill `table` using the candidate magic in `entry`.
    ///
    /// Walks every (blockers, moveset) pair and writes it at the hashed
    /// index. A slot from a previous generation is treated as empty;
    /// within the current generation, a slot already holding a
    /// *different* moveset means the magic collides and is rejected.
    /// Constructive collisions (same moveset) are fine and how the table
    /// stays dense.
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

    /// Fills squares reachable by shifting `board` north until it hits a
    /// blocker or the top edge. The blocker square itself is included
    /// (so captures are encoded), but squares beyond it are not.
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

    /// Southbound ray fill. See [`slide_north`](Self::slide_north).
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

    /// Eastbound ray fill. See [`slide_north`](Self::slide_north).
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

    /// Westbound ray fill. See [`slide_north`](Self::slide_north).
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

    /// Northeast diagonal ray fill. See [`slide_north`](Self::slide_north).
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

    /// Northwest diagonal ray fill. See [`slide_north`](Self::slide_north).
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

    /// Southeast diagonal ray fill. See [`slide_north`](Self::slide_north).
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

    /// Southwest diagonal ray fill. See [`slide_north`](Self::slide_north).
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

    /// All squares attacked by knights in `board`.
    ///
    /// Each of the eight L-jumps is implemented as a shift on `board`
    /// pre-masked to drop knights that would wrap around files (knights
    /// on the H-file can't jump further east, etc.).
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

    /// White pawn capture targets for every pawn in `board`. Pawns on
    /// the 8th rank or on the relevant edge file are masked out before
    /// shifting to avoid wrap-around.
    fn attack_pawn(board: BitBoard) -> BitBoard {
        (!(Self::LAST_FILE | Self::LAST_RANK) & board).shift_north_east()
            | (!(Self::FIRST_FILE | Self::LAST_RANK) & board).shift_north_west()
    }

    /// King attack squares (8-neighborhood) for every king in `board`,
    /// with edge files/ranks masked off so shifts don't wrap.
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

    /// Reference (non-magic) bishop attack generator. Used during magic
    /// search to compute the truth table; runtime queries use
    /// [`get_bishop`](Self::get_bishop) instead.
    fn attack_bishop(board: BitBoard, blockers: BitBoard) -> BitBoard {
        Self::slide_north_east(board, blockers)
            | Self::slide_north_west(board, blockers)
            | Self::slide_south_east(board, blockers)
            | Self::slide_south_west(board, blockers)
    }

    /// Reference (non-magic) rook attack generator. See
    /// [`attack_bishop`](Self::attack_bishop).
    fn attack_rook(board: BitBoard, blockers: BitBoard) -> BitBoard {
        Self::slide_north(board, blockers)
            | Self::slide_south(board, blockers)
            | Self::slide_east(board, blockers)
            | Self::slide_west(board, blockers)
    }

    /// Relevant-blocker mask for a rook on `board`.
    ///
    /// Each ray is extended through an empty board and then trimmed of
    /// its terminal edge square — a piece on the edge of a ray cannot
    /// block the attack any further, so it does not affect the hash and
    /// excluding it shrinks the table.
    fn mask_rook(board: BitBoard) -> BitBoard {
        (Self::slide_north(board, BitBoard::EMPTY) & !Self::LAST_RANK)
            | (Self::slide_south(board, BitBoard::EMPTY) & !Self::FIRST_RANK)
            | (Self::slide_east(board, BitBoard::EMPTY) & !Self::LAST_FILE)
            | (Self::slide_west(board, BitBoard::EMPTY) & !Self::FIRST_FILE)
    }

    /// Builds the full `line` and `between` lookup tables in one pass.
    ///
    /// For each ordered pair of squares we walk the connecting ray once
    /// (if there is one), accumulating the full line in both directions
    /// from `a` and the strictly-interior squares between `a` and `b`.
    /// Pairs that aren't on a rank, file, or diagonal stay at `0`.
    fn compute_line_and_between() -> (
        [[BitBoard; Square::NUM]; Square::NUM],
        [[BitBoard; Square::NUM]; Square::NUM],
    ) {
        let mut line = [[BitBoard::EMPTY; Square::NUM]; Square::NUM];
        let mut between = [[BitBoard::EMPTY; Square::NUM]; Square::NUM];

        for a in Square::all() {
            for b in Square::all() {
                if a == b {
                    continue;
                }

                let af = (a as i32) % File::NUM as i32;
                let ar = (a as i32) / File::NUM as i32;
                let bf = (b as i32) % File::NUM as i32;
                let br = (b as i32) / File::NUM as i32;

                let df = bf - af;
                let dr = br - ar;

                let on_ray = df == 0 || dr == 0 || df.abs() == dr.abs();
                if !on_ray {
                    continue;
                }

                let step_f = df.signum();
                let step_r = dr.signum();

                let on_board = |f: i32, r: i32| {
                    (0..File::NUM as i32).contains(&f) && (0..Rank::NUM as i32).contains(&r)
                };
                let bb_at =
                    |f: i32, r: i32| BitBoard::on(Square::index((r * File::NUM as i32 + f) as u8));

                let mut line_bb = BitBoard::EMPTY;

                let (mut f, mut r) = (af, ar);
                while on_board(f, r) {
                    line_bb |= bb_at(f, r);
                    f += step_f;
                    r += step_r;
                }
                let (mut f, mut r) = (af - step_f, ar - step_r);
                while on_board(f, r) {
                    line_bb |= bb_at(f, r);
                    f -= step_f;
                    r -= step_r;
                }

                let mut between_bb = BitBoard::EMPTY;
                let (mut f, mut r) = (af + step_f, ar + step_r);
                while f != bf || r != br {
                    between_bb |= bb_at(f, r);
                    f += step_f;
                    r += step_r;
                }

                line[a as usize][b as usize] = line_bb;
                between[a as usize][b as usize] = between_bb;
            }
        }

        (line, between)
    }

    /// Relevant-blocker mask for a bishop on `board`. See
    /// [`mask_rook`](Self::mask_rook) for the edge-trimming info.
    fn mask_bishop(board: BitBoard) -> BitBoard {
        Self::slide_north_east(board, BitBoard::EMPTY) & !(Self::LAST_RANK | Self::LAST_FILE)
            | Self::slide_north_west(board, BitBoard::EMPTY) & !(Self::LAST_RANK | Self::FIRST_FILE)
            | Self::slide_south_east(board, BitBoard::EMPTY) & !(Self::FIRST_RANK | Self::LAST_FILE)
            | Self::slide_south_west(board, BitBoard::EMPTY)
                & !(Self::FIRST_RANK | Self::FIRST_FILE)
    }
}

/// Process-wide singleton attack table.
pub static ATTACK_TABLE: Lazy<AttackTable> = Lazy::new(AttackTable::new);

#[cfg(test)]
mod tests {
    use super::AttackTable;
    use crate::bitboard::*;
    use crate::core::*;

    #[test]
    fn test_attack_sliders_degenerates() {
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
    fn test_attack_king() {
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
    fn test_attack_knight() {
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
    fn test_attack_pawn() {
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
    fn test_slide_north() {
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
    fn test_slide_south() {
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
    fn test_slide_east() {
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
    fn test_slide_west() {
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
    fn test_slide_north_east() {
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
    fn test_slide_north_west() {
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
    fn test_slide_south_east() {
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
    fn test_slide_south_west() {
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
    fn test_attack_bishop() {
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
    fn test_attack_rook() {
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

    #[test]
    fn test_get_line_same_square_is_empty() {
        let table = AttackTable::new();
        for square in Square::all() {
            assert_eq!(table.get_line(square, square), BitBoard::EMPTY);
            assert_eq!(table.get_between(square, square), BitBoard::EMPTY);
        }
    }

    #[test]
    fn test_get_line_off_ray_pairs_are_empty() {
        let table = AttackTable::new();
        // A knight-jump pair: not on any rank, file, or diagonal.
        assert_eq!(table.get_line(Square::E4, Square::F6), BitBoard::EMPTY);
        assert_eq!(table.get_between(Square::E4, Square::F6), BitBoard::EMPTY);

        // Same color but no shared ray.
        assert_eq!(table.get_line(Square::A1, Square::C2), BitBoard::EMPTY);
        assert_eq!(table.get_between(Square::A1, Square::C2), BitBoard::EMPTY);
    }

    #[test]
    fn test_get_line_rank() {
        let table = AttackTable::new();
        let rank_one = bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            X X X X X X X X
        };
        assert_eq!(table.get_line(Square::A1, Square::H1), rank_one);
        assert_eq!(table.get_line(Square::D1, Square::F1), rank_one);

        let between_d1_h1 = bitboard! {
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . X X X .
        };
        assert_eq!(table.get_between(Square::D1, Square::H1), between_d1_h1);
    }

    #[test]
    fn test_get_line_file() {
        let table = AttackTable::new();
        let a_file = bitboard! {
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
        };
        assert_eq!(table.get_line(Square::A1, Square::A8), a_file);
        assert_eq!(table.get_line(Square::A3, Square::A6), a_file);

        let between_a2_a7 = bitboard! {
            . . . . . . . .
            . . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            X . . . . . . .
            . . . . . . . .
            . . . . . . . .
        };
        assert_eq!(table.get_between(Square::A2, Square::A7), between_a2_a7);
    }

    #[test]
    fn test_get_line_main_diagonal() {
        let table = AttackTable::new();
        let main_diag = bitboard! {
            . . . . . . . X
            . . . . . . X .
            . . . . . X . .
            . . . . X . . .
            . . . X . . . .
            . . X . . . . .
            . X . . . . . .
            X . . . . . . .
        };
        assert_eq!(table.get_line(Square::A1, Square::H8), main_diag);
        assert_eq!(table.get_line(Square::C3, Square::F6), main_diag);

        let between_a1_h8 = bitboard! {
            . . . . . . . .
            . . . . . . X .
            . . . . . X . .
            . . . . X . . .
            . . . X . . . .
            . . X . . . . .
            . X . . . . . .
            . . . . . . . .
        };
        assert_eq!(table.get_between(Square::A1, Square::H8), between_a1_h8);
    }

    #[test]
    fn test_get_line_anti_diagonal() {
        let table = AttackTable::new();
        let anti_diag = bitboard! {
            X . . . . . . .
            . X . . . . . .
            . . X . . . . .
            . . . X . . . .
            . . . . X . . .
            . . . . . X . .
            . . . . . . X .
            . . . . . . . X
        };
        assert_eq!(table.get_line(Square::A8, Square::H1), anti_diag);
        assert_eq!(table.get_line(Square::D5, Square::F3), anti_diag);

        let between_a8_h1 = bitboard! {
            . . . . . . . .
            . X . . . . . .
            . . X . . . . .
            . . . X . . . .
            . . . . X . . .
            . . . . . X . .
            . . . . . . X .
            . . . . . . . .
        };
        assert_eq!(table.get_between(Square::A8, Square::H1), between_a8_h1);
    }

    #[test]
    fn test_get_between_adjacent_squares_is_empty() {
        let table = AttackTable::new();
        // Adjacent along a ray: line is the whole ray, between is empty.
        assert_eq!(table.get_between(Square::A1, Square::B1), BitBoard::EMPTY);
        assert_eq!(table.get_between(Square::E4, Square::E5), BitBoard::EMPTY);
        assert_eq!(table.get_between(Square::D4, Square::E5), BitBoard::EMPTY);

        // But the lines are still non-empty (the full rank/file/diagonal).
        assert_ne!(table.get_line(Square::A1, Square::B1), BitBoard::EMPTY);
        assert_ne!(table.get_line(Square::E4, Square::E5), BitBoard::EMPTY);
        assert_ne!(table.get_line(Square::D4, Square::E5), BitBoard::EMPTY);
    }

    #[test]
    fn test_get_line_is_symmetric() {
        let table = AttackTable::new();
        for a in Square::all() {
            for b in Square::all() {
                assert_eq!(
                    table.get_line(a, b),
                    table.get_line(b, a),
                    "line not symmetric for ({:?}, {:?})",
                    a,
                    b
                );
                assert_eq!(
                    table.get_between(a, b),
                    table.get_between(b, a),
                    "between not symmetric for ({:?}, {:?})",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn test_get_between_is_subset_of_line_minus_endpoints() {
        let table = AttackTable::new();
        for a in Square::all() {
            for b in Square::all() {
                if a == b {
                    continue;
                }
                let line = table.get_line(a, b);
                let between = table.get_between(a, b);
                // Between is a subset of the line.
                assert_eq!(between & !line, BitBoard::EMPTY);
                // Between never contains the endpoints.
                assert_eq!(between & BitBoard::on(a), BitBoard::EMPTY);
                assert_eq!(between & BitBoard::on(b), BitBoard::EMPTY);
                // If line is non-empty it includes both endpoints.
                if line != BitBoard::EMPTY {
                    assert_ne!(line & BitBoard::on(a), BitBoard::EMPTY);
                    assert_ne!(line & BitBoard::on(b), BitBoard::EMPTY);
                }
            }
        }
    }
}
