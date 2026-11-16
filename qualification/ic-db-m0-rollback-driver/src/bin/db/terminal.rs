//! Foreground line editing; the prepared launcher still owns the simulator.

use rustyline::{DefaultEditor, error::ReadlineError};
use std::{
    io::{self, BufRead, Write},
    process::Child,
};

// Private pipe delimiter, emitted only in the terminal-relay mode. Normal output
// escapes control characters (including JSON), so it cannot request input.
const INPUT_REQUEST: u8 = 5;

pub(super) fn request_input() -> io::Result<()> {
    let mut output = io::stdout().lock();
    output.write_all(&[INPUT_REQUEST])?;
    output.flush()
}

pub(super) fn readline(editor: &mut DefaultEditor) -> Result<Option<String>, String> {
    loop {
        match editor.readline("db> ") {
            Ok(line) => {
                if line.contains(['\r', '\n']) {
                    eprintln!("Enter one command at a time.");
                    continue;
                }
                // Rustyline retains at most 100 entries and ignores empty lines
                // and consecutive duplicates. No history file is opened.
                if line.len() < 4096 {
                    editor
                        .add_history_entry(line.as_str())
                        .map_err(|e| e.to_string())?;
                }
                return Ok(Some(line));
            }
            Err(ReadlineError::Interrupted) => println!("^C"),
            Err(ReadlineError::Eof) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        }
    }
}

pub(super) fn relay(child: &mut Child, editor: &mut DefaultEditor) -> Result<(), String> {
    let mut input = child.stdin.take().ok_or("missing console input pipe")?;
    let mut output = io::BufReader::new(child.stdout.take().ok_or("missing console output pipe")?);
    loop {
        let bytes = output.fill_buf().map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Ok(());
        }
        let request = bytes.iter().position(|byte| *byte == INPUT_REQUEST);
        let end = request.unwrap_or(bytes.len());
        io::stdout()
            .write_all(&bytes[..end])
            .map_err(|e| e.to_string())?;
        io::stdout().flush().map_err(|e| e.to_string())?;
        output.consume(end + usize::from(request.is_some()));
        if request.is_some() {
            let Some(line) = readline(editor)? else {
                return Ok(());
            };
            writeln!(input, "{line}").map_err(|error| {
                if error.kind() == io::ErrorKind::BrokenPipe {
                    "The local database session has ended. Restart db to create a new temporary fixture.".into()
                } else {
                    format!("cannot send console command: {error}")
                }
            })?;
        }
    }
}
