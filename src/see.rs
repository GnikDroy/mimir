//! Static Exchange Evaluation.
//!
//! Resolves a capture sequence on a single destination square without a
//! tree search: walk the [`attackers_to`] bitboard, alternate sides,
//! always recapture with the least valuable attacker, and refresh
//! X-ray sliders that were hidden behind the attacker that just moved.
//!
//! [`see_ge`] is the threshold form: returns `true` iff the swap-off's
//! material balance is at least `threshold` centipawns from the
//! side-to-move's perspective. Quiescence calls it with `threshold = 0`
//! to discard captures that obviously lose material before paying for
//! `make_move` and a recursive `quiescence` call.
//!
//! Pinned attackers are not filtered out — adding pin tracking would require
//! auxiliary state and the resulting score is still a useful pruning signal.
//!
//! Castling is short-circuited to `0 >= threshold` (never a capture).
//! Promotions add the queen-minus-pawn delta to the initial gain and
//! treat the moved piece as the promoted piece for the recapture
//! exchange. En passant adjusts the occupancy for the captured pawn,
//! which lives on a different square from the destination.

use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::core::*;
use crate::move_scorer::PIECE_VALUES;
use crate::state::GameState;
use crate::ATTACK_TABLE;

#[inline(always)]
fn value(piece: Piece) -> i32 {
    PIECE_VALUES[piece as usize]
}

/// Pieces of either color attacking `sq` under the occupancy `occ`. Pass
/// a modified `occ` (with already-spent attackers removed) to expose
/// X-ray sliders. Non-sliders ignore `occ` here; the [`see_ge`] main
/// loop filters them out via `attackers &= occ`.
fn attackers_to(state: &GameState, sq: Square, occ: BitBoard) -> BitBoard {
    let p = &state.pieces;
    let w = Color::White as usize;
    let b = Color::Black as usize;

    let rooks_queens = p[w][Piece::Rook as usize]
        | p[w][Piece::Queen as usize]
        | p[b][Piece::Rook as usize]
        | p[b][Piece::Queen as usize];
    let bishops_queens = p[w][Piece::Bishop as usize]
        | p[w][Piece::Queen as usize]
        | p[b][Piece::Bishop as usize]
        | p[b][Piece::Queen as usize];
    let knights = p[w][Piece::Knight as usize] | p[b][Piece::Knight as usize];
    let kings = p[w][Piece::King as usize] | p[b][Piece::King as usize];

    // A white pawn attacks `sq` iff a hypothetical black pawn on `sq`
    // would attack the white pawn's square — so we query the opposite
    // color's pawn-attack table from `sq`.
    (ATTACK_TABLE.get_rook(sq, occ) & rooks_queens)
        | (ATTACK_TABLE.get_bishop(sq, occ) & bishops_queens)
        | (ATTACK_TABLE.get_knight(sq) & knights)
        | (ATTACK_TABLE.get_king(sq) & kings)
        | (ATTACK_TABLE.get_pawn(sq, Color::Black) & p[w][Piece::Pawn as usize])
        | (ATTACK_TABLE.get_pawn(sq, Color::White) & p[b][Piece::Pawn as usize])
}

