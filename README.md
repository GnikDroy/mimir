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
- [x] Killer moves and history heuristics for quiet moves.
- [x] Principal variation search (PVS) with aspiration windows.
- [x] Null move pruning (NMP) and Late move reductions (LMR)
- [x] Reverse Futility Pruning (RFP)
- [x] Delta pruning and Static Exchange Evaluation (SEE).
- [x] A UCI compatible interface with a soft-hard limit time scheduler.
- [x] Playable on lichess [@playmimir](www.lichess.org/@/playmimir). Uses the lichess bot API. 

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
