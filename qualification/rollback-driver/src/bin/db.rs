#![forbid(unsafe_code)]
//! Local product-record console. Real canister integration belongs to Canic.

use clap::{CommandFactory, Parser, Subcommand};
use ic_db_m0_rollback_driver::{LIMITS, instance};
use ic_host_text::{escape::terminal_text, table::render_bordered};
use ic_wire::native;
use std::io::{self, BufRead, IsTerminal, Write};

#[derive(Parser)]
#[command(
    name = "db",
    about = "Inspect and query a fresh local IC DB fixture (no SQL)",
    after_help = "Without a command, read commands interactively or from standard input.\nThis M1 fixture has six Entry rows, one record each for Document, WideNumbers, BinaryObject, NarrowNumbers and IdentityRow, and an empty OptionalLink entity; it is discarded on exit.\nIt is not a deployed database. query currently reads Entry rows.",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Inspect generated fields, types, indexes and admitted schema identity.
    Schema,
    /// Inspect the manifest identity, entity revision and physical allocation.
    Status,
    /// Read bounded Entry candidates in ID order; continue even when no rows match.
    Query {
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t=3, value_parser=clap::value_parser!(u64).range(1..=3))]
        limit: u64,
        /// Retain candidates with amount at least this value.
        #[arg(long = "min", default_value_t = 0)]
        minimum: u64,
    },
    /// Display command help.
    Help,
    /// Leave the console.
    #[command(alias = "exit")]
    Quit,
}
fn help() {
    Cli::command().print_help().unwrap();
    println!();
}
type Status = ([u8; 32], [u8; 16], u64, u64);
type FieldView = (String, String, bool, Option<u32>, Option<(u32, String)>);
type IndexView = (String, Vec<String>, bool, bool);
type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);

fn print_schema(entities: Vec<EntityView>) {
    println!("\nSchema: {} entities\n", entities.len());
    let targets: std::collections::BTreeMap<_, _> = entities
        .iter()
        .map(|(name, id, primary, _, _)| (*id, format!("{name}.{primary}")))
        .collect();
    for (name, _, primary, mut fields, indexes) in entities {
        println!("{}", terminal_text(&name));
        println!(
            "Primary key: {} (supplied by the application)\n",
            terminal_text(&primary)
        );
        fields.sort_by_key(|field| field.0 != primary); // primary first, otherwise canonical order
        let references = fields.iter().any(|field| field.4.is_some());
        let rows: Vec<Vec<String>> = fields
            .into_iter()
            .map(|(name, kind, optional, max, reference)| {
                let key = if name == primary { "Primary" } else { "-" };
                let mut row = vec![
                    name,
                    kind,
                    if optional { "Yes" } else { "No" }.into(),
                    key.into(),
                    max.map_or_else(|| "-".into(), |bytes| format!("{bytes} bytes")),
                ];
                if references {
                    row.push(reference.map_or_else(
                        || "-".into(),
                        |(entity, index)| format!("{} (via {index})", targets[&entity]),
                    ));
                }
                row
            })
            .collect();
        let mut columns = vec!["Field", "Type", "Optional", "Key", "Limit"];
        if references {
            columns.push("References");
        }
        println!("{}", render_bordered(&columns, &rows));
        if indexes.is_empty() {
            println!("Indexes: none\n");
        } else {
            let rows: Vec<Vec<String>> = indexes
                .into_iter()
                .map(|(name, fields, unique, present_only)| {
                    vec![
                        name,
                        fields.join(", "),
                        if unique { "Yes" } else { "No" }.into(),
                        if present_only {
                            "All optional keys present"
                        } else {
                            "All rows"
                        }
                        .into(),
                    ]
                })
                .collect();
            println!(
                "\nIndexes\n{}\n",
                render_bordered(
                    &["Name", "Fields (in order)", "Unique", "Membership"],
                    &rows
                )
            );
        }
    }
    println!(
        "Optional fields can be absent. Text limits count UTF-8 bytes; Blob limits count raw bytes."
    );
    println!(
        "Present references require an existing target and restrict its deletion; absent references have no target."
    );
    println!("Stored schema matches this executable. Use db status for storage details.");
}

