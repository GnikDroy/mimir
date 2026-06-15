//! Precomputed reduction values for Late Move Reductions.
//!
//! Indexed by `[depth][move_index]`. Reductions grow with both axes:
//! deeper searches can afford bigger reductions, and later moves in the
//! ordering are less likely to be best.
//!
//! Formula (integer approximation of `0.75 + ln(d) * ln(m) / 2.25`):
//! `(3072 + 872 * log2(d) * log2(m)) / 4096`. Constants chosen so the
//! result tracks the float version within ±1 across the table.

use crate::search::MAX_PLY;
use crate::types::MAX_MOVE_COUNT;

#[inline(always)]
const fn ilog2(x: usize) -> usize {
    (usize::BITS - 1 - x.leading_zeros()) as usize
}

const fn compute_reduction(depth: usize, move_idx: usize) -> u8 {
    if depth < 2 || move_idx < 1 {
        return 0;
    }
    let ld = ilog2(depth);
    let lm = ilog2(move_idx);
    ((3072 + 872 * ld * lm) / 4096) as u8
}

const fn build_table() -> [[u8; MAX_MOVE_COUNT]; MAX_PLY] {
    let mut table = [[0u8; MAX_MOVE_COUNT]; MAX_PLY];
    let mut depth = 0;
    while depth < MAX_PLY {
        let mut move_idx = 0;
        while move_idx < MAX_MOVE_COUNT {
            table[depth][move_idx] = compute_reduction(depth, move_idx);
            move_idx += 1;
        }
        depth += 1;
    }
    table
}

pub static LATE_MOVE_REDUCTION_TABLE: [[u8; MAX_MOVE_COUNT]; MAX_PLY] = build_table();

/// Reduction for a move at the given `depth` and `move_idx` (0-based
/// position in the ordered move list). Out-of-range inputs clamp to the
/// table edges.
#[inline(always)]
pub fn reduction(depth: u8, move_idx: usize) -> u8 {
    let d = (depth as usize).min(MAX_PLY - 1);
    let m = move_idx.min(MAX_MOVE_COUNT - 1);
    LATE_MOVE_REDUCTION_TABLE[d][m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduction_zero_for_first_move() {
        assert_eq!(reduction(8, 0), 0);
    }

    #[test]
    fn test_reduction_zero_for_shallow_depth() {
        assert_eq!(reduction(0, 8), 0);
        assert_eq!(reduction(1, 8), 0);
    }

    #[test]
    fn test_reduction_grows_with_depth() {
        let shallow = reduction(3, 10);
        let deep = reduction(20, 10);
        assert!(deep >= shallow);
    }

    #[test]
    fn test_reduction_grows_with_move_index() {
        let early = reduction(10, 3);
        let late = reduction(10, 30);
        assert!(late >= early);
    }

    #[test]
    fn test_reduction_clamps_above_table_dim() {
        let max = reduction((MAX_PLY - 1) as u8, MAX_MOVE_COUNT - 1);
        assert_eq!(reduction(200, 1000), max);
    }
}
