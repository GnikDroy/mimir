use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::core::*;
use crate::state::*;

impl GameState {
    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let mut gs = GameState {
            pieces: [[BitBoard::EMPTY; Piece::NUM]; Color::NUM],
            occupancies: [BitBoard::EMPTY; Color::NUM + 1],
            side_to_move: Color::White,
            castling_rights: 0,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        };

        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() != 6 {
            return Err("Invalid FEN: must have 6 fields".to_string());
        }

        let board_str = parts[0];
        let mut rank = 7;
        let mut file = 0;

        for ch in board_str.chars() {
            match ch {
                '/' => {
                    if file != 8 {
                        return Err("Invalid FEN: incorrect number of files".to_string());
                    }
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => {
                    file += ch.to_digit(10).unwrap() as usize;
                }
                _ => {
                    let square_index = rank * 8 + file;
                    let square = Square::index(square_index as u8);
                    let (color, piece) = match ch {
                        'P' => (Color::White, Piece::Pawn),
                        'N' => (Color::White, Piece::Knight),
                        'B' => (Color::White, Piece::Bishop),
                        'R' => (Color::White, Piece::Rook),
                        'Q' => (Color::White, Piece::Queen),
                        'K' => (Color::White, Piece::King),
                        'p' => (Color::Black, Piece::Pawn),
                        'n' => (Color::Black, Piece::Knight),
                        'b' => (Color::Black, Piece::Bishop),
                        'r' => (Color::Black, Piece::Rook),
                        'q' => (Color::Black, Piece::Queen),
                        'k' => (Color::Black, Piece::King),
                        _ => return Err(format!("Invalid FEN character: {}", ch)),
                    };

                    let bb = BitBoard::on(square);
                    gs.pieces[color as usize][piece as usize] |= bb;
                    gs.occupancies[color as usize] |= bb;

                    file += 1;
                }
            }
        }

        gs.occupancies[2] = gs.occupancies[0] | gs.occupancies[1];

        gs.side_to_move = match parts[1] {
            "w" => Color::White,
            "b" => Color::Black,
            _ => return Err("Invalid FEN: side to move".to_string()),
        };

        gs.castling_rights = 0;
        for ch in parts[2].chars() {
            match ch {
                'K' => gs.castling_rights |= 0b0001,
                'Q' => gs.castling_rights |= 0b0010,
                'k' => gs.castling_rights |= 0b0100,
                'q' => gs.castling_rights |= 0b1000,
                '-' => {}
                _ => return Err(format!("Invalid FEN castling char: {}", ch)),
            }
        }

        gs.en_passant = if parts[3] != "-" {
            Some(Square::from_algebraic(parts[3]).ok_or("Invalid FEN en passant square")?)
        } else {
            None
        };

        gs.halfmove_clock = parts[4]
            .parse::<u8>()
            .map_err(|_| "Invalid FEN halfmove clock")?;

        gs.fullmove_number = parts[5]
            .parse::<u16>()
            .map_err(|_| "Invalid FEN fullmove number")?;

        Ok(gs)
    }
}
