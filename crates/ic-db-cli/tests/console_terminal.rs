//! Exercise the installed-layout CLI through a controlling terminal and its real launcher.
#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "support/installed.rs"]
mod installed;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

fn prompt(
    output: &mpsc::Receiver<Vec<u8>>,
    timeout: Duration,
    expected: &str,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();

    loop {
        let chunk = output
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|e| {
                format!(
                    "no prompt: {e}; output: {}",
                    String::from_utf8_lossy(&bytes)
                )
            })?;
        bytes.extend(chunk);
        // Ignore terminal control sequences, not rendered text. Arrow-key
        // redraws may contain prompts before the requested result arrives.
        let mut visible = Vec::new();
        let mut escape = 0;
        for &byte in &bytes {
            match (escape, byte) {
                (0, 27) => escape = 1,
                (1, b'[') => escape = 2,
                (1, _) | (2, 0x40..=0x7e) => escape = 0,
                (2, _) | (0, b'\r') => {}
                _ => visible.push(byte),
            }
        }
        let text = String::from_utf8_lossy(&visible);
        if escape == 0 && text.ends_with("db> ") && text.contains(expected) {
            return Ok(text.into_owned());
        }
        if bytes.len() > 1_048_576 {
            return Err("terminal output exceeds test bound".into());
        }
    }
}

#[test]
fn terminal_count_returns_without_eof_and_prints_call_statistics() {
    terminal_session(false);
}

#[test]
#[ignore = "90-second idle regression; select explicitly when changing console lifetime"]
fn terminal_survives_pocketic_default_idle_timeout() {
    terminal_session(true);
}

fn terminal_session(idle: bool) {
    let prefix = std::env::temp_dir().join(format!("ic-db-terminal-{}-{idle}", std::process::id()));
    let binary = installed::install(&prefix);
    // script supplies the controlling terminal missing from ordinary piped tests.
    let mut command = Command::new("script");
    #[cfg(target_os = "linux")]
    command.args([
        "-qec",
        &format!("exec '{}'", binary.to_string_lossy().replace('\'', "'\\''")),
        "/dev/null",
    ]);
    #[cfg(target_os = "macos")]
    command.args([
        std::ffi::OsStr::new("-q"),
        std::ffi::OsStr::new("/dev/null"),
        binary.as_os_str(),
    ]);
    let mut child = command
        .env("TERM", "xterm-256color")
        .env_remove("IC_TESTKIT_POCKET_IC_URL")
        .env_remove("IC_DB_TERMINAL_INPUT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (send, output) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = [0; 512];
        while let Ok(count) = stdout.read(&mut bytes) {
            if count == 0 || send.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let result = (|| -> Result<(), String> {
        prompt(&output, Duration::from_secs(45), "LOCAL M1 FIXTURE")?;
        if idle {
            // No calls or keepalive requests: exceed the server's 60-second default.
            for _ in 0..2 {
                std::thread::sleep(Duration::from_secs(45));
            }
        }
        for (command, expected) in [
            ("count Document\n", "| 1     |"),
            ("count Entry\n", "| 6     |"),
            ("\x1b[A\x1b[A\n", "| 1     |"), // up twice: Document
            ("\x1b[A\x1b[A\x1b[B\n", "| 1     |"), // down: Document again
            ("count Entrx\x1b[D\x1b[3~y\n", "| 6     |"), // edit in place
            ("count Document\x1b[A\x1b[B\n", "| 1     |"), // restore draft
            ("count Entry\x03", "^C"),       // cancel input, retain session
            (
                "\x1b[200~count Document\ncount Entry\x1b[201~\n",
                "Enter one command at a time.",
            ),
            ("\n", ""),
            ("unknown-command\n", ""),
            ("count 'Document\n", "unterminated quote or trailing escape"),
            ("count 'Document'\n", "| 1     |"),
            (
                "query Document --select min:note --group-min \"a b\"\n",
                "query call (query):",
            ),
            (
                "query Document --select min:note --group-min ''\n",
                "query call (query):",
            ),
            ("count Document\n", "| 1     |"),
        ] {
            input
                .write_all(command.as_bytes())
                .map_err(|e| e.to_string())?;
            let response = prompt(&output, Duration::from_secs(5), expected)?;
            if expected == "unterminated quote or trailing escape"
                && response.contains("query call")
            {
                return Err("incomplete quote executed a command".into());
            }
            if expected == "Enter one command at a time." && response.contains("query call") {
                return Err("multiline paste executed a command".into());
            }

            if expected.starts_with('|') {
                if !response.contains(&format!(
                    "{expected}\n+-------+\nquery call (cardinality): "
                )) || !response.contains(" sec, 0 cycles charged\n")
                {
                    return Err(format!(
                        "missing count/statistics before prompt: {response}"
                    ));
                }
            } else if command == "\n" && response.contains("Usage:") {
                return Err("blank line displayed help".into());
            }
        }
        input.write_all(b"quit\n").map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("terminal session: {status}"))
                };
            }
            if Instant::now() >= deadline {
                return Err("quit waited for more terminal input".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    // Failure also closes the controlling terminal; do not leave a stopped session.
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    std::fs::remove_dir_all(prefix).unwrap();
    result.unwrap();
}
