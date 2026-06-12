//! UCI (Universal Chess Interface) front-end.
//!
//! The UCI layer is split into two pieces:
//!
//! - [`parser`] turns raw stdin lines into typed
//!   [`parser::UCICommand`] values.
//! - [`adapter`] holds the engine's runtime state and executes those
//!   commands, dispatching searches onto a worker thread.
//!
//! [`run`] is the entry point invoked from `main` and drives the
//! read/dispatch loop until the GUI sends `quit` (or stdin closes).

pub mod adapter;
pub mod parser;

use once_cell::sync::Lazy;

use crate::ATTACK_TABLE;

/// Runs the UCI protocol loop on stdin/stdout until exit.
///
/// Forces the global [`ATTACK_TABLE`] to initialize up front so the first
/// `go` command does not pay the magic-bitboard build cost mid-search,
/// then reads commands line-by-line and forwards them to
/// [`adapter::UCIAdapter`]. The loop exits when the adapter returns
/// `false` (e.g. on `quit`) or when stdin is exhausted.
pub fn run() {
    // We forces attack table generation so the cost is paid up-front rather than mid-game.
    Lazy::force(&ATTACK_TABLE);

    let mut adapter = adapter::UCIAdapter::new();

    while let Some(command) = parser::UCICommand::read(&mut std::io::stdin().lock()) {
        if !adapter.handle_command(command) {
            break;
        }
    }
}