fn print_status((schema, incarnation, revision, pages): Status) {
    let rows = vec![
        vec!["Format".into(), "ICDBMF01".into()],
        vec!["Schema admission".into(), "Exact match".into()],
        vec![
            "Schema fingerprint".into(),
            schema.map(|b| format!("{b:02x}")).concat(),
        ],
        vec![
            "Database incarnation".into(),
            incarnation.map(|b| format!("{b:02x}")).concat(),
        ],
        vec!["Entry revision".into(), revision.to_string()],
        vec![
            "Physical allocation".into(),
            format!("{pages} pages (64 KiB each)"),
        ],
    ];
    println!("\n{}", render_bordered(&["Property", "Value"], &rows));
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Help)) {
        help();
        return Ok(());
    }
    if matches!(cli.command, Some(Command::Quit)) {
        return Ok(());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let prefix = executable
        .parent()
        .and_then(|bin| bin.parent())
        .ok_or("cannot locate installation prefix; reinstall with make db")?;
    let fixture = prefix.join("share/ic-db/m1/records.wasm");
    let wasm = std::fs::read(&fixture)
        .map_err(|e| format!("{}: {e}; reinstall with make db", fixture.display()))?;
    let owned = instance().await?;
    let pic = owned.instance();
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm, vec![], None).await;
    println!("IC DB — LOCAL M1 FIXTURE (fresh temporary database; discarded on exit)");
    println!("Enter help for commands. Use schema to inspect all entities; query reads Entry.");
    let interactive = io::stdin().is_terminal() && cli.command.is_none();
    let execute = async |command: Command| -> Result<(), String> {
        match command {
            Command::Help => help(),
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
                    let (_, entities): (Status, Vec<EntityView>) =
                        native::decode_response(id, &reply, LIMITS).map_err(|e| e.to_string())?;
                    print_schema(entities);
                } else {
                    print_status(
                        native::decode_response(id, &reply, LIMITS).map_err(|e| e.to_string())?,
                    );
                }
            }
            Command::Query {
                after,
                limit,
                minimum,
            } => {
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
                let count = rows.len();
                let rows: Vec<Vec<String>> = rows
                    .into_iter()
                    .map(|(id, amount)| vec![id.to_string(), amount.to_string()])
                    .collect();
                println!("\n{}", render_bordered(&["id", "amount"], &rows));
                println!("{count} rows returned; {scanned} candidates examined.");
                if let Some(after) = next {
                    let command = if interactive { "query" } else { "db query" };
                    let filter = if minimum == 0 {
                        String::new()
                    } else {
                        format!(" --min {minimum}")
                    };
                    println!("Next page: {command} --after {after} --limit {limit}{filter}");
                } else {
                    println!("Complete: no more candidates.");
                }
            }
        }
        Ok(())
    };
    let outcome = if let Some(command) = cli.command {
        execute(command).await
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
            if line.trim().is_empty() {
                continue;
            }
            // Console tokenization only: Clap owns the same grammar and validation as argv.
            match Cli::try_parse_from(std::iter::once("db").chain(line.split_whitespace())) {
                Ok(Cli {
                    command: Some(Command::Quit),
                }) => break Ok(()),
                Ok(Cli {
                    command: Some(command),
                }) => {
                    if let Err(error) = execute(command).await {
                        eprintln!("{error}");
                        if !interactive {
                            break Err(error);
                        }
                    }
                }
                Ok(Cli { command: None }) => {}
                Err(error) => {
                    error.print().map_err(|e| e.to_string())?;
                    if error.use_stderr() && !interactive {
                        break Err("invalid command".into());
                    }
                }
            }
        }
    };
    owned.close().await;
    outcome
}
