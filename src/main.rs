use chess_engine::{search, GameState, MoveMethods, ATTACK_TABLE};
use once_cell::sync::Lazy;

fn main() {
    Lazy::force(&ATTACK_TABLE);

    let mut state = GameState::new();

    // mate in one (d2d4)
    // let mut state = GameState::from_fen("3r4/1K6/2Nb4/2kb4/8/8/3PB3/8 w - - 0 1").unwrap();

    // mate in two (b4f8, g8f8, e2e8)
    // let mut state =
    //     GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();

    //mate in three
    // let mut state = GameState::from_fen("8/8/8/P7/5knN/1P6/7p/7K b - - 1 53").unwrap();

    let mut searcher = search::Searcher::new();

    let limit = 200;
    let mut cnt = 0;
    let mut history = vec![];

    while !state.is_checkmate() && !state.is_stalemate() && cnt < limit {
        let result = searcher.search(&mut state, 6);
        println!(
            "Position: {}, Best move: {:?}, Eval: {}, Depth: {}, QDepth: {}, Nodes: {}",
            state,
            result.best_move.unwrap().repr_string(),
            result.evaluation,
            result.depth,
            result.max_quiescence_depth_reached,
            result.nodes_searched
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
