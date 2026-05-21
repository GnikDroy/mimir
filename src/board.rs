use crate::bitboard::*;
use crate::core::*;

/// Represents the complete game state
#[derive(Debug, Clone, Copy)]
pub struct GameState {
    pub pieces: [[BitBoard; Piece::NUM]; Color::NUM],
    pub occupancies: [BitBoard; Color::NUM + 1],

    pub side_to_move: Color,
    pub castling_rights: u8, // 4 bits: WK, WQ, BK, BQ
    pub en_passant: Option<Square>,
    pub halfmove_clock: u8,
    pub fullmove_number: u16,
}

impl GameState {
    pub fn new() -> Self {
        GameState {
            pieces: [[0; Piece::NUM]; Color::NUM],
            occupancies: [0; 3],
            side_to_move: Color::White,
            castling_rights: 0,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    /// Load standard starting position
    pub fn starting_position() -> Self {
        let mut state = GameState::new();

        let white_boards = &mut state.pieces[Color::White as usize];
        white_boards[Piece::King as usize] = BitBoard::on(Square::E1);
        white_boards[Piece::Queen as usize] = BitBoard::on(Square::D1);
        white_boards[Piece::Rook as usize] = BitBoard::on(Square::A1) | BitBoard::on(Square::H1);
        white_boards[Piece::Bishop as usize] = BitBoard::on(Square::C1) | BitBoard::on(Square::F1);
        white_boards[Piece::Knight as usize] = BitBoard::on(Square::B1) | BitBoard::on(Square::G1);
        white_boards[Piece::Pawn as usize] = bitboard!(
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            X X X X X X X X
            . . . . . . . .
        );

        let black_boards = &mut state.pieces[Color::Black as usize];
        black_boards[Piece::King as usize] = BitBoard::on(Square::E8);
        black_boards[Piece::Queen as usize] = BitBoard::on(Square::D8);
        black_boards[Piece::Rook as usize] = BitBoard::on(Square::A8) | BitBoard::on(Square::H8);
        black_boards[Piece::Bishop as usize] = BitBoard::on(Square::C8) | BitBoard::on(Square::F8);
        black_boards[Piece::Knight as usize] = BitBoard::on(Square::B8) | BitBoard::on(Square::G8);
        black_boards[Piece::Pawn as usize] = bitboard!(
            . . . . . . . .
            X X X X X X X X
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
            . . . . . . . .
        );

        let mut white_occupancy = 0u64;
        let mut black_occupancy = 0u64;

        for piece in Piece::all() {
            white_occupancy |= state.pieces[Color::White as usize][piece as usize];
            black_occupancy |= state.pieces[Color::Black as usize][piece as usize];
        }

        state.occupancies[Color::White as usize] = white_occupancy;
        state.occupancies[Color::Black as usize] = black_occupancy;
        state.occupancies[2] = white_occupancy | black_occupancy; // All pieces

        state.castling_rights = 0b1111; // All castling allowed
        state.side_to_move = Color::White;
        state.fullmove_number = 1;
        state
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}