/// SEE lower-bound test: `true` iff the static-exchange material balance
/// of `mv` is at least `threshold` centipawns from `state.side_to_move`'s
/// perspective.
pub fn see_ge(state: &GameState, mv: Move, threshold: i32) -> bool {
    // Castling never captures and never crosses through a contested
    // square in a way SEE can quantify, so it's vacuously zero.
    if mv.is_castle() {
        return 0 >= threshold;
    }

    let from = mv.get_from();
    let to = mv.get_to();

    // Material gained on the first move: the captured piece (if any),
    // plus the promotion delta (pawn -> promoted piece) for promotions.
    let captured_value = mv.get_captured_piece().map_or(0, value);
    let moved_after = mv
        .get_promotion_piece()
        .map_or_else(|| mv.get_moved_piece(), |pp| pp.to_piece());
    let promo_gain = if mv.is_promotion() {
        value(moved_after) - value(Piece::Pawn)
    } else {
        0
    };

    // First gate: even without any recapture, do we clear threshold?
    let mut swap = captured_value + promo_gain - threshold;
    if swap < 0 {
        return false;
    }

    // Second gate: even if the opponent captures our moved piece (the
    // worst plausible reply), is our net still above threshold? If so,
    // no deeper exploration is needed.
    swap = value(moved_after) - swap;
    if swap <= 0 {
        return true;
    }

    // Post-move occupancy: clear `from` (mover left). Toggling `to` only
    // matters for "what does a slider sitting on `to` see" lookups; the
    // attack-table mask excludes the origin square, so the bit on `to`
    // is irrelevant either way. We toggle it to mirror Stockfish.
    let mut occ = state.occupancies[2] ^ BitBoard::on(from) ^ BitBoard::on(to);

    // En passant removes a pawn that is not on `to` — important so
    // X-ray sliders behind the captured pawn surface correctly.
    if mv.is_enpassant() {
        let ep_sq = match state.side_to_move {
            Color::White => Square::index(to as u8 - 8),
            Color::Black => Square::index(to as u8 + 8),
        };
        occ ^= BitBoard::on(ep_sq);
    }

    let mut attackers = attackers_to(state, to, occ);
    let mut stm = state.side_to_move.opposite();

    // Cached masks for the X-ray refresh.
    let p = &state.pieces;
    let bishops_queens = p[0][Piece::Bishop as usize]
        | p[0][Piece::Queen as usize]
        | p[1][Piece::Bishop as usize]
        | p[1][Piece::Queen as usize];
    let rooks_queens = p[0][Piece::Rook as usize]
        | p[0][Piece::Queen as usize]
        | p[1][Piece::Rook as usize]
        | p[1][Piece::Queen as usize];

    // `res = 1` ⇒ the side-to-move's claim that SEE ≥ threshold is
    // currently winning. The bit flips each iteration to reflect the
    // alternating capture roles.
    let mut res: i32 = 1;

    loop {
        // Drop attackers that have been spent already — including the
        // original mover, which is still in non-slider piece bitboards
        // but no longer at `from` in `occ`.
        attackers &= occ;
        let stm_attackers = attackers & state.occupancies[stm as usize];
        if stm_attackers == 0 {
            break;
        }

        // Least valuable attacker for `stm`.
        let stm_pieces = &state.pieces[stm as usize];
        let (piece, cand) = [
            Piece::Pawn,
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
            Piece::King,
        ]
        .into_iter()
        .find_map(|kind| {
            let bb = stm_attackers & stm_pieces[kind as usize];
            (bb != 0).then_some((kind, bb))
        })
        .unwrap();

        res ^= 1;

        // King may only finalize the exchange when no opponent attackers
        // remain — otherwise the king would step into check, which is
        // illegal. When that happens, the side forfeits this round, so
        // we revert the speculative `res` flip and stop the loop.
        if piece == Piece::King {
            if (attackers & state.occupancies[stm.opposite() as usize]) != 0 {
                res ^= 1;
            }
            break;
        }

        // Update the swap balance. If even this LVA is too weak to make
        // the move worthwhile for `stm`, they decline and the chain
        // ends with the previous side's outcome.
        swap = value(piece) - swap;
        if swap < res {
            break;
        }

        // Remove the LVA from occupancy and refresh X-rays from sliders
        // newly visible to `to` along the LVA's ray.
        let lsb = cand & cand.wrapping_neg();
        occ ^= lsb;
        match piece {
            Piece::Pawn | Piece::Bishop => {
                attackers |= ATTACK_TABLE.get_bishop(to, occ) & bishops_queens;
            }
            Piece::Rook => {
                attackers |= ATTACK_TABLE.get_rook(to, occ) & rooks_queens;
            }
            Piece::Queen => {
                attackers |= ATTACK_TABLE.get_bishop(to, occ) & bishops_queens;
                attackers |= ATTACK_TABLE.get_rook(to, occ) & rooks_queens;
            }
            // King is handled above and broken out of; knights have no
            // X-ray successors.
            Piece::Knight | Piece::King => {}
        }

        stm = stm.opposite();
    }

    res != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GameState;

    fn find_move(state: &GameState, uci: &str) -> Move {
        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);
        moves
            .iter()
            .copied()
            .find(|m| m.to_uci() == uci)
            .unwrap_or_else(|| panic!("move {uci} not generated"))
    }

    #[test]
    fn rook_takes_undefended_pawn() {
        // Rxe4 — pawn on e4 has no defenders. SEE = +100.
        let state = GameState::from_fen("k7/8/8/8/4p3/8/8/4R2K w - - 0 1").unwrap();
        let mv = find_move(&state, "e1e4");
        assert!(see_ge(&state, mv, 100));
        assert!(see_ge(&state, mv, 0));
        assert!(!see_ge(&state, mv, 101));
    }

    #[test]
    fn pawn_takes_undefended_queen() {
        // cxd4 — black queen has no defenders. SEE = +900.
        let state = GameState::from_fen("k7/8/8/8/3q4/2P5/8/7K w - - 0 1").unwrap();
        let mv = find_move(&state, "c3d4");
        assert!(see_ge(&state, mv, 900));
        assert!(!see_ge(&state, mv, 901));
    }

    #[test]
    fn queen_takes_pawn_defended_by_knight() {
        // Qxd5 — pawn on d5 defended by knight on f6. Knight recaptures
        // the queen and the chain ends. SEE = +100 - 900 = -800.
        let state = GameState::from_fen("k7/8/5n2/3p4/8/8/8/3Q3K w - - 0 1").unwrap();
        let mv = find_move(&state, "d1d5");
        assert!(!see_ge(&state, mv, 0));
        assert!(see_ge(&state, mv, -800));
        assert!(!see_ge(&state, mv, -799));
    }

    #[test]
    fn even_pawn_trade_short_circuits() {
        // exd5, defended by cxd5. Symmetric pawn exchange — SEE = 0
        // and the algorithm should short-circuit via the second gate.
        let state = GameState::from_fen("k7/8/2p5/3p4/4P3/8/8/7K w - - 0 1").unwrap();
        let mv = find_move(&state, "e4d5");
        assert!(see_ge(&state, mv, 0));
        assert!(!see_ge(&state, mv, 1));
    }

    #[test]
    fn xray_reveals_rook_behind_queen() {
        // Qxd5 — black queen on d8 recaptures, but our rook on d1 is
        // X-rayed behind the moving queen and reveals as a re-recapturer
        // once the queen leaves d4. SEE = +100 - 900 + 900 = +100.
        // (Black king on a8 keeps the position legal — h8 would be
        // attacked by our queen on d4 along the long diagonal.)
        let state = GameState::from_fen("k2q4/8/8/3p4/3Q4/8/8/3R3K w - - 0 1").unwrap();
        let mv = find_move(&state, "d4d5");
        assert!(see_ge(&state, mv, 100));
        assert!(see_ge(&state, mv, 0));
        assert!(!see_ge(&state, mv, 101));
    }

    #[test]
    fn king_recaptures_when_no_other_defender() {
        // Nxd5 — pawn on d5 only defended by the black king. King takes
        // legally because there are no white defenders on d5.
        // SEE = +100 - 320 = -220.
        let state = GameState::from_fen("8/8/3k4/3p4/8/2N5/8/4K3 w - - 0 1").unwrap();
        let mv = find_move(&state, "c3d5");
        assert!(!see_ge(&state, mv, 0));
        assert!(see_ge(&state, mv, -220));
        assert!(!see_ge(&state, mv, -219));
    }

    #[test]
    fn king_cannot_recapture_when_square_defended() {
        // Nxd5 — pawn on d5 attacked by black king, but our pawn on c4
        // also attacks d5. King recapture would be into check and is
        // refused, so the swap ends after our capture. SEE = +100.
        let state = GameState::from_fen("8/8/3k4/3p4/2P5/2N5/8/4K3 w - - 0 1").unwrap();
        let mv = find_move(&state, "c3d5");
        assert!(see_ge(&state, mv, 100));
        assert!(!see_ge(&state, mv, 101));
    }

    #[test]
    fn en_passant_with_no_recapture() {
        // exd6 e.p. — black pawn on d5 has no defender. SEE = +100.
        let state = GameState::from_fen("k7/8/8/3pP3/8/8/8/7K w - d6 0 1").unwrap();
        let mv = find_move(&state, "e5d6");
        assert!(see_ge(&state, mv, 100));
        assert!(!see_ge(&state, mv, 101));
    }

    #[test]
    fn promotion_without_capture() {
        // e8=Q — empty target, no defenders. SEE = +800 (queen − pawn).
        let state = GameState::from_fen("k7/4P3/8/8/8/8/8/7K w - - 0 1").unwrap();
        let mv = find_move(&state, "e7e8q");
        assert!(see_ge(&state, mv, 800));
        assert!(!see_ge(&state, mv, 801));
    }

    #[test]
    fn promotion_capture_no_recapture() {
        // exd8=Q — captures rook, promotes to queen; no defender.
        // SEE = +500 (rook) + 800 (promo) = +1300.
        let state = GameState::from_fen("3r3k/4P3/8/8/8/8/8/7K w - - 0 1").unwrap();
        let mv = find_move(&state, "e7d8q");
        assert!(see_ge(&state, mv, 1300));
        assert!(!see_ge(&state, mv, 1301));
    }

    #[test]
    fn castling_short_circuits_to_zero() {
        // O-O — castling is never a capture, so SEE is vacuously zero.
        let state =
            GameState::from_fen("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let mv = find_move(&state, "e1g1");
        assert!(see_ge(&state, mv, 0));
        assert!(!see_ge(&state, mv, 1));
    }

    #[test]
    fn chain_with_two_attackers_each_side() {
        // Two rooks vs two rooks battery on the e-file. White's e2 rook
        // takes e7; black's e8 rook recaptures; white's e1 rook is
        // X-rayed through the now-empty e2/e7 squares and re-recaptures.
        // SEE = +500 - 500 + 500 = +500.
        let state = GameState::from_fen("4r2k/4r3/8/8/8/8/4R3/4R2K w - - 0 1").unwrap();
        let mv = find_move(&state, "e2e7");
        assert!(see_ge(&state, mv, 500));
        assert!(!see_ge(&state, mv, 501));
    }
}
