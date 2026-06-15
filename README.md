# Mimir

Mimir is a UCI compatible chess engine built from scratch.

## Features

- Bitboard move generation (legal & pseudo-legal) verified with perft tests.
- Full compliance with FIDE rules: 50 move rule, three-fold-repetition & insufficient material.
- Negamax alpha-beta pruning with iterative deepening depth first search and quiescence search to counter horizon effects.
- Transposition tables with zobrist hashing.
- A classical PST tapered evaluation function optimized by Texel's tuning.
- Modern NNUE based evaluation based on 768 perspective architechture with incremental updates.
- Move ordering via hash moves from transposition tables, Most Valuable Victim - Least Valuable Aggressor (MVV-LVA) and Static Exchange Evaluation (SEE).
- Killer moves and history heuristics for quiet moves.
- Principal variation search (PVS) with aspiration windows.
- Null move pruning (NMP) and Late move reductions (LMR)
- A UCI compatible interface with a time scheduler.
- Playable on lichess via the lichess bot API.

## Building

### Requirements

- Rust 1.70+ (for `#![feature(variant_count)]`)
- Standard build tools (cargo)

### Building and Testing

```bash
cargo build --release

# Run all tests
cargo test --release
```

## References

- [Chess Programming Wiki](https://www.chessprogramming.org/) – comprehensive reference for most techniques outlined above
- [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) - A bridge between lichess and bots.
- beans.bin - Credits to @ciekce [Stormphrax] from discord for training the network
