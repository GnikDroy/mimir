//! PGN (Portable Game Notation) documents.
//!
//! Assembles a full PGN document — tag header, SAN move text, move
//! numbers, and the terminal result token — from either a `&[Move]`
//! ([`PGN::from_moves`]) or a UCI movestring ([`PGN::from_uci`]).
//! Per-move SAN encoding lives in [`crate::notation::san`].

use std::collections::HashMap;
use std::fmt::{self, Display, Write};

use once_cell::sync::Lazy;

use crate::core::*;
use crate::notation::san::SAN;
use crate::state::GameState;

/// A rendered PGN document.
///
/// Produced via [`PGN::from_moves`] or [`PGN::from_uci`]; consume via
/// [`Display`] (`println!("{}", pgn)`) or `String::from(pgn)`.
pub struct PGN {
    document: String,
}

impl PGN {
    /// Renders a PGN document for `moves` played from `fen` (or the
    /// standard start position when `fen` is `None`).
    pub fn from_moves(moves: &[Move], fen: Option<&str>) -> Self {
        let mut state = match fen {
            Some(fen) => GameState::from_fen(fen).unwrap(),
            None => GameState::new(),
        };
        let initial_fullmove = state.fullmove_number;
        let initial_side = state.side_to_move;

        let san = SAN::from_moves(moves, &state);

        let mut body = String::new();
        for (i, (&mv, san_move)) in moves.iter().zip(san.iter()).enumerate() {
            if i == 0 && initial_side == Color::Black {
                write!(body, "{}... ", initial_fullmove).unwrap();
            } else if state.side_to_move == Color::White {
                write!(body, "{}. ", state.fullmove_number).unwrap();
            }
            body.push_str(san_move);
            body.push(' ');
            state.make_move(mv);
        }

        let result = Self::result_token(&state);
        body.push_str(result);
        body.insert_str(0, &Self::header(result, fen));
        Self { document: body }
    }

    /// Renders a PGN document from a space-separated UCI movestring.
    /// Each token is matched against the current position's legal moves
    /// so the resulting SAN reflects the true game.
    pub fn from_uci(uci: &str, fen: Option<&str>) -> Self {
        let mut state = match fen {
            Some(fen) => GameState::from_fen(fen).unwrap(),
            None => GameState::new(),
        };
        let mut move_list: Vec<Move> = Vec::new();
        let mut legal_moves: MoveList = MoveList::default();

        for mv_uci in uci.split(' ') {
            legal_moves.clear();
            state.generate_moves(&mut legal_moves);
            let mv = *legal_moves
                .iter()
                .find(|&mv| Self::uci_matches(mv, mv_uci))
                .expect("Invalid UCI move list");
            state.make_move(mv);
            move_list.push(mv);
        }
        Self::from_moves(&move_list, fen)
    }

    fn header(result: &str, fen: Option<&str>) -> String {
        let fen_line = match fen {
            Some(fen) => format!("[FEN \"{}\"]\n[SetUp \"1\"]\n", fen),
            None => String::new(),
        };

        format!(
            "[Event \"?\"]\n\
             [Site \"?\"]\n\
             [Date \"????.??.??\"]\n\
             [Round \"?\"]\n\
             [White \"?\"]\n\
             [Black \"?\"]\n\
             [Result \"{}\"]\n{}\n",
            result, fen_line
        )
    }

    fn result_token(state: &GameState) -> &'static str {
        if state.is_checkmate() {
            if state.side_to_move == Color::White {
                "0-1"
            } else {
                "1-0"
            }
        } else if state.is_stalemate() {
            "1/2-1/2"
        } else {
            "*"
        }
    }

    fn uci_matches(mv: &Move, uci: &str) -> bool {
        let cached = UCI_MOVE_CACHE.get(uci).unwrap();
        mv.get_from() == cached.get_from()
            && mv.get_to() == cached.get_to()
            && mv.get_promotion_piece() == cached.get_promotion_piece()
    }
}

impl Display for PGN {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.document)
    }
}

impl From<PGN> for String {
    fn from(pgn: PGN) -> Self {
        pgn.document
    }
}

