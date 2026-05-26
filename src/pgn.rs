use std::collections::HashMap;

use once_cell::sync::Lazy;

use crate::bitboard::BitBoardMethods;
use crate::core::*;
use crate::state::GameState;

fn pgn_header(result: &str, fen: Option<&str>) -> String {
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

#[inline(always)]
fn promotion_piece_to_byte(piece: PromotionPiece) -> u8 {
    match piece {
        PromotionPiece::Bishop => b'b',
        PromotionPiece::Knight => b'n',
        PromotionPiece::Rook => b'r',
        PromotionPiece::Queen => b'q',
    }
}

fn file_to_char(file: File) -> char {
    (b'a' + file as u8) as char
}

fn rank_to_char(rank: Rank) -> char {
    (b'1' + rank as u8) as char
}

fn disambiguate_from(mv: Move, state: &GameState) -> Option<String> {
    let piece = mv.get_moved_piece();

    let piece_char = match piece_to_char(piece) {
        Some(c) => c,
        None => return None, // Pawns don't need disambiguation
    };

    let mut legal_moves: Vec<Move> = Vec::with_capacity(256);
    state.clone().generate_valid_moves(&mut legal_moves);

    // Check if there's another piece of the same type that can move to the same square
    // If so we gather all the (Piece, Square) pairs.
    let disambiguation_squares = state.pieces[state.side_to_move as usize][piece as usize]
        .iter()
        .filter(|&sq| sq != mv.get_from())
        .filter_map(|from_sq| {
            let mut potential_move = mv.clone();
            potential_move.set_from(from_sq);

            legal_moves
                .contains(&potential_move)
                .then_some((piece, from_sq))
        })
        .collect::<Vec<_>>();

    if disambiguation_squares.is_empty() {
        return Some(format!("{}", piece_char));
    }

    let (file, rank) = mv.get_from().coordinate();
    let mut str = String::with_capacity(3);

    let can_distinguish_by_file = !disambiguation_squares
        .iter()
        .any(|&(_, sq)| sq.coordinate().0 == file);

    let can_distinguish_by_rank = !disambiguation_squares
        .iter()
        .any(|&(_, sq)| sq.coordinate().1 == mv.get_from().coordinate().1);

    if can_distinguish_by_file {
        str.push(piece_char);
        str.push(file_to_char(file));
    } else if can_distinguish_by_rank {
        str.push(piece_char);
        str.push(rank_to_char(rank));
    } else {
        str.push(piece_char);
        str.push(file_to_char(file));
        str.push(rank_to_char(rank));
    }
    Some(str)
}

fn pawn_move_pgn_string(mv: Move) -> String {
    let mut pgn_string = String::new();

    if mv.is_capture() {
        let from_file = file_to_char(mv.get_from().coordinate().0);
        pgn_string.push(from_file);
        pgn_string.push('x');
    }

    let (file, rank) = mv.get_to().coordinate();
    pgn_string.push(file_to_char(file));
    pgn_string.push(rank_to_char(rank));

    if mv.is_promotion() {
        let promotion_piece = mv.get_promotion_piece().unwrap().to_piece();
        let promotion_char = piece_to_char(promotion_piece).unwrap();
        pgn_string.push('=');
        pgn_string.push(promotion_char);
    }

    pgn_string
}

pub fn move_to_pgn(mv: Move, state: &GameState) -> String {
    if mv.get_moved_piece() == Piece::Pawn {
        return pawn_move_pgn_string(mv);
    }

    match mv.get_type() {
        MoveType::Castle { kingside: true } => "O-O".into(),
        MoveType::Castle { kingside: false } => "O-O-O".into(),
        MoveType::Capture { enpassant: false } | MoveType::Quiet => {
            let mut result = disambiguate_from(mv, state).unwrap();
            if mv.is_capture() {
                result.push('x');
            }
            let (file, rank) = mv.get_to().coordinate();
            result.push(file_to_char(file));
            result.push(rank_to_char(rank));
            result
        }
        _ => panic!("Unexpected move type for non-pawn move"),
    }
}

pub fn to_pgn(move_list: &Vec<Move>, fen: Option<&str>) -> String {
    let mut state = match fen {
        Some(fen) => GameState::from_fen(fen).unwrap(),
        None => GameState::new(),
    };
    let mut pgn_string = String::new();
    let initial_move_start_index = state.fullmove_number;
    let initial_side_to_move = state.side_to_move;
    for mv in move_list.iter() {
        if initial_side_to_move == Color::Black && state.fullmove_number == initial_move_start_index
        {
            pgn_string.push_str(&format!("{}... ", state.fullmove_number));
        } else if state.side_to_move == Color::White {
            pgn_string.push_str(&format!("{}. ", state.fullmove_number));
        }
        pgn_string.push_str(&move_to_pgn(*mv, &state));
        state.make_move(*mv);
        if state.is_checkmate() {
            pgn_string.push('#');
        } else if state.is_in_check(state.side_to_move) {
            pgn_string.push('+');
        }

        pgn_string.push(' ');
    }

    let result = if state.is_checkmate() {
        if state.side_to_move == Color::White {
            "0-1"
        } else {
            "1-0"
        }
    } else if state.is_stalemate() {
        "1/2-1/2"
    } else {
        "*"
    };
    pgn_string.push_str(result);
    pgn_string.insert_str(0, &pgn_header(result, fen));

    pgn_string
}

fn move_matches_uci(mv: &Move, uci: &str) -> bool {
    let move_uci = UCI_MOVE_CACHE.get(uci).unwrap();
    mv.get_from() == move_uci.get_from()
        && mv.get_to() == move_uci.get_to()
        && mv.get_promotion_piece() == move_uci.get_promotion_piece()
}

pub static UCI_MOVE_CACHE: Lazy<HashMap<String, Move>> = Lazy::new(|| {
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
                    promotion_piece_to_byte(promo) as char
                );
                cache.insert(uci_string, mv);
            }
        }
    }
    cache
});

pub fn uci_to_pgn(uci: &str, fen: Option<&str>) -> String {
    let mut state = match fen {
        Some(fen) => GameState::from_fen(fen).unwrap(),
        None => GameState::new(),
    };
    let mut move_list: Vec<Move> = Vec::new();
    let mut legal_moves: Vec<Move> = Vec::with_capacity(256);

    for mv_uci in uci.split(' ') {
        legal_moves.clear();
        state.generate_valid_moves(&mut legal_moves);
        move_list.push(
            *legal_moves
                .iter()
                .find(|&mv| move_matches_uci(mv, mv_uci))
                .expect("Invalid UCI move list"),
        );
        state.make_move(*move_list.last().unwrap());
    }
    to_pgn(&move_list, fen)
}

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
        let pgn = uci_to_pgn(uci, None);
        assert_eq!(pgn, expected_pgn);
    }

    #[test]
    #[should_panic]
    fn test_uci_to_pgn_invalid_move() {
        let uci = "e2e4 e7e5 e1e3";
        uci_to_pgn(uci, None);
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
            let expected_pgn = uci_to_pgn(&uci, None);
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
        let pgn = uci_to_pgn(uci, initial_fen.into());
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
