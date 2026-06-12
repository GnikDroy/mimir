//! Standard Algebraic Notation.
//!
//! [`SAN`] holds the per-move SAN encoding of a game: one string per
//! move played, including the trailing `+`/`#` check annotation.
//!
//! Conventions implemented here:
//! - Pawn moves use file-letter capture prefix (`exd5`) and `=Q`-style
//!   promotion suffix; non-pawn moves use the piece letter plus the
//!   minimum disambiguator (file, rank, or both) needed to identify
//!   the origin square against all same-type peers that could legally
//!   reach the destination.
//! - Castling is rendered as `O-O` / `O-O-O`.
//! - Only check (`+`) and checkmate (`#`) annotations are emitted;
//!   subjective marks (`!`, `?`, `!?`, …) are not.

use std::ops::Deref;

use crate::bitboard::BitBoardMethods;
use crate::core::*;
use crate::state::GameState;

/// SAN encoding of a sequence of moves played from a given position.
///
/// Each entry is one move's SAN string (with check annotation).
/// Derefs to `[String]`, so all slice methods (`iter`, `len`,
/// `is_empty`, indexing, …) are available directly.
pub struct SAN {
    moves: Vec<String>,
}

impl SAN {
    /// SAN piece letter, or [`None`] for pawns (pawn moves are notated by
    /// file rather than by a piece letter).
    fn piece_to_char(piece: Piece) -> Option<char> {
        match piece {
            Piece::Pawn => None,
            Piece::Knight => Some('N'),
            Piece::Bishop => Some('B'),
            Piece::Rook => Some('R'),
            Piece::Queen => Some('Q'),
            Piece::King => Some('K'),
        }
    }

    /// File index → SAN file letter (`a`–`h`).
    fn file_to_char(file: File) -> char {
        (b'a' + file as u8) as char
    }

    /// Rank index → SAN rank digit (`1`–`8`).
    fn rank_to_char(rank: Rank) -> char {
        (b'1' + rank as u8) as char
    }

    /// SAN prefix for a non-pawn move: the piece letter plus the minimum
    /// disambiguator (file, rank, or both) needed to identify the origin.
    fn append_piece_prefix(mv: Move, state: &GameState, str: &mut String) {
        let piece = mv.get_moved_piece();
        let piece_char = Self::piece_to_char(piece).expect("piece_prefix called on a pawn move");

        let mut legal = MoveList::default();
        state.generate_moves(&mut legal);

        // Same-piece-type peers that could legally reach mv.get_to().
        let peers: Vec<Square> = state.pieces[state.side_to_move as usize][piece as usize]
            .iter()
            .filter(|&sq| sq != mv.get_from())
            .filter(|&from_sq| {
                let mut candidate = mv;
                candidate.set_from(from_sq);
                legal.contains(&candidate)
            })
            .collect();

        if peers.is_empty() {
            str.push(piece_char);
            return;
        }

        let (from_file, from_rank) = mv.get_from().coordinate();
        let file_unique = peers.iter().all(|sq| sq.coordinate().0 != from_file);
        let rank_unique = peers.iter().all(|sq| sq.coordinate().1 != from_rank);

        str.push(piece_char);
        if file_unique {
            str.push(Self::file_to_char(from_file));
        } else if rank_unique {
            str.push(Self::rank_to_char(from_rank));
        } else {
            str.push(Self::file_to_char(from_file));
            str.push(Self::rank_to_char(from_rank));
        }
    }

    /// Encodes a single move from `state` (without the trailing `+`/`#`
    /// annotation, which is added by [`SAN::from_moves`] after the move
    /// is applied).
    ///
    /// `mv` must be legal in `state`. `state` is used only as scratch
    /// space for disambiguation and is not mutated overall.
    fn encode_move(mv: Move, state: &mut GameState) -> String {
        let mut san = String::new();
        // pawns are handled differently than other pieces
        if mv.get_moved_piece() == Piece::Pawn {
            if mv.is_capture() {
                san.push(Self::file_to_char(mv.get_from().coordinate().0));
                san.push('x');
            }

            let (file, rank) = mv.get_to().coordinate();
            san.push(Self::file_to_char(file));
            san.push(Self::rank_to_char(rank));

            if mv.is_promotion() {
                let promo = mv.get_promotion_piece().unwrap().to_piece();
                san.push('=');
                san.push(Self::piece_to_char(promo).unwrap());
            }
        } else {
            match mv.get_type() {
                MoveType::KingSideCastling => return "O-O".into(),
                MoveType::QueenSideCastling => return "O-O-O".into(),
                MoveType::Normal | MoveType::Capture => {}
                _ => panic!("Unexpected move type for non-pawn move"),
            }

            Self::append_piece_prefix(mv, state, &mut san);
            if mv.is_capture() {
                san.push('x');
            }

            let (file, rank) = mv.get_to().coordinate();
            san.push(Self::file_to_char(file));
            san.push(Self::rank_to_char(rank));
        }
        san
    }

    /// Encodes `moves` played starting from `state`.
    /// Every move in `moves` must be legal at its turn.
    pub fn from_moves(moves: &[Move], state: &GameState) -> Self {
        let mut state = *state;
        let mut entries = Vec::with_capacity(moves.len());
        for &mv in moves {
            let mut san = Self::encode_move(mv, &mut state);
            state.make_move(mv);

            // annotations
            if state.is_checkmate() {
                san.push('#');
            } else if state.is_in_check(state.side_to_move) {
                san.push('+');
            }

            entries.push(san);
        }
        Self { moves: entries }
    }
}

impl Deref for SAN {
    type Target = [String];

    fn deref(&self) -> &Self::Target {
        &self.moves
    }
}
