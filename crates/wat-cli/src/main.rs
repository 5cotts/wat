use std::io::{self, BufRead, Write};
use wat_core::Shell;

fn main() {
    let mut shell = Shell::new();
    let stdin = io::stdin();
    let stdout = io::stdout();

    print!("{}", shell.prompt());
    stdout.lock().flush().unwrap();

    for line in stdin.lock().lines() {
        let line = line.expect("failed to read line");
        let output = shell.feed(&line);
        if !output.is_empty() {
            print!("{}", output);
        }
        if shell.exit_requested {
            std::process::exit(shell.last_exit_code());
        }
        print!("{}", shell.prompt());
        stdout.lock().flush().unwrap();
    }
}
