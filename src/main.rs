use chess_engine::{search, GameState, MoveMethods, ATTACK_TABLE};
use once_cell::sync::Lazy;

fn main() {
    Lazy::force(&ATTACK_TABLE);

    let mut state = GameState::new();

    let mut searcher = search::Searcher::new();

    let limit = 200;
    let mut cnt = 0;
    let mut history = vec![];

    while !state.is_checkmate() && !state.is_stalemate() && cnt < limit {
        let start_time = std::time::Instant::now();
        let result = searcher.search(&mut state, 6);
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

    history
        .iter()
        .map(|m| m.repr_string())
        .for_each(|s| print!("{} ", s));
}
