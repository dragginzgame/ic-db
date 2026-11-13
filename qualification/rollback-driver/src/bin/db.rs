//! Read-only local M0 database console. Real canister integration belongs to Canic.

use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_wire::native;
use std::io::{self, BufRead, IsTerminal, Write};

const HELP: &str = "Usage: db [COMMAND]
Without a command, read commands interactively or from standard input.
Commands:
  schema                              inspect executable and stored schema
  status                              inspect format, maintenance phase and allocation
  query [--after ID] [--limit 1..3] [--min VALUE]
                                      read rows in ID order; use next_after to continue
  help
  quit
No SQL. This console reads a fresh six-row M0 fixture; it does not connect to a deployed database.";

enum Command {
    Schema,
    Status,
    Query(u64, u64, u64),
    Help,
    Quit,
}

fn parse(line: &str) -> Result<Command, String> {
    let words: Vec<_> = line.split_whitespace().collect();
    match words.as_slice() {
        ["schema"] => Ok(Command::Schema),
        ["status"] => Ok(Command::Status),
        ["help"] | [] => Ok(Command::Help),
        ["quit"] | ["exit"] => Ok(Command::Quit),
        ["query", args @ ..] => {
            let mut values = [0, 3, 0];
            let mut seen = [false; 3];
            for pair in args.chunks(2) {
                let [flag, value] = pair else {
                    return Err("query options need a value".into());
                };
                let slot = match *flag {
                    "--after" => 0,
                    "--limit" => 1,
                    "--min" => 2,
                    _ => return Err(format!("unknown query option: {flag}")),
                };
                if seen[slot] {
                    return Err(format!("duplicate option: {flag}"));
                }
                seen[slot] = true;
                values[slot] = value
                    .parse()
                    .map_err(|_| format!("invalid unsigned integer: {value}"))?;
            }
            if values[0] > 6 || !(1..=3).contains(&values[1]) {
                return Err("fixture bounds: after 0..6; limit 1..3".into());
            }
            Ok(Command::Query(values[0], values[1], values[2]))
        }
        _ => Err("unknown command; enter help (SQL is not supported)".into()),
    }
}

fn print_status(status: [u64; 9]) {
    let phase = match status[2] {
        0 => "live",
        1 => "copying",
        2 => "ready",
        3 => "cancelling",
        4 => "published/cleaning",
        _ => "unknown",
    };
    println!(
        "executable_schema={} stored_schema={} phase={phase} transformation={} cursor={}",
        status[0], status[1], status[3], status[4]
    );
    println!(
        "source_rows={} target_rows={} index_entries={} physical_pages={}",
        status[5], status[6], status[7], status[8]
    );
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let one_shot = (!args.is_empty()).then(|| args.join(" "));
    if matches!(one_shot.as_deref(), Some("help" | "--help" | "-h")) {
        println!("{HELP}");
        return Ok(());
    }
    if let Some(line) = &one_shot {
        parse(line)?;
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let prefix = executable
        .parent()
        .and_then(|bin| bin.parent())
        .ok_or("cannot locate installation prefix; reinstall with make db")?;
    let fixture = prefix.join("share/ic-db/m0/maintenance-a.wasm");
    let wasm = std::fs::read(&fixture)
        .map_err(|error| format!("{}: {error}; reinstall with make db", fixture.display()))?;
    let owned = instance().await?;
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    println!("IC DB — LOCAL M0 FIXTURE (fresh temporary database; discarded on exit)");
    println!("Enter help for commands. Query values are synthetic fixture data.");
    let interactive = io::stdin().is_terminal() && one_shot.is_none();
    let execute = async |command: Command| -> Result<(), String> {
        match command {
            Command::Help => println!("{HELP}"),
            Command::Quit => {}
            Command::Schema | Command::Status => {
                let id = if matches!(command, Command::Schema) {
                    2
                } else {
                    1
                };
                let endpoint = if id == 2 { "schema" } else { "inspect" };
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        endpoint,
                        native::encode_request(id, &(), LIMITS).map_err(|e| e.to_string())?,
                    )
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                if id == 2 {
                    let (status, text): ([u64; 9], String) =
                        native::decode_response(id, &reply, LIMITS).map_err(|e| e.to_string())?;
                    println!("{text}");
                    print_status(status);
                } else {
                    print_status(
                        native::decode_response(id, &reply, LIMITS).map_err(|e| e.to_string())?,
                    );
                }
            }
            Command::Query(after, limit, minimum) => {
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "query",
                        native::encode_request(3, &(after, limit, minimum), LIMITS)
                            .map_err(|e| e.to_string())?,
                    )
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                type Page = (Vec<(u64, u64)>, Option<u64>, u64);
                let result: Result<Page, u8> =
                    native::decode_response(3, &reply, LIMITS).map_err(|e| e.to_string())?;
                let (rows, next, scanned) = result
                    .map_err(|code| format!("database refused query (fixture code {code})"))?;
                println!("id\tvalue");
                for (id, value) in rows {
                    println!("{id}\t{value}");
                }
                println!(
                    "scanned={scanned} next_after={next:?} complete={}",
                    next.is_none()
                );
            }
        }
        Ok(())
    };
    let outcome = if let Some(line) = one_shot {
        execute(parse(&line)?).await
    } else {
        let mut input = io::stdin().lock();
        loop {
            if interactive {
                print!("db> ");
                io::stdout().flush().map_err(|e| e.to_string())?;
            }
            let mut line = String::new();
            let count = std::io::Read::take(&mut input, 4097)
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            if count == 0 {
                break Ok(());
            }
            if count > 4096 {
                break Err("command exceeds 4096 bytes".into());
            }
            match parse(&line) {
                Ok(Command::Quit) => break Ok(()),
                Ok(command) => {
                    if let Err(error) = execute(command).await {
                        eprintln!("{error}");
                        if !interactive {
                            break Err(error);
                        }
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    if !interactive {
                        break Err(error);
                    }
                }
            }
        }
    };
    owned.close().await;
    outcome
}
