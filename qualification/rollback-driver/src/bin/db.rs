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
    after_help = "Without a command, read commands interactively or from standard input.\nThis local fixture is discarded on exit; it is not a deployed database.\nSchema inspection covers all declared entities; query currently supports Entry and Document.",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Inspect generated fields, types, indexes and admitted schema identity.
    #[command(
        after_help = "Optional fields can be absent. Text limits count UTF-8 bytes; Blob limits count raw bytes.\nPresent references require an existing target and restrict its deletion; absent references have no target.\nUse db status for stored schema admission and storage details."
    )]
    Schema {
        /// Exact declared entity name; omit to inspect the whole schema.
        entity: Option<String>,
    },
    /// Inspect the manifest identity, entity revision and physical allocation.
    Status,
    /// Read one fluent page from a local fixture entity.
    #[command(
        after_help = "Order defaults to the primary key. --order-by selects a complete index order or refuses; --desc reverses every component.\nUse the returned cursor with --after and the same entity, filters, order, projection and DISTINCT setting. An empty page may still continue.\n--limit counts returned rows; --work counts examined candidates, including duplicates; --bytes counts encoded results, excluding transport framing.\n--select id works for both entities; id,amount and category work for Entry. Filters use the fields' exact declared types.\nFor category DISTINCT, use --select category --order-by category --distinct. --category is supported there; --min varies within a category and refuses.\n--select count counts matching rows; category,count groups Entry rows. Aggregates require completion before --work, refuse --after, --distinct and input ordering, and never return partial aggregates. --limit selects completed groups, not input rows. Extrema return no rows when no present value exists."
    )]
    Query {
        #[arg(default_value="Entry", value_parser=["Entry", "Document"])]
        entity: String,
        /// Opaque hexadecimal continuation from a previous page (not a row ID).
        #[arg(long, value_name="CURSOR", value_parser=parse_cursor)]
        after: Option<CursorArg>,
        #[arg(long, default_value_t=3, value_parser=clap::value_parser!(u32).range(1..=32))]
        limit: u32,
        /// Entry.amount >= VALUE.
        #[arg(long = "min")]
        minimum: Option<u64>,
        /// Entry.category == VALUE.
        #[arg(long)]
        category: Option<u64>,
        /// Document.entry_id == VALUE.
        #[arg(long)]
        entry_id: Option<u64>,
        /// Comma-separated field names; the primary key breaks ties.
        #[arg(long, value_delimiter = ',', value_name = "FIELDS")]
        order_by: Vec<String>,
        #[arg(long)]
        desc: bool,
        #[arg(long, default_value_t=128, value_parser=clap::value_parser!(u32).range(1..=512))]
        work: u32,
        #[arg(long, default_value_t=4096, value_parser=clap::value_parser!(u32).range(1..=131072))]
        bytes: u32,
        /// full, id, id,amount, category, count, count:FIELD, distinct-count:category,
        /// category,count or category,count:FIELD. Field counts exclude absence.
        /// min:amount/max:amount for Entry; min:note/max:note and
        /// min:priority/max:priority for Document. Extrema ignore absence.
        /// sum:amount for Entry; sum:priority/sum:score for Document (128-bit results).
        /// Prefix extrema with category, for Entry or entry_id, for Document
        /// to group results; all-absent groups display an absent value.
        #[arg(long, default_value = "full")]
        select: String,
        /// Deduplicate a leading-index projection; filters must be constant per tuple.
        #[arg(long)]
        distinct: bool,
        /// Keep completed Nat64 aggregate values at least VALUE (grouped terminals only).
        #[arg(long)]
        group_min: Option<u64>,
        /// Order completed groups by key or aggregate value; ties use ascending keys.
        #[arg(long, value_parser=["key", "asc", "desc"])]
        group_order: Option<String>,
    },
    /// Display command help.
    Help,
    /// Leave the console.
    #[command(alias = "exit")]
    Quit,
}
#[derive(Clone, Debug)]
struct CursorArg(Vec<u8>);
fn parse_cursor(text: &str) -> Result<CursorArg, String> {
    if !(70..=1092).contains(&text.len())
        || !text.len().is_multiple_of(2)
        || !text.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("expected a hexadecimal query cursor (70–1092 digits), not a row ID".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map(CursorArg)
}
fn help() {
    Cli::command().print_help().unwrap();
    println!();
}
type Status = ([u8; 32], [u8; 16], u64, u64);
type FieldView = (String, String, bool, String, Option<(String, String)>);
type IndexView = (String, Vec<String>, bool, bool);
type EntityView = (String, u32, String, Vec<FieldView>, Vec<IndexView>);

type TypeView = (String, String, Vec<(String, String, bool, String)>);

fn print_schema(entities: Vec<EntityView>, types: Vec<TypeView>) {
    println!(
        "\nSchema: {} {}\n",
        entities.len(),
        if entities.len() == 1 {
            "entity"
        } else {
            "entities"
        }
    );
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
            .map(|(name, kind, optional, info, reference)| {
                let key = if name == primary { "Primary" } else { "-" };
                let mut row = vec![
                    name,
                    kind,
                    if optional { "Yes" } else { "No" }.into(),
                    key.into(),
                    if info.is_empty() { "-".into() } else { info },
                ];
                if references {
                    row.push(reference.map_or_else(
                        || "-".into(),
                        |(target, index)| format!("{target} (via {index})"),
                    ));
                }
                row
            })
            .collect();
        let mut columns = vec!["Field", "Type", "Optional", "Key", "Info"];
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
    for (name, kind, members) in types {
        println!("{}: {}", terminal_text(&kind), terminal_text(&name));
        let rows: Vec<Vec<String>> = members
            .into_iter()
            .map(|(name, ty, optional, info)| {
                vec![
                    terminal_text(&name).into_owned(),
                    terminal_text(&ty).into_owned(),
                    if optional { "Yes" } else { "No" }.into(),
                    if info.is_empty() { "-".into() } else { info },
                ]
            })
            .collect();
        println!(
            "{}\n",
            render_bordered(
                &[
                    match kind.as_str() {
                        "Enum" => "Variant",
                        "Set" => "Item",
                        "Map" => "Role",
                        _ => "Field",
                    },
                    if kind == "Enum" { "Payload" } else { "Type" },
                    "Optional",
                    "Info"
                ],
                &rows
            )
        );
    }
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
    let interactive = io::stdin().is_terminal() && cli.command.is_none();
    let execute = async |command: Command| -> Result<(), String> {
        match command {
            Command::Help => help(),
            Command::Quit => {}
            Command::Schema { .. } | Command::Status => {
                let (id, selected) = match command {
                    Command::Schema { entity } => (2, entity),
                    _ => (1, None),
                };
                let endpoint = if id == 2 { "schema" } else { "inspect" };
                let request = if id == 2 {
                    native::encode_request(id, &selected, LIMITS)
                } else {
                    native::encode_request(id, &(), LIMITS)
                }
                .map_err(|e| e.to_string())?;
                let reply = pic
                    .query_call(canister, canister, endpoint, request)
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                if id == 2 {
                    let (_, entities, types): (Status, Vec<EntityView>, Vec<TypeView>) =
                        native::decode_response(id, &reply, LIMITS).map_err(|e| e.to_string())?;
                    if entities.is_empty()
                        && let Some(name) = selected
                    {
                        return Err(format!(
                            "Unknown entity '{}'. Use db schema to list entities.",
                            terminal_text(&name)
                        ));
                    }
                    print_schema(entities, types);
                } else {
                    print_status(
                        native::decode_response(id, &reply, LIMITS).map_err(|e| e.to_string())?,
                    );
                }
            }
            Command::Query {
                entity,
                after,
                limit,
                minimum,
                category,
                entry_id,
                order_by,
                desc,
                work,
                bytes,
                select,
                distinct,
                group_min,
                group_order,
            } => {
                let reply = pic
                    .query_call(
                        canister,
                        canister,
                        "query",
                        native::encode_request(
                            3,
                            &(
                                entity.as_str(),
                                after.map(|cursor| cursor.0),
                                limit,
                                minimum,
                                category,
                                entry_id,
                                order_by,
                                desc,
                                work,
                                bytes,
                                select,
                                (distinct, group_min, group_order),
                            ),
                            LIMITS,
                        )
                        .map_err(|e| e.to_string())?,
                    )
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                type Page = (
                    Vec<String>,
                    Vec<Vec<String>>,
                    Option<Vec<u8>>,
                    u32,
                    u32,
                    u32,
                );
                let result: Result<Page, String> =
                    native::decode_response(3, &reply, LIMITS).map_err(|e| e.to_string())?;
                let (columns, rows, next, examined, fetched, bytes) = result?;
                let count = rows.len();
                let rows: Vec<Vec<String>> = rows
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|v| terminal_text(&v).into_owned())
                            .collect()
                    })
                    .collect();
                let columns: Vec<_> = columns.iter().map(String::as_str).collect();
                println!("\n{entity}\n{}", render_bordered(&columns, &rows));
                let noun = if count == 1 { "row" } else { "rows" };
                println!(
                    "{count} {noun}; {examined} examined; {fetched} fetched; {bytes} encoded bytes."
                );
                println!(
                    "Next cursor: {}",
                    next.map_or_else(
                        || "none".into(),
                        |c| c.iter().map(|b| format!("{b:02x}")).collect::<String>()
                    )
                );
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
