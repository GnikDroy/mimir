use std::fs::{File, OpenOptions};
use std::io::{BufRead, Write};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;

static LOG_FILE: Lazy<Mutex<File>> = Lazy::new(|| {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("uci.log")
        .expect("failed to open uci.log");
    Mutex::new(file)
});

static STDOUT: Lazy<Mutex<std::io::Stdout>> = Lazy::new(|| Mutex::new(std::io::stdout()));

fn log_line(direction: &str, line: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    if let Ok(mut file) = LOG_FILE.lock() {
        let _ = writeln!(file, "[{ts}] {direction} {line}");
        let _ = file.flush();
    }
}

pub fn read() -> Option<String> {
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let mut line = String::new();

    let bytes_read = handle.read_line(&mut line).ok()?;
    if bytes_read == 0 {
        return None;
    }

    let trimmed = line.trim_end_matches(['\r', '\n']);
    log_line("IN", trimmed);
    Some(line)
}

pub fn write(line: &str) {
    if let Ok(mut out) = STDOUT.lock() {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    log_line("OUT", line);
}
