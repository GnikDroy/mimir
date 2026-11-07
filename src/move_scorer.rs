//! Move-ordering policy for alpha-beta and quiescence search.
//!
//! [`MoveScorer`] owns the persistent ordering state (killer moves, history
//! heuristic) and produces per-move scores.

use crate::core::*;
use crate::search::MAX_PLY;
use crate::state::GameState;

/// Number of killer moves per ply.
const KILLER_MOVES_PER_PLY: usize = 2;

/// History cap. Keeps `history_bonus` just under `KILLER_BONUS`.
const HISTORY_MAX: i32 = 8 * 8_000;

/// Depth squared multipler constant
const HISTORY_BONUS_MULTIPLIER: i32 = 8;

/// Depth-scaled magnitude of a history reward/penalty.
#[inline]
fn depth_bonus(depth: u8) -> i32 {
    (depth as i32) * (depth as i32) * HISTORY_BONUS_MULTIPLIER
}

/// TT move. Dominates all other ordering signals.
const TT_BONUS: i32 = 1_000_000;

/// Queen promotions, ranked above captures.
const QUEEN_PROMO_BONUS: i32 = 100_000;

/// Killer quiet moves at the current ply.
const KILLER_BONUS: i32 = 9_000;

/// Material values aligned to `core::Piece` ordering
pub const PIECE_VALUES: [i32; Piece::NUM] = [20_000, 900, 500, 330, 320, 100];

pub struct MoveScorer {
    killer_moves: Box<[[Move; KILLER_MOVES_PER_PLY]; MAX_PLY]>,
    history: Box<[[[i32; Square::NUM]; Square::NUM]; Color::NUM]>,
}

impl MoveScorer {
    pub fn new() -> Self {
        let killer_moves = Box::new([[0u32; KILLER_MOVES_PER_PLY]; MAX_PLY]);
        let history = Box::new([[[0i32; Square::NUM]; Square::NUM]; Color::NUM]);
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
            self.history[state.side_to_move as usize][from][to] / HISTORY_BONUS_MULTIPLIER;

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

    /// Reset killers at the start of a new search. History persists.
    #[inline(always)]
    pub fn clear_killers(&mut self) {
        self.killer_moves.fill([0u32; KILLER_MOVES_PER_PLY]);
    }

    /// Reset history heuristic.
    pub fn clear_history(&mut self) {
        for color in self.history.iter_mut() {
            for from in color.iter_mut() {
                from.fill(0);
            }
        }
    }

    /// `true` when `mv` is a registered killer move at `ply`.
    #[inline]
    pub fn is_killer(&self, mv: Move, ply: usize) -> bool {
        self.killer_moves[ply].contains(&mv)
    }

    /// Returns the quiet move (non-capture, non-promotion) with the highest
    /// raw history score for `side`, or `None` if no such move has nonzero
    /// history. Used to gauge the quality of the history heuristic, a node
    /// where every quiet move has zero history isn't a meaningful sample.
    pub fn find_history_top(&self, side: Color, moves: &MoveList) -> Option<Move> {
        let mut best: Option<Move> = None;
        let mut best_val: i32 = 0;
        for &mv in moves.iter() {
            if mv.is_capture() || mv.is_promotion() {
                continue;
            }
            let v = self.history[side as usize][mv.get_from() as usize][mv.get_to() as usize];
            if v > best_val {
                best_val = v;
                best = Some(mv);
            }
        }
        best
    }

    /// Insert `mv` as the primary killer at `ply`, shifting older entries
    /// down. If `mv` already occupies a slot, entries before it shift down
    /// to that slot (no duplicates). Otherwise the oldest entry is evicted.
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

    /// Reward a quiet move that caused a beta cutoff.
    #[inline]
    pub fn reward_history(&mut self, mv: Move, depth: u8, side: Color) {
        self.apply_bonus(mv, side, depth_bonus(depth));
    }

    /// Penalize a quiet move searched before the cutoff move.
    #[inline]
    pub fn penalize_history(&mut self, mv: Move, depth: u8, side: Color) {
        self.apply_bonus(mv, side, -depth_bonus(depth));
    }

    /// History bonus with gravity and malus.
    /// Entries decay toward the signal but within `[-HISTORY_MAX, HISTORY_MAX]`
    #[inline]
    fn apply_bonus(&mut self, mv: Move, side: Color, bonus: i32) {
        let from = mv.get_from() as usize;
        let to = mv.get_to() as usize;
        let entry = &mut self.history[side as usize][from][to];
        *entry += bonus - *entry * bonus.abs() / HISTORY_MAX;
    }
}

impl Default for MoveScorer {
    fn default() -> Self {
        Self::new()
    }
}
