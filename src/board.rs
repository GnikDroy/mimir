use crate::attack_table::ATTACK_TABLE;
use crate::bitboard::*;
use crate::core::*;

/// Undo information for move reversal
#[derive(Debug, Clone, Copy)]
pub struct UndoInfo {
    pub captured_piece: Option<Piece>,
    pub en_passant_before: Option<Square>,
    pub castling_rights_before: u8,
    pub halfmove_clock_before: u8,
}

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

impl GameState {
    pub fn get_attacked_squares(&self, attacker: Color) -> BitBoard {
        let mut attacked = BitBoard::EMPTY;

        let attacker_boards = &self.pieces[attacker as usize];
        let pawn_board = attacker_boards[Piece::Pawn as usize];
        // It is faster to compute for all pawns at once instead of using attack table.
        // This includes en passant squares, since they are attacked by pawns as well.
        let pawn_attacks = match attacker {
            Color::White => pawn_board.shift_north_east() | pawn_board.shift_north_west(),
            Color::Black => pawn_board.shift_south_east() | pawn_board.shift_south_west(),
        };
        attacked |= pawn_attacks;

        for piece in [
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
            Piece::King,
        ] {
            let board = attacker_boards[piece as usize];
            for from in board.iter() {
                let attacks = match piece {
                    Piece::Knight => ATTACK_TABLE.get_knight(from),
                    Piece::Bishop => ATTACK_TABLE.get_bishop(from, self.occupancies[2]),
                    Piece::Rook => ATTACK_TABLE.get_rook(from, self.occupancies[2]),
                    Piece::Queen => ATTACK_TABLE.get_queen(from, self.occupancies[2]),
                    Piece::King => ATTACK_TABLE.get_king(from),
                    _ => unreachable!(),
                };
                attacked |= attacks;
            }
        }

        attacked
    }

    pub fn is_in_check(&self, color: Color) -> bool {
        let king_board = self.pieces[color as usize][Piece::King as usize];
        let enemy_attacks = self.get_attacked_squares(color.opposite());
        (king_board & enemy_attacks) != BitBoard::EMPTY
    }

