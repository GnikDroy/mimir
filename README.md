# A UCI compatible chess engine.

An attempt to create a relatively strong AI for chess from scratch.

Although rewarding, creating a chess engine is a time consuming endeavour. Strong engines like StockFish have had decades of development invested into them. After my initial research, I have set a clear end goal and feature set for this project.

## Features

- Pseudo-legal bitboard move generation tested with several perft tests.
- Zobrist hashing, transposition tables, and draw-by-repetition detection.
- A PST based tapered evaluation function optimized by Texel's tuning.
- 768 perspective NNUE based evalulation with incremental updates.
  (the network used is beans.bin, Credits to @ciekce [Stormphrax] from discord for training the net)
- Negamax alpha-beta pruning with iterative deepening depth first search and quiescence search to counter horizon effects.
- Move ordering via hash moves from transposition tables, Most Valuable Victim - Least Valuable Aggressor (MVV-LVA) and Static Exchange Evaluation (SEE).
- Killer moves and history heuristics for quiet moves.
- Principal variation search (PVS) with aspiration windows.
- Null move pruning (NMP) and Late move reductions (LMR)
- A UCI compatible interface with proper time management.
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
