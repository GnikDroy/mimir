pub mod adapter;
pub mod io;
pub mod parser;

use once_cell::sync::Lazy;

use crate::ATTACK_TABLE;

pub fn run() {
    Lazy::force(&ATTACK_TABLE);

    let mut adapter = adapter::UCIAdapter::new();

    while let Some(command) = parser::UCICommand::read() {
        if !adapter.handle_command(command) {
            break;
        }
    }
}
