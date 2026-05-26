use chess_engine::{pgn::uci_to_pgn, search, GameState, MoveMethods, ATTACK_TABLE};
use once_cell::sync::Lazy;

fn main() {
    Lazy::force(&ATTACK_TABLE);

    let fen = "8/8/4k3/8/8/8/8/KBB5 w - - 0 1";
    let fen = "8/8/4k3/8/8/8/1N6/KB6 w - - 0 1";
    let mut state = GameState::from_fen(fen).unwrap();

    let mut searcher = search::Searcher::new();

    let limit = 100;
    let mut cnt = 0;
    let mut history = vec![];

    while !state.is_checkmate() && !state.is_stalemate() && cnt < limit {
        let start_time = std::time::Instant::now();
        let result = searcher.search(&mut state, 14);
        let elapsed = start_time.elapsed();
        println!(
            "Position: {}, Best move: {:?}, Eval: {}, Depth: {}, QDepth: {}, Nodes: {}, Time: {:.2?}, Nodes/s: {:.2}",
            state,
            result.best_move.unwrap().repr_string(),
            result.evaluation,
            result.depth,
            result.max_quiescence_depth_reached,
            result.nodes_searched,
            elapsed,
            result.nodes_searched as f64 / elapsed.as_secs_f64().max(0.00001)
        );
        history.push(result.best_move.unwrap());
        state.make_move(result.best_move.unwrap());
        cnt += 1;
    }

    let uci = history
        .iter()
        .map(|m| m.repr_string())
        .collect::<Vec<_>>()
        .join(" ");
    println!("Best move sequence: {}", uci);
    println!("{}", uci_to_pgn(&uci, fen.into()));
}