    /// Make a move on the board and return undo information (does not validate legality)
    pub fn make_move(&mut self, move_encoded: u16) -> UndoInfo {
        let from = move_encoded.get_from();
        let to = move_encoded.get_to();
        let move_type = move_encoded.get_type();

        let from_board = BitBoard::on(from);
        let to_board = BitBoard::on(to);

        let moving_color = self.side_to_move;
        let enemy_color = moving_color.opposite();

        // Save undo info before making changes
        let en_passant_before = self.en_passant;
        let castling_rights_before = self.castling_rights;
        let halfmove_clock_before = self.halfmove_clock;

        // Find the moving piece
        let moving_piece = Piece::all()
            .find(|&p| (self.pieces[moving_color as usize][p as usize] & from_board) != 0)
            .expect("No piece at from square");

        // Remove piece from source
        self.pieces[moving_color as usize][moving_piece as usize] &= !from_board;
        self.occupancies[moving_color as usize] &= !from_board;

        // Track captured piece
        let mut captured_piece = None;

        // Handle captures
        if let MoveType::Capture { .. } = move_type {
            for piece in Piece::all() {
                if (self.pieces[enemy_color as usize][piece as usize] & to_board) != 0 {
                    captured_piece = Some(piece);
                    self.pieces[enemy_color as usize][piece as usize] &= !to_board;
                    self.occupancies[enemy_color as usize] &= !to_board;
                    break;
                }
            }
        }

        // Handle en passant captures
        if let MoveType::Capture { enpassant: true } = move_type {
            let capture_square = match moving_color {
                Color::White => Square::index(to as usize - 8),
                Color::Black => Square::index(to as usize + 8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.pieces[enemy_color as usize][Piece::Pawn as usize] &= !capture_board;
            self.occupancies[enemy_color as usize] &= !capture_board;
            captured_piece = Some(Piece::Pawn);
        }

        // Place piece at destination
        match move_type {
            MoveType::Promotion { piece, .. } => {
                self.pieces[moving_color as usize][piece as usize] |= to_board;
                self.occupancies[moving_color as usize] |= to_board;
            }
            _ => {
                self.pieces[moving_color as usize][moving_piece as usize] |= to_board;
                self.occupancies[moving_color as usize] |= to_board;
            }
        }

        // Handle castling
        if let MoveType::Castle { kingside } = move_type {
            let (rook_from, rook_to) = match (moving_color, kingside) {
                (Color::White, true) => (Square::H1, Square::F1),
                (Color::White, false) => (Square::A1, Square::D1),
                (Color::Black, true) => (Square::H8, Square::F8),
                (Color::Black, false) => (Square::A8, Square::D8),
            };
            let rook_from_board = BitBoard::on(rook_from);
            let rook_to_board = BitBoard::on(rook_to);
            self.pieces[moving_color as usize][Piece::Rook as usize] &= !rook_from_board;
            self.pieces[moving_color as usize][Piece::Rook as usize] |= rook_to_board;
            self.occupancies[moving_color as usize] &= !rook_from_board;
            self.occupancies[moving_color as usize] |= rook_to_board;
        }

        self.occupancies[2] =
            self.occupancies[moving_color as usize] | self.occupancies[enemy_color as usize];

        // Update castling rights
        if moving_piece == Piece::King {
            let bits_to_clear = match moving_color {
                Color::White => 0b0011,
                Color::Black => 0b1100,
            };
            self.castling_rights &= !bits_to_clear;
        } else if moving_piece == Piece::Rook {
            match (moving_color, from) {
                (Color::White, Square::A1) => self.castling_rights &= !0b0010,
                (Color::White, Square::H1) => self.castling_rights &= !0b0001,
                (Color::Black, Square::A8) => self.castling_rights &= !0b1000,
                (Color::Black, Square::H8) => self.castling_rights &= !0b0100,
                _ => {}
            }
        }

        // Also lose castling rights if a rook is captured
        if let Some(Piece::Rook) = captured_piece {
            match (enemy_color, to) {
                (Color::White, Square::A1) => self.castling_rights &= !0b0010,
                (Color::White, Square::H1) => self.castling_rights &= !0b0001,
                (Color::Black, Square::A8) => self.castling_rights &= !0b1000,
                (Color::Black, Square::H8) => self.castling_rights &= !0b0100,
                _ => {}
            }
        }

        // Update en passant
        self.en_passant = if let MoveType::DoublePawnPush = move_type {
            let ep_square = match moving_color {
                Color::White => Square::index(to as usize - 8),
                Color::Black => Square::index(to as usize + 8),
            };
            Some(ep_square)
        } else {
            None
        };

        // Toggle side to move
        self.side_to_move = enemy_color;

        UndoInfo {
            captured_piece,
            en_passant_before,
            castling_rights_before,
            halfmove_clock_before,
        }
    }

    /// Undo a move using the provided undo information
    pub fn unmake_move(&mut self, move_encoded: u16, undo_info: UndoInfo) {
        let from = move_encoded.get_from();
        let to = move_encoded.get_to();
        let move_type = move_encoded.get_type();

        // Restore side to move first (so we identify the moving side correctly)
        self.side_to_move = self.side_to_move.opposite();

        let from_board = BitBoard::on(from);
        let to_board = BitBoard::on(to);

        let moving_color = self.side_to_move;
        let enemy_color = moving_color.opposite();

        // Remove piece from destination and restore pawn at source for promotions
        // Or for regular moves, just find and move the piece back
        match move_type {
            MoveType::Promotion { piece, .. } => {
                // Remove the promoted piece from destination
                self.pieces[moving_color as usize][piece as usize] &= !to_board;
                // Restore the pawn at source
                self.pieces[moving_color as usize][Piece::Pawn as usize] |= from_board;
                self.occupancies[moving_color as usize] &= !to_board;
                self.occupancies[moving_color as usize] |= from_board;
            }
            _ => {
                // Find the piece that moved and put it back
                let moving_piece = Piece::all()
                    .find(|&p| (self.pieces[moving_color as usize][p as usize] & to_board) != 0)
                    .expect("No piece at to square");
                self.pieces[moving_color as usize][moving_piece as usize] &= !to_board;
                self.pieces[moving_color as usize][moving_piece as usize] |= from_board;
                self.occupancies[moving_color as usize] &= !to_board;
                self.occupancies[moving_color as usize] |= from_board;
            }
        }

        // Handle en passant restore
        if let MoveType::Capture { enpassant: true } = move_type {
            let capture_square = match moving_color {
                Color::White => Square::index(to as usize - 8),
                Color::Black => Square::index(to as usize + 8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.pieces[enemy_color as usize][Piece::Pawn as usize] |= capture_board;
            self.occupancies[enemy_color as usize] |= capture_board;
        } else if let Some(captured) = undo_info.captured_piece {
            self.pieces[enemy_color as usize][captured as usize] |= to_board;
            self.occupancies[enemy_color as usize] |= to_board;
        }

        // Handle castling unmake
        if let MoveType::Castle { kingside } = move_type {
            let (rook_from, rook_to) = match (moving_color, kingside) {
                (Color::White, true) => (Square::H1, Square::F1),
                (Color::White, false) => (Square::A1, Square::D1),
                (Color::Black, true) => (Square::H8, Square::F8),
                (Color::Black, false) => (Square::A8, Square::D8),
            };
            let rook_from_board = BitBoard::on(rook_from);
            let rook_to_board = BitBoard::on(rook_to);
            self.pieces[moving_color as usize][Piece::Rook as usize] &= !rook_to_board;
            self.pieces[moving_color as usize][Piece::Rook as usize] |= rook_from_board;
            self.occupancies[moving_color as usize] &= !rook_to_board;
            self.occupancies[moving_color as usize] |= rook_from_board;
        }

        self.occupancies[2] =
            self.occupancies[moving_color as usize] | self.occupancies[enemy_color as usize];

        // Restore game state
        self.en_passant = undo_info.en_passant_before;
        self.castling_rights = undo_info.castling_rights_before;
        self.halfmove_clock = undo_info.halfmove_clock_before;
    }
}
