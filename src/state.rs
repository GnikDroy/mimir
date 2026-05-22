use std::ffi::os_str::Display;

use crate::attack_table::ATTACK_TABLE;
use crate::bitboard::*;
use crate::core::*;

#[derive(Debug, Clone, Copy)]
pub struct UndoInfo {
    pub captured_piece: Option<Piece>,
    pub moved_piece: Piece,
    pub en_passant_before: Option<Square>,
    pub castling_rights_before: u8,
    pub halfmove_clock_before: u8,
    pub fullmove_number_before: u16,
}

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
    pub fn empty() -> Self {
        GameState {
            pieces: [[0; Piece::NUM]; Color::NUM],
            occupancies: [0; Color::NUM + 1],
            side_to_move: Color::White,
            castling_rights: 0,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    pub fn new() -> Self {
        GameState::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap()
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap()
    }
}

impl GameState {
    #[inline(always)]
    pub fn is_square_attacked(&self, sq: Square, attacker: Color) -> bool {
        let occ = self.occupancies[2];
        let enemy = &self.pieces[attacker as usize];

        let sq_bb = BitBoard::on(sq);

        let rook_attackers = enemy[Piece::Rook as usize] | enemy[Piece::Queen as usize];
        if (ATTACK_TABLE.get_rook(sq, occ) & rook_attackers) != 0 {
            return true;
        }

        let bishop_attackers = enemy[Piece::Bishop as usize] | enemy[Piece::Queen as usize];
        if (ATTACK_TABLE.get_bishop(sq, occ) & bishop_attackers) != 0 {
            return true;
        }

        if (ATTACK_TABLE.get_knight(sq) & enemy[Piece::Knight as usize]) != 0 {
            return true;
        }

        let pawn_attackers = match attacker {
            Color::White => sq_bb.shift_south_east() | sq_bb.shift_south_west(),
            Color::Black => sq_bb.shift_north_east() | sq_bb.shift_north_west(),
        };

        if (pawn_attackers & enemy[Piece::Pawn as usize]) != 0 {
            return true;
        }
        if (ATTACK_TABLE.get_king(sq) & enemy[Piece::King as usize]) != 0 {
            return true;
        }

        false
    }

    #[inline(always)]
    pub fn is_in_check(&self, color: Color) -> bool {
        let enemy = color.opposite();
        let king_bb = self.pieces[color as usize][Piece::King as usize];
        self.is_square_attacked(Square::index(king_bb.trailing_zeros() as u8), enemy)
    }

    fn no_moves(&mut self) -> bool {
        let mut moves = Vec::with_capacity(256);
        self.generate_valid_moves(&mut moves);
        moves.is_empty()
    }

    #[inline(always)]
    pub fn is_checkmate(&mut self) -> bool {
        self.is_in_check(self.side_to_move) && self.no_moves()
    }

    #[inline(always)]
    pub fn is_stalemate(&mut self) -> bool {
        !self.is_in_check(self.side_to_move) && self.no_moves()
    }

    /// Make a move on the board and return undo information (does not validate legality)
    pub fn make_move(&mut self, move_encoded: Move) -> UndoInfo {
        let from = move_encoded.get_from();
        let to = move_encoded.get_to();

        let from_board = BitBoard::on(from);
        let to_board = BitBoard::on(to);

        let moving_color = self.side_to_move;
        let enemy_color = moving_color.opposite();

        // Save undo info before making changes
        let en_passant_before = self.en_passant;
        let castling_rights_before = self.castling_rights;
        let halfmove_clock_before = self.halfmove_clock;
        let fullmove_number_before = self.fullmove_number;

        // Find the moving piece
        let moving_piece = move_encoded.get_moved_piece();

        // Remove piece from source
        self.pieces[moving_color as usize][moving_piece as usize] &= !from_board;
        self.occupancies[moving_color as usize] &= !from_board;

        // Place piece at destination
        if move_encoded.is_promotion() {
            let piece = move_encoded.get_promotion_piece().unwrap().to_piece();
            self.pieces[moving_color as usize][piece as usize] |= to_board;
            self.occupancies[moving_color as usize] |= to_board;
        } else {
            self.pieces[moving_color as usize][moving_piece as usize] |= to_board;
            self.occupancies[moving_color as usize] |= to_board;
        }

        let captured_piece = move_encoded.get_captured_piece();

        // Handle captures
        if let Some(_) = captured_piece {
            self.pieces[enemy_color as usize][captured_piece.unwrap() as usize] &= !to_board;
            self.occupancies[enemy_color as usize] &= !to_board;
        }

        // Handle en passant captures
        if move_encoded.is_enpassant() {
            let capture_square = match moving_color {
                Color::White => Square::index(to as u8 - 8),
                Color::Black => Square::index(to as u8 + 8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.pieces[enemy_color as usize][Piece::Pawn as usize] &= !capture_board;
            self.occupancies[enemy_color as usize] &= !capture_board;
        }

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
        self.en_passant = if move_encoded.is_double_pawn_push() {
            let ep_square = match moving_color {
                Color::White => Square::index(to as u8 - 8),
                Color::Black => Square::index(to as u8 + 8),
            };
            Some(ep_square)
        } else {
            None
        };

        // Handle castling
        if move_encoded.is_castle() {
            let kingside = move_encoded.is_kingside_castle();
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

        // Toggle side to move
        self.side_to_move = enemy_color;

        if self.side_to_move == Color::White {
            self.fullmove_number += 1;
        }

        self.halfmove_clock += if moving_piece == Piece::Pawn || move_encoded.is_capture() {
            0
        } else {
            1
        };

        UndoInfo {
            captured_piece,
            moved_piece: moving_piece,
            en_passant_before,
            castling_rights_before,
            halfmove_clock_before,
            fullmove_number_before,
        }
    }

    pub fn unmake_move(&mut self, move_encoded: Move, undo_info: &UndoInfo) {
        let from = move_encoded.get_from();
        let to = move_encoded.get_to();

        // Restore side to move first (so we identify the moving side correctly)
        self.side_to_move = self.side_to_move.opposite();

        let from_board = BitBoard::on(from);
        let to_board = BitBoard::on(to);

        let moving_color = self.side_to_move;
        let enemy_color = moving_color.opposite();

        // Remove piece from destination and restore pawn at source for promotions
        // Or for regular moves, just find and move the piece back
        if move_encoded.is_promotion() {
            let piece = move_encoded.get_promotion_piece().unwrap().to_piece();
            self.pieces[moving_color as usize][piece as usize] &= !to_board;
            self.pieces[moving_color as usize][Piece::Pawn as usize] |= from_board;
            self.occupancies[moving_color as usize] &= !to_board;
            self.occupancies[moving_color as usize] |= from_board;
        } else {
            let moving_piece = undo_info.moved_piece;
            self.pieces[moving_color as usize][moving_piece as usize] &= !to_board;
            self.pieces[moving_color as usize][moving_piece as usize] |= from_board;
            self.occupancies[moving_color as usize] &= !to_board;
            self.occupancies[moving_color as usize] |= from_board;
        }

        // Handle en passant & capture piece restore
        if move_encoded.is_enpassant() {
            let capture_square = match moving_color {
                Color::White => Square::index(to as u8 - File::NUM as u8),
                Color::Black => Square::index(to as u8 + File::NUM as u8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.pieces[enemy_color as usize][Piece::Pawn as usize] |= capture_board;
            self.occupancies[enemy_color as usize] |= capture_board;
        } else if move_encoded.is_capture() {
            let captured = undo_info.captured_piece.unwrap();
            self.pieces[enemy_color as usize][captured as usize] |= to_board;
            self.occupancies[enemy_color as usize] |= to_board;
        }

        // Handle castling unmake
        if move_encoded.is_castle() {
            let (rook_from, rook_to) = match (moving_color, move_encoded.is_kingside_castle()) {
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
        self.fullmove_number = undo_info.fullmove_number_before;
    }
}
