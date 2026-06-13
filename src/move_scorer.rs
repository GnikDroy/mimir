//! Move-ordering policy for alpha-beta and quiescence search.
//!
//! [`MoveScorer`] owns the persistent ordering state (killer moves, history
//! heuristic) and produces per-move scores.

use crate::core::*;
use crate::search::MAX_PLY;
use crate::state::GameState;

/// Number of killer moves per ply.
const KILLER_MOVES_PER_PLY: usize = 2;

/// History cap; keeps `history_bonus` just under `KILLER_BONUS`.
const HISTORY_MAX: u32 = 8 * 8_000;

/// Depth squared multipler constant
const HISTORY_BONUS_MULTIPLIER: u32 = 8;

/// TT move; dominates all other ordering signals.
const TT_BONUS: i32 = 1_000_000;

/// Queen promotions, ranked above captures.
const QUEEN_PROMO_BONUS: i32 = 100_000;

/// Killer quiet moves at the current ply.
const KILLER_BONUS: i32 = 9_000;

/// Material values aligned to `core::Piece` ordering
const PIECE_VALUES: [i32; Piece::NUM] = [20_000, 900, 500, 330, 320, 100];

pub struct MoveScorer {
    killer_moves: Box<[[Move; KILLER_MOVES_PER_PLY]; MAX_PLY]>,
    history: Box<[[[Move; Square::NUM]; Square::NUM]; Color::NUM]>,
}

impl MoveScorer {
    pub fn new() -> Self {
        let killer_moves = Box::new([[0u32; KILLER_MOVES_PER_PLY]; MAX_PLY]);
        let history = Box::new([[[0u32; Square::NUM]; Square::NUM]; Color::NUM]);
        Self {
            killer_moves,
            history,
        }
    }

    /// Main-search ordering: TT move first, then queen-promotions,
    /// then captures (MVV-LVA), then quiet moves by history + killer.
    pub fn score_main(
        &self,
        mv: Move,
        state: &GameState,
        tt_move: Option<Move>,
        ply: usize,
    ) -> i32 {
        let tt_bonus = (Some(mv) == tt_move) as i32 * TT_BONUS;
        let killer_bonus = self.killer_moves[ply].contains(&mv) as i32 * KILLER_BONUS;

        let from = mv.get_from() as usize;
        let to = mv.get_to() as usize;
        let history_bonus =
            (self.history[state.side_to_move as usize][from][to] / HISTORY_BONUS_MULTIPLIER) as i32;

        tt_bonus
            + match mv.get_type() {
                t if t.is_promotion() && t.get_promotion_piece() == Some(PromotionPiece::Queen) => {
                    QUEEN_PROMO_BONUS
                }
                MoveType::Capture | MoveType::EnPassant => {
                    let captured = mv.get_captured_piece().unwrap() as usize;
                    let moved = mv.get_moved_piece() as usize;
                    // MVV-LVA style: prefer capturing high-value pieces with low-value attackers
                    (PIECE_VALUES[captured] * 100) - PIECE_VALUES[moved]
                }
                _ => history_bonus + killer_bonus,
            }
    }

    /// Quiescence ordering: queen-promotions, then captures (MVV-LVA).
    /// Quiet moves all tie at zero (only check evasions, so very rare)
    pub fn score_quiescence(mv: Move) -> i32 {
        match mv.get_type() {
            t if t.is_promotion() && t.get_promotion_piece() == Some(PromotionPiece::Queen) => {
                QUEEN_PROMO_BONUS
            }
            MoveType::Capture | MoveType::EnPassant => {
                let captured = mv.get_captured_piece().unwrap() as usize;
                let moved = mv.get_moved_piece() as usize;
                // MVV-LVA style: prefer capturing high-value pieces with low-value attackers
                (PIECE_VALUES[captured] * 100) - PIECE_VALUES[moved]
            }
            _ => 0,
        }
    }

    /// Reset killers at the start of a new search; history persists.
    #[inline(always)]
    pub fn clear_killers(&mut self) {
        self.killer_moves.fill([0u32; KILLER_MOVES_PER_PLY]);
    }

    /// Insert `mv` as the primary killer at `ply`, shifting older entries
    /// down. If `mv` already occupies a slot, entries before it shift down
    /// to that slot (no duplicates); otherwise the oldest entry is evicted.
    #[inline]
    pub fn update_killer(&mut self, mv: Move, ply: usize) {
        let killers = &mut self.killer_moves[ply];
        let stop = killers
            .iter()
            .position(|&k| k == mv)
            .unwrap_or(KILLER_MOVES_PER_PLY - 1);
        for i in (1..=stop).rev() {
            killers[i] = killers[i - 1];
        }
        killers[0] = mv;
    }

    /// Add a depth-scaled bonus to the history score for a quiet move
    /// that caused a beta cutoff.
    #[inline]
    pub fn update_history(&mut self, mv: Move, depth: u8, side: Color) {
        let from = mv.get_from() as usize;
        let to = mv.get_to() as usize;

        // Add a bonus based on depth (deeper moves that cause cutoffs are more valuable)
        let bonus = (depth as u32) * (depth as u32) * HISTORY_BONUS_MULTIPLIER;

        // Cap history values to prevent overflow and unbounded growth
        self.history[side as usize][from][to] = self.history[side as usize][from][to]
            .saturating_add(bonus)
            .min(HISTORY_MAX);
    }
}

impl Default for MoveScorer {
    fn default() -> Self {
        Self::new()
    }
}
