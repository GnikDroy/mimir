//! Searches a fixed list of positions to a fixed depth and reports the
//! total node count, elapsed time and speed.
use crate::time::Instant;

use crate::search::{SearchLimits, SearchResult, Searcher};
use crate::state::GameState;

/// Depth used when `bench` is given no argument.
pub const DEFAULT_DEPTH: u8 = 12;

/// Positions searched
const POSITIONS: [&str; 16] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "rnbqkb1r/1p2pppp/p2p1n2/8/3NP3/2N5/PPP2PPP/R1BQKB1R w KQkq - 0 6",
    "r1bqkbnr/pp2pppp/2np4/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 0 4",
    "rnbqkb1r/pp3ppp/4pn2/2pp4/2PP4/5NP1/PP2PP1P/RNBQKB1R w KQkq - 0 5",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "r2q1rk1/pp2ppbp/2n2np1/2p5/2PP4/2N1PN2/PP3PPP/R1BQ1RK1 w - - 0 10",
    "2rq1rk1/pb2bppp/1p2pn2/8/2BN4/2N1P3/PP3PPP/2RQ1RK1 w - - 0 15",
    "r1b2rk1/2q1bppp/p2ppn2/1p6/3BPP2/2NB4/PPPQ2PP/2KR3R w - - 0 14",
    "rnb1kb1r/1p3ppp/pq1ppn2/6B1/3NPP2/2N5/PPP3PP/R2QKB1R w KQkq - 1 8",
    "3qr2k/1p3rbp/2p3p1/p7/P2pBNn1/1P3n2/6P1/B1Q1RR1K b - - 1 30",
    "r3r1k1/pp1q1ppp/2np1n2/2p5/2P5/1PN1PN2/PB1Q1PPP/3RR1K1 w - - 0 16",
    "4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "8/5ppp/8/5PPP/8/8/1k6/6K1 w - - 0 1",
    "4r3/1k6/pp3r2/1b2P2p/3R1p2/P1R2P2/1P4PP/6K1 w - - 0 35",
    "8/8/4k3/8/4P3/4K3/8/8 w - - 0 1",
];

/// Searches every position in [`POSITIONS`] to `depth` and writes a
/// summary to `writer`.
pub fn run<W: std::io::Write>(depth: u8, writer: &mut W) -> std::io::Result<()> {
    let mut searcher = Searcher::default();
    searcher.new_game();

    let mut total_nodes = 0u64;
    let started = Instant::now();

    for (i, fen) in POSITIONS.iter().enumerate() {
        let Ok(mut state) = GameState::from_fen(fen) else {
            writeln!(writer, "info string bench: bad FEN at index {i}")?;
            continue;
        };
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(depth),
            &[],
            None::<fn(SearchResult)>,
        );
        total_nodes += result.analytics.total_nodes();
    }

    let elapsed = started.elapsed();
    let millis = elapsed.as_millis().max(1);
    let nps = (total_nodes as u128 * 1000 / millis) as u64;

    writeln!(writer, "===========================")?;
    writeln!(writer, "Depth           : {depth}")?;
    writeln!(writer, "Positions       : {}", POSITIONS.len())?;
    writeln!(writer, "Total time (ms) : {millis}")?;
    writeln!(writer, "Nodes searched  : {total_nodes}")?;
    writeln!(writer, "Nodes/second    : {nps}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_bench_positions_are_valid_fens() {
        for (i, fen) in POSITIONS.iter().enumerate() {
            assert!(
                GameState::from_fen(fen).is_ok(),
                "position {i} is not a valid FEN: {fen}",
            );
        }
    }

    #[test]
    fn test_node_count_is_reproducible() {
        fn nodes_at_depth(depth: u8) -> String {
            let mut out = Vec::new();
            run(depth, &mut out).unwrap();
            String::from_utf8(out)
                .unwrap()
                .lines()
                .find(|line| line.starts_with("Nodes searched"))
                .unwrap()
                .to_string()
        }
        assert_eq!(nodes_at_depth(6), nodes_at_depth(6));
    }
}