/// Pre-built table mapping every possible UCI move string to a `Move`
/// shell with `from`, `to`, and (optionally) promotion piece set.
///
/// **Kept as a static cache for performance.** [`PGN::from_uci`]
/// consults this table once per input token; a `HashMap` lookup is
/// materially faster than re-parsing the algebraic squares for each
/// token, especially across long game databases (see
/// `test_uci_to_pgn_all_carlsen_games`). The stored `Move` only carries
/// the fields [`PGN::uci_matches`] compares against — piece type and
/// move-type flags come from the live legal-move list at lookup time.
static UCI_MOVE_CACHE: Lazy<HashMap<String, Move>> = Lazy::new(|| {
    #[inline(always)]
    fn promotion_uci_char(piece: PromotionPiece) -> char {
        match piece {
            PromotionPiece::Bishop => 'b',
            PromotionPiece::Knight => 'n',
            PromotionPiece::Rook => 'r',
            PromotionPiece::Queen => 'q',
        }
    }

    let mut cache = HashMap::new();
    for from in Square::all() {
        for to in Square::all() {
            let mut mv = Move::from_quiet(from, to, Piece::Pawn);
            mv.set_from(from);
            mv.set_to(to);
            let uci_string = format!("{}{}", from.to_algebraic(), to.to_algebraic());
            cache.insert(uci_string, mv);
        }
    }

    for from in Square::all() {
        for to in Square::all() {
            for promo in [
                PromotionPiece::Bishop,
                PromotionPiece::Knight,
                PromotionPiece::Rook,
                PromotionPiece::Queen,
            ] {
                let mv = Move::from_promotion(from, to, Piece::Pawn, promo, None);
                let uci_string = format!(
                    "{}{}{}",
                    from.to_algebraic(),
                    to.to_algebraic(),
                    promotion_uci_char(promo)
                );
                cache.insert(uci_string, mv);
            }
        }
    }
    cache
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::BufRead;

    #[test]
    fn test_uci_to_pgn() {
        let uci = "\
        b1c3 g7g6 g1f3 f8g7 c3e4 b8c6 e2e3 c6e5 f1e2 e8f8 e1g1 e5f3 e2f3 d7d5 e4c5 d8d6 \
        c5b3 g8f6 f1e1 f8g8 e1e2 c8d7 e2e1 g6g5 h2h3 a8d8 b3d4 e7e5 d4b3 d7a4 g1h1 a4b3 \
        c2b3 e5e4 f3g4 d8e8 h1g1 d5d4 e3d4 d6d4 d1c2 d4b6 g4f5 b6a5 f5e4 a5b4 f2f3 b4d6 \
        g1h1 d6g3 e1e2 g3d6 c2c4 f6h5 e2e3 h5g3 h1g1 g3e4 e3e4 e8b8 c4a4 d6b6 g1h1 g7f6 \
        e4e8 g8g7 e8b8 h8b8 a4e4 b6c5 h1h2 c5d6 h2g1 f6d4 g1h1 d6f6 a1b1 g7g8 e4e2 f6f5 \
        d2d3 f5a5 b1a1 a5b5";
        let expected_pgn = "\
                [Event \"?\"]\n\
                [Site \"?\"]\n\
                [Date \"????.??.??\"]\n\
                [Round \"?\"]\n\
                [White \"?\"]\n\
                [Black \"?\"]\n\
                [Result \"*\"]\n\n\
                1. Nc3 g6 2. Nf3 Bg7 3. Ne4 Nc6 4. e3 Ne5 5. Be2 Kf8 6. O-O Nxf3+ 7. Bxf3 d5 \
                8. Nc5 Qd6 9. Nb3 Nf6 10. Re1 Kg8 11. Re2 Bd7 12. Re1 g5 13. h3 Rd8 14. Nd4 e5 \
                15. Nb3 Ba4 16. Kh1 Bxb3 17. cxb3 e4 18. Bg4 Re8 19. Kg1 d4 20. exd4 Qxd4 \
                21. Qc2 Qb6 22. Bf5 Qa5 23. Bxe4 Qb4 24. f3 Qd6 25. Kh1 Qg3 26. Re2 Qd6 27. Qc4 Nh5 \
                28. Re3 Ng3+ 29. Kg1 Nxe4 30. Rxe4 Rb8 31. Qa4 Qb6+ 32. Kh1 Bf6 33. Re8+ Kg7 \
                34. Rxb8 Rxb8 35. Qe4 Qc5 36. Kh2 Qd6+ 37. Kg1 Bd4+ 38. Kh1 Qf6 39. Rb1 Kg8 40. Qe2 Qf5 \
                41. d3 Qa5 42. Ra1 Qb5 *";
        let pgn = PGN::from_uci(uci, None).to_string();
        assert_eq!(pgn, expected_pgn);
    }

    #[test]
    #[should_panic]
    fn test_uci_to_pgn_invalid_move() {
        let uci = "e2e4 e7e5 e1e3";
        PGN::from_uci(uci, None);
    }

    #[test]
    fn test_uci_to_pgn_all_carlsen_games() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path_to_uci_moves = format!("{}/resources/test/pgn/carlsen_uci.txt", manifest_dir);
        let path_to_pgn = format!("{}/resources/test/pgn/carlsen.pgn", manifest_dir);

        let uci_data = std::io::BufReader::new(
            File::open(path_to_uci_moves).expect("Failed to open UCI moves file"),
        )
        .lines();

        let pgn_data = std::fs::read_to_string(path_to_pgn).expect("Failed to read PGN file");

        let parts: Vec<&str> = pgn_data.split("\n\n").collect();
        for (chunk, uci_moves) in parts.chunks(2).zip(uci_data) {
            let uci = uci_moves.expect("Failed to read UCI moves line");
            let expected_pgn = PGN::from_uci(&uci, None).to_string();
            let expected_pgn_moves = expected_pgn
                .split("\n\n")
                .nth(1)
                .expect("Failed to split PGN header from moves");
            let pgn_moves = chunk[1].trim();

            // PGN moves have have new lines every 80 characters, so we remove all newlines for the comparison
            let pgn_moves = pgn_moves.replace('\n', " ");
            let expected_pgn_moves = expected_pgn_moves.replace('\n', " ");

            // remove the results from the end of the move strings for comparison
            // most grandmaster games end with "1-0", "0-1", or "1/2-1/2"
            // but they rarely play out the final move that leads to checkmate or stalemate
            // we therefore cannot construct the results from the moves, so we just ignore them for the comparison
            let pgn_moves = pgn_moves
                .rsplitn(2, ' ')
                .nth(1)
                .unwrap_or(&pgn_moves)
                .trim();
            let expected_pgn_moves = expected_pgn_moves
                .rsplitn(2, ' ')
                .nth(1)
                .unwrap_or(&expected_pgn_moves)
                .trim();

            assert!(pgn_moves == expected_pgn_moves, "PGN moves do not match");
        }
    }

    #[test]
    fn test_uci_with_initial_fen() {
        let initial_fen = "7k/6p1/1ppr3p/5R2/1P1q4/r2P3P/2Q1RPP1/5K2 b - - 0 32";
        let uci = "a3a1 e2e1 a1e1 f1e1 d6e6 e1d1 d4b4 f5f8 b4f8 c2c4 e6f6";
        let pgn = PGN::from_uci(uci, initial_fen.into()).to_string();
        let expected_pgn = "\
            [Event \"?\"]\n\
            [Site \"?\"]\n\
            [Date \"????.??.??\"]\n\
            [Round \"?\"]\n\
            [White \"?\"]\n\
            [Black \"?\"]\n\
            [Result \"*\"]\n\
            [FEN \"7k/6p1/1ppr3p/5R2/1P1q4/r2P3P/2Q1RPP1/5K2 b - - 0 32\"]\n\
            [SetUp \"1\"]\n\n\
            32... Ra1+ 33. Re1 Rxe1+ 34. Kxe1 Re6+ 35. Kd1 Qxb4 36. Rf8+ Qxf8 37. Qc4 Rf6 *";
        assert_eq!(pgn, expected_pgn);
    }
}
