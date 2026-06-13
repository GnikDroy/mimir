//! Score-encoding conventions shared by search, TT, and evaluation.
//!
//! Mate scores are encoded as `MATE_SCORE - distance_to_mate`. Inside the
//! search the distance is measured from the root; in the TT it must be
//! relative to the storing node, since the same position can be re-entered
//! at different plies. [`encode_tt_score`] / [`decode_tt_score`] are the
//! round-trip across that boundary.

use crate::search::MAX_PLY;

/// Score for a forced mate. Large enough to outweigh any material score,
/// kept below `i32::MAX` so distance-to-mate offsets don't overflow.
pub const MATE_SCORE: i32 = i32::MAX / 2;

/// Convert a search score (mate distance from the root) into the
/// ply-relative form stored in the TT. Non-mate scores pass through.
#[inline(always)]
pub fn encode_tt_score(score: i32, ply: usize) -> i32 {
    let ply = ply as i32;
    if score > MATE_SCORE - MAX_PLY as i32 {
        score + ply
    } else if score < -MATE_SCORE + MAX_PLY as i32 {
        score - ply
    } else {
        score
    }
}

/// Inverse of [`encode_tt_score`].
#[inline(always)]
pub fn decode_tt_score(score: i32, ply: usize) -> i32 {
    let ply = ply as i32;
    if score > MATE_SCORE - MAX_PLY as i32 {
        score - ply
    } else if score < -MATE_SCORE + MAX_PLY as i32 {
        score + ply
    } else {
        score
    }
}

/// Signed ply distance to mate (positive: side-to-move mates; negative:
/// side-to-move gets mated). `None` for non-mate scores.
pub fn mate_in_plies(score: i32) -> Option<i32> {
    let ply = MATE_SCORE - score.abs();
    (ply <= MAX_PLY as i32).then(|| ply * score.signum())
}
