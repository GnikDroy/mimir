fn main() {
    let command = std::env::args().skip(1).collect::<Vec<_>>().join(" ");

    if command.trim().is_empty() {
        mimir::uci::run();
    } else {
        mimir::uci::run_command(&command);
    }
}
