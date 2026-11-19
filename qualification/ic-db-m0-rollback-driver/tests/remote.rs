//! Native CLI refusal against an independent HTTP peer; no fixture or credentials.

use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum Reply {
    Busy(u16),
    Stall,
    Redirect,
    Oversized,
}

fn refuse(command: &str, reply: Reply, local_root: bool) -> (Output, Vec<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (stop, stopped) = mpsc::channel();
    let peer = thread::spawn(move || {
        let mut requests = Vec::new();
        loop {
            if stopped.try_recv().is_ok() {
                break;
            }
            let (mut socket, _) = match listener.accept() {
                Ok(socket) => socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("accept failed: {e}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 8192];
            let mut read = 0;
            // TCP may split the request line across reads. Keep capture bounded.
            while !bytes[..read].contains(&b'\n') {
                let count = socket.read(&mut bytes[read..]).unwrap();
                assert!(count > 0, "missing or oversized HTTP request line");
                read += count;
            }
            assert!(bytes[..read].starts_with(if local_root { b"GET " } else { b"POST " }));
            requests.push(
                String::from_utf8_lossy(&bytes[..read])
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned(),
            );

            match reply {
                Reply::Busy(status) => write!(
                    socket, "HTTP/1.1 {status} Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ).unwrap(),
                Reply::Redirect => write!(
                    socket, "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{address}/redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ).unwrap(),
                Reply::Oversized => {
                    let body = vec![0; 2 * 1024 * 1024 + 1];
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                    // Early rejection can close the socket before the final byte.
                    if let Err(error) = socket.write_all(&body) {
                        assert!(matches!(error.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset), "{error}");
                    }
                }
                Reply::Stall => {
                    // Keep the accepted request unanswered until the CLI exits.
                    let _ = stopped.recv_timeout(Duration::from_secs(8));
                    break;
                }
            }
        }

        requests
    });
    let mut process = Command::new(env!("CARGO_BIN_EXE_db"));
    process.args([
        command,
        "--provider",
        &format!("http://{address}"),
        "--canister",
        "aaaaa-aa",
        "--anonymous",
        "--timeout-secs",
        "1",
        "--json",
    ]);
    if local_root {
        process.arg("--local-root-key");
    }
    let mut child = process
        .env_remove("IC_TESTKIT_POCKET_IC_URL")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let mut exceeded = false;
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(5) {
            exceeded = true;
            child.kill().unwrap();
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let _ = stop.send(());
    let requests = peer.join().unwrap();
    assert!(!exceeded, "CLI exceeded the test watchdog");
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "no partial JSON or fixture fallback"
    );

    (output, requests)
}

#[test]
fn busy_remote_provider_is_not_retried() {
    for command in ["inspect", "witness-list"] {
        for status in [429, 503] {
            let (output, requests) = refuse(command, Reply::Busy(status), false);
            // Agent may concurrently fetch node keys. Each distinct endpoint
            // gets at most one attempt; verification reads are not retries.
            assert!(!requests.is_empty() && requests.len() <= 2, "{requests:?}");
            let unique = requests.iter().collect::<std::collections::BTreeSet<_>>();
            assert_eq!(unique.len(), requests.len(), "{requests:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains(&status.to_string()), "{error}");
            assert_eq!(error.matches("query call (").count(), 1);
            assert!(error.contains("cycles unavailable"), "{error}");
        }
    }
}

#[test]
fn stalled_remote_query_returns_with_call_statistics() {
    for command in ["inspect", "witness-list"] {
        let (output, requests) = refuse(command, Reply::Stall, false);
        assert_eq!(requests.len(), 1);
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("query deadline exceeded (1 sec)"), "{error}");
        assert_eq!(error.matches("query call (").count(), 1);
        assert!(error.contains("cycles unavailable"), "{error}");
    }
}

#[test]
fn local_root_failure_never_dispatches_a_database_query() {
    for command in ["inspect", "witness-list"] {
        for reply in [Reply::Stall, Reply::Busy(429), Reply::Busy(503)] {
            let (output, requests) = refuse(command, reply, true);
            assert_eq!(requests, ["GET /api/v2/status HTTP/1.1"]);
            let error = String::from_utf8(output.stderr).unwrap();
            let expected = match reply {
                Reply::Stall => "local root-key query deadline exceeded",
                _ => "cannot fetch local root key",
            };
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("query call ("), "no database call was made");
        }
    }
}

#[test]
fn redirects_and_oversized_http_replies_refuse_before_protocol_decoding() {
    for command in ["inspect", "witness-list"] {
        for reply in [Reply::Redirect, Reply::Oversized] {
            let (output, requests) = refuse(command, reply, false);
            assert!(
                requests
                    .iter()
                    .all(|request| !request.contains("/redirected"))
            );
            let error = String::from_utf8(output.stderr).unwrap();
            let expected = match reply {
                Reply::Redirect => "307",
                _ => "length limit exceeded",
            };
            assert!(error.contains(expected), "{error}");
            assert_eq!(error.matches("query call (").count(), 1);
            assert!(error.contains("cycles unavailable"), "{error}");
        }
    }
}
