# Mimir

Mimir is a UCI compatible chess engine built from scratch.

## Features

- [x] Bitboard move generation (legal & pseudo-legal) verified with perft tests.
- [x] Full compliance of drawing rules: Insufficient material, three-fold-repetition, and 50 move rule.
- [x] Negamax alpha-beta pruning with iterative deepening depth first search and quiescence search to counter horizon effects.
- [x] Transposition tables with zobrist hashing.
- [x] A classical PST tapered evaluation function optimized by Texel's tuning.
- [x] Modern NNUE based evaluation based on 768 perspective architechture with incremental updates.
- [x] Move ordering via hash moves from transposition tables, Most Valuable Victim - Least Valuable Aggressor (MVV-LVA) for captures.
- [x] Killer moves and history heuristics with gravity and malus for quiet moves.
- [x] Principal variation search (PVS) with aspiration windows.
- [x] Null move pruning (NMP) and Late move reductions (LMR)
- [x] Reverse Futility Pruning (RFP)
- [x] Delta pruning and Static Exchange Evaluation (SEE).
- [x] Futility Pruning
- [x] A UCI compatible interface with a soft-hard limit time scheduler.
- [x] Playable on lichess [@playmimir](https://lichess.org/@/playmimir). Uses the lichess bot API. 

## Building

### Requirements

- Stable Rust, no nightly features and no dependencies
- Standard build tools (cargo)

### Building and Testing

```bash
cargo build --release

# Run all tests
cargo test --release
```

### WebAssembly

```bash
RUSTFLAGS="-C target-feature=+simd128" CARGO_PROFILE_RELEASE_DEBUG=false \
    cargo build --release --target wasm32-unknown-unknown --lib
cp target/wasm32-unknown-unknown/release/mimir.wasm web/

# Browsers refuse ES modules and wasm over file://, so serve the directory.
python3 -m http.server --directory web 8000
```

Runs at roughly two thirds the speed of the native binary.

### NNUE Training & Network

Mimir uses a simple NNUE network instead of a HCE by default.

`mimir-512-v1.bin` is a \[ (768 → 512)×2 -> 1 \] network trained on 3B unique positions.
We use bullet for training the network. The training code is present in training/

## Strength

Mimir is estimated at roughly 3325 Elo on the CCRL Blitz scale, placing it between Stash 34 (3315) and Stash 35 (3345) as of Nov 7, 2026.

## References

- [Chess Programming Wiki](https://www.chessprogramming.org/) – comprehensive reference for most techniques outlined above
- [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) - A bridge between lichess and bots.
