use std::fmt::Display;
use std::str::FromStr;

use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::core::*;
use crate::state::*;
use crate::zobrist::ZOBRIST_HASHER;

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
            zobrist_hash: 0,
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

        gs.zobrist_hash = ZOBRIST_HASHER.hash(&gs);
        Ok(gs)
    }

    pub fn to_fen(&self) -> String {
        let mut fen = String::new();

        for rank in (0..8).rev() {
            let mut empty_count = 0;
            for file in 0..8 {
                let square_index = rank * 8 + file;
                let square = Square::index(square_index as u8);
                let mut piece_char = None;

                for color in Color::all() {
                    for piece in Piece::all() {
                        if self.pieces[color as usize][piece as usize] & BitBoard::on(square) != 0 {
                            piece_char = Some(match (color, piece) {
                                (Color::White, Piece::Pawn) => 'P',
                                (Color::White, Piece::Knight) => 'N',
                                (Color::White, Piece::Bishop) => 'B',
                                (Color::White, Piece::Rook) => 'R',
                                (Color::White, Piece::Queen) => 'Q',
                                (Color::White, Piece::King) => 'K',
                                (Color::Black, Piece::Pawn) => 'p',
                                (Color::Black, Piece::Knight) => 'n',
                                (Color::Black, Piece::Bishop) => 'b',
                                (Color::Black, Piece::Rook) => 'r',
                                (Color::Black, Piece::Queen) => 'q',
                                (Color::Black, Piece::King) => 'k',
                            });
                            break;
                        }
                    }
                    if piece_char.is_some() {
                        break;
                    }
                }

                if let Some(ch) = piece_char {
                    if empty_count > 0 {
                        fen.push_str(&empty_count.to_string());
                        empty_count = 0;
                    }
                    fen.push(ch);
                } else {
                    empty_count += 1;
                }
            }
            if empty_count > 0 {
                fen.push_str(&empty_count.to_string());
            }
            if rank > 0 {
                fen.push('/');
            }
        }

        fen.push(' ');
        fen.push(match self.side_to_move {
            Color::White => 'w',
            Color::Black => 'b',
        });
        fen.push(' ');

        if self.castling_rights == 0 {
            fen.push('-');
        } else {
            if self.castling_rights & 0b0001 != 0 {
                fen.push('K');
            }
            if self.castling_rights & 0b0010 != 0 {
                fen.push('Q');
            }
            if self.castling_rights & 0b0100 != 0 {
                fen.push('k');
            }
            if self.castling_rights & 0b1000 != 0 {
                fen.push('q');
            }
        }
        fen.push(' ');

        fen.push_str(
            &self
                .en_passant
                .map_or("-".to_string(), |sq| sq.to_algebraic()),
        );
        fen.push(' ');

        fen.push_str(&self.halfmove_clock.to_string());
        fen.push(' ');
        fen.push_str(&self.fullmove_number.to_string());

        fen
    }
}

impl Display for GameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_fen())
    }
}

impl FromStr for GameState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_fen(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fen_round_trip() {
        const FENS: [&'static str; 4] = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        ];

        for fen in FENS {
            let state = GameState::from_fen(fen).unwrap();
            assert_eq!(state.to_fen(), fen);
        }
    }

    #[test]
    fn test_fen_sets_up_position() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq e3 5 10";
        let state = GameState::from_fen(fen).unwrap();

        assert_eq!(
            state.pieces[Color::White as usize][Piece::King as usize],
            BitBoard::on(Square::E1)
        );
        assert_eq!(
            state.pieces[Color::Black as usize][Piece::Queen as usize],
            BitBoard::on(Square::D8)
        );
        assert_eq!(
            state.pieces[Color::White as usize][Piece::Knight as usize],
            BitBoard::on(Square::B1) | BitBoard::on(Square::G1)
        );
        assert_eq!(state.side_to_move, Color::White);
        assert_eq!(state.en_passant, Some(Square::E3));
        assert_eq!(state.castling_rights, 0b1111);
        assert_eq!(state.halfmove_clock, 5);
        assert_eq!(state.fullmove_number, 10);
    }
}
