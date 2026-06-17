//! Precomputed reduction values for Late Move Reductions.
//!
//! Indexed by `[depth][move_index]`. Reductions grow with both axes:
//! deeper searches can afford bigger reductions, and later moves in the
//! ordering are less likely to be best.
//!
//! Formula: `BASE + ln(d) * ln(m) / DIV`. Tune aggression by adjusting
//! `BASE` (baseline reduction floor) or `DIV` (smaller → more aggressive
//! slope).

use crate::search::MAX_PLY;
use crate::types::MAX_MOVE_COUNT;

/// Reduction floor added to every (d, m) entry once `d ≥ 2` and `m ≥ 1`.
const BASE: f64 = 0.75;

/// Divisor on the `ln(d) * ln(m)` term. Lower → reduces harder.
const DIV: f64 = 2.25;

/// `const fn` natural-log approximation for positive integers.
///
/// Range-reduces via `x = 2^k * m` with `m ∈ [1, 2)`, then evaluates
/// `ln(m) = 2 * atanh((m-1)/(m+1))` as a 5-term odd-power series. The
/// substitution variable `u = (m-1)/(m+1)` lies in `[0, 1/3)`, so the
/// remainder term `u^11 / 11 < 4e-8` — comfortably below any rounding
/// step of the `u8` reduction table.
const fn ln_approx(x: usize) -> f64 {
    if x <= 1 {
        return 0.0;
    }
    let k = x.ilog2();
    let scale = 1usize << k;
    let m = (x as f64) / (scale as f64);
    let u = (m - 1.0) / (m + 1.0);
    let u2 = u * u;
    let u3 = u2 * u;
    let u5 = u3 * u2;
    let u7 = u5 * u2;
    let u9 = u7 * u2;
    let atanh_u = u + u3 / 3.0 + u5 / 5.0 + u7 / 7.0 + u9 / 9.0;
    std::f64::consts::LN_2 * (k as f64) + 2.0 * atanh_u
}

const fn compute_reduction(depth: usize, move_idx: usize) -> u8 {
    if depth < 2 || move_idx < 1 {
        return 0;
    }
    let r = BASE + ln_approx(depth) * ln_approx(move_idx) / DIV;
    r as u8
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

    #[test]
    fn test_ln_approx_matches_std() {
        for x in 1..=MAX_MOVE_COUNT {
            let approx = ln_approx(x);
            let exact = (x as f64).ln();
            assert!(
                (approx - exact).abs() < 1e-5,
                "ln_approx({x}) = {approx} but ln({x}) = {exact}",
            );
        }
    }
}
