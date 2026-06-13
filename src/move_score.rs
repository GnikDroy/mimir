//! Per-node scored move buffer used by the search.
//!
//! [`MoveScore`] pairs a [`MoveList`] with one score per slot, kept aligned
//! by every mutation path. Populate via [`MoveScore::moves_mut`], score with
//! [`MoveScore::score_main`] / [`MoveScore::score_quiescence`], then iterate
//! descending with [`MoveScore::ordered`]. Scoring policy (killers, history,
//! MVV-LVA) lives on [`MoveScorer`].

use std::iter::FusedIterator;
use std::ops::Deref;

use crate::core::*;
use crate::move_scorer::MoveScorer;
use crate::stack_vec::StackVec;
use crate::state::GameState;

/// Score buffer for [`MoveList`]
type ScoreList = StackVec<i32, MAX_MOVE_COUNT>;

/// Generated moves paired with their ordering scores.
///
/// Derefs to [`MoveList`] for read-only slice access.
pub struct MoveScore {
    moves: MoveList,
    scores: ScoreList,
}

impl MoveScore {
    /// Empty buffer
    #[inline(always)]
    pub fn new() -> Self {
        unsafe {
            Self {
                moves: MoveList::new_uninit(),
                scores: ScoreList::new_uninit(),
            }
        }
    }

    /// Mutable handle to the move buffer for population and filtering,
    /// used before scoring. Scores are (re)built by `score_main` /
    /// `score_quiescence`; descending iteration goes through `ordered`.
    #[inline(always)]
    pub fn moves_mut(&mut self) -> &mut MoveList {
        &mut self.moves
    }

    /// Score every move in the buffer with the main-search ordering.
    pub fn score_main(
        &mut self,
        scorer: &MoveScorer,
        state: &GameState,
        tt_move: Option<Move>,
        ply: usize,
    ) {
        self.scores.clear();
        self.scores.extend(
            self.moves
                .iter()
                .map(|&mv| scorer.score_main(mv, state, tt_move, ply)),
        );
    }

    /// Score every move in the buffer with the quiescence ordering.
    pub fn score_quiescence(&mut self) {
        self.scores.clear();
        self.scores
            .extend(self.moves.iter().map(|&mv| MoveScorer::score_quiescence(mv)));
    }

    /// Iterate scored moves in descending order, one `O(n)` selection
    /// scan per yielded move.
    pub fn ordered(&mut self) -> OrderedMoves<'_> {
        OrderedMoves { buf: self, next: 0 }
    }
}

impl Default for MoveScore {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for MoveScore {
    type Target = MoveList;

    fn deref(&self) -> &MoveList {
        &self.moves
    }
}

/// Lazy iterator returned by [`MoveScore::ordered`].
pub struct OrderedMoves<'a> {
    buf: &'a mut MoveScore,
    next: usize,
}

impl<'a> OrderedMoves<'a> {
    /// Swap and return the best scoring move.
    #[inline]
    fn pop_best(&mut self) -> Move {
        let i = self.next;
        let scores = &self.buf.scores;
        // Strict `>` keeps the first occurrence on ties, so lowest index
        // wins without an explicit tiebreaker.
        let mut best_idx = i;
        let mut best_score = scores[i];
        for j in (i + 1)..scores.len() {
            let s = scores[j];
            if s > best_score {
                best_score = s;
                best_idx = j;
            }
        }
        if best_idx != i {
            self.buf.moves.swap(i, best_idx);
            self.buf.scores.swap(i, best_idx);
        }
        self.buf.moves[i]
    }
}

impl<'a> Iterator for OrderedMoves<'a> {
    type Item = Move;

    #[inline]
    fn next(&mut self) -> Option<Move> {
        if self.next >= self.buf.len() {
            return None;
        }
        let mv = self.pop_best();
        self.next += 1;
        Some(mv)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buf.len() - self.next;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for OrderedMoves<'_> {}

impl FusedIterator for OrderedMoves<'_> {}
