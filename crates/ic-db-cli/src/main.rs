#![forbid(unsafe_code)]
//! Local product-record console.

mod commands;
mod terminal;
#[cfg(test)]
mod tests;

use clap::{CommandFactory, Parser, Subcommand};
use ic_db_cli::RowFilters;
use ic_db_cli::host::{LIMITS, instance};
use ic_host_text::{bordered_table, escape};
use ic_testkit::pocket_ic;
use ic_wire::{Direction, Encoding};

use std::{
    io::{self, BufRead, IsTerminal, Write},
    time::Instant,
};

// Payload allowance for the local fixture transport, shared by query and aggregate.
const FIXTURE_OUTPUT_BYTES: u32 = 4096;

#[derive(Clone, Copy)]
enum CallKind {
    Query,
    Update,
}

fn charged_cycles(before: u128, after: u128) -> String {
    match before.checked_sub(after) {
        Some(cycles) if cycles >= 1_000_000 => {
            // Round to one decimal without converting the exact balance to f64.
            let tenths = cycles / 100_000 + u128::from(cycles % 100_000 >= 50_000);

            format!(
                "{}.{:01}M cycles charged ({cycles})",
                tenths / 10,
                tenths % 10
            )
        }
        Some(cycles) => format!("{cycles} cycles charged"),
        None => format!(
            "cycle charge unavailable (balance increased by {})",
            after - before
        ),
    }
}

struct FixtureCalls<'a> {
    pic: &'a pocket_ic::nonblocking::PocketIc,
    canister: pocket_ic::CanisterId,
    statistics: Vec<String>,
}

impl FixtureCalls<'_> {
    async fn invoke(
        &mut self,
        kind: CallKind,
        method: &str,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, pocket_ic::RejectResponse> {
        let before = self.pic.cycle_balance(self.canister).await;
        let started = Instant::now();
        let result = match kind {
            CallKind::Query => {
                self.pic
                    .query_call(self.canister, self.canister, method, payload)
                    .await
            }
            CallKind::Update => {
                self.pic
                    .update_call(self.canister, self.canister, method, payload)
                    .await
            }
        };
        let elapsed = started.elapsed();
        let after = self.pic.cycle_balance(self.canister).await;
        let kind = match kind {
            CallKind::Query => "query",
            CallKind::Update => "update",
        };
        let statistics = format!(
            "{kind} call ({method}): {:.2} sec, {}",
            elapsed.as_secs_f64(),
            charged_cycles(before, after)
        );

        self.statistics.push(statistics);
        result
    }

    async fn framed<T: serde::de::DeserializeOwned>(
        &mut self,
        kind: CallKind,
        method: &str,
        id: u64,
        request: &(impl serde::Serialize + ?Sized),
    ) -> Result<T, String> {
        let frame = |direction| Encoding::Frame {
            method: id,
            direction,
        };
        let payload = ic_wire::encode(request, frame(Direction::Request), LIMITS)
            .map_err(|e| e.to_string())?;
        let reply = self
            .invoke(kind, method, payload)
            .await
            .map_err(|e| format!("{e:?}"))?;
        ic_wire::decode(&reply, frame(Direction::Response), LIMITS).map_err(|e| e.to_string())
    }
}

#[derive(Parser)]
#[command(
    name = "db",
    about = "Inspect schemas and query a local IC DB fixture",
    styles = help_styles(),
    color = help_color(),
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

fn help_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;

    clap::builder::Styles::styled()
        .header(AnsiColor::Cyan.on_default().bold())
        .usage(AnsiColor::Cyan.on_default().bold())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Yellow.on_default())
}

fn help_color() -> clap::ColorChoice {
    // The foreground terminal is relayed through pipes by the simulator launcher.
    if std::env::var_os("IC_DB_TERMINAL_INPUT").as_deref() == Some(std::ffi::OsStr::new("1"))
        && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
        && std::env::var_os("TERM").as_deref() != Some(std::ffi::OsStr::new("dumb"))
    {
        clap::ColorChoice::Always
    } else {
        clap::ColorChoice::Auto
    }
}

fn parse_console_line(line: &str) -> Result<Cli, clap::Error> {
    // Only split argument words: no shell execution, expansion or SQL parsing.
    let words = shlex::split(line.trim_end_matches(['\r', '\n'])).ok_or_else(|| {
        Cli::command().error(
            clap::error::ErrorKind::InvalidValue,
            "unterminated quote or trailing escape",
        )
    })?;

    Cli::try_parse_from(std::iter::once("db".to_owned()).chain(words))
}

#[derive(Subcommand)]
#[command(mut_subcommands = |command| command.display_order(0))]
enum Command {
    /// List entity names and column counts in the current local database.
    Entities {
        /// Emit a JSON array with name and columns for each entity.
        #[arg(long)]
        json: bool,
    },
    /// Inspect generated fields, types, indexes and admitted schema identity.
    #[command(
        after_help = "Optional fields can be absent. Text limits count UTF-8 bytes; Blob limits count raw bytes.\nPresent references require an existing target and restrict its deletion; absent references have no target.\nUse db status for stored schema admission and storage details."
    )]
    Schema {
        /// Exact declared entity name; omit to inspect the whole schema.
        entity: Option<String>,
        /// Emit JSON display data. A single invocation writes only JSON to stdout.
        /// This describes the local fixture; it is not a schema import format.
        #[arg(long)]
        json: bool,
    },
    /// Inspect the manifest identity, entity revision and physical allocation.
    Status,
    /// Read the exact maintained whole-entity row count without scanning rows.
    #[command(
        after_help = "Uses atomic entity metadata; it does not validate every row or index.\nFor filtered counts, use db query ENTITY --select count.\nSupports every entity listed by db entities."
    )]
    Count {
        /// Exact declared entity name; use db entities to list names.
        entity: String,
    },
    /// Run a fixed migration with a real local canister upgrade during copying.
    #[command(
        after_help = "Creates a separate six-row Item fixture, widens amount from Nat64 to Nat128 and adds an indexed label.\nCopies two rows per call, upgrades the local canister, resumes, validates, publishes and cleans the retired source.\nRuns once per console session. All data is temporary; this does not connect to or migrate a deployed database."
    )]
    MigrationDemo,
    /// Run a retained aggregate, displaying progress and each completed result page.
    #[command(
        after_help = "Runs bounded update calls against this temporary fixture, then cancels the job. No partial aggregate is presented as a result.\n--min and --category filter Entry rows; --entry-id filters Document rows. --group-min, --group-max, --group-order and --limit select completed groups. --limit never truncates input.\n--work bounds input candidates per call; --page-size bounds completed groups per response. The fixture and jobs are discarded on exit."
    )]
    Aggregate {
        #[arg(value_parser=["Entry", "Document", "DecimalRow", "BinaryObject"])]
        entity: String,
        /// Entry: distinct:amount pages distinct values with --page-size; --group-order first accepts --order-by and --desc;
        /// count/count:amount, distinct-count:amount or distinct-count:category, and min/max/sum/avg:amount;
        /// Entry supports category,summary:amount; DecimalRow supports zone,summary:value and zone,summary:maybe.
        /// For single measures, prefix with category, for Entry groups. Document: count/count:note/count:priority,
        /// distinct-count:note, min/max:note, min/max:priority, sum:priority/sum:score;
        /// prefix with entry_id, for groups. DecimalRow: count, count/distinct-count/min/max/sum/avg
        /// of :value or :maybe; prefix with zone, for groups. Unknown entity/selector pairs refuse.
        /// BinaryObject: count/count:payload/count:optional, distinct-count:payload or :optional,
        /// min/max:payload or :optional; payload/optional,count or ,count:optional.
        /// Blob payload bounds filter input before accumulation.
        /// DISTINCT counts include absence; count:optional counts only present values.
        #[arg(long, default_value = "count")]
        select: String,
        #[command(flatten)]
        filters: FilterArgs,
        /// Entry.amount >= VALUE.
        #[arg(long = "min")]
        minimum: Option<u64>,
        /// Entry.category == VALUE.
        #[arg(long)]
        category: Option<u64>,
        /// Document.entry_id == VALUE.
        #[arg(long)]
        entry_id: Option<u64>,
        /// Maximum completed groups across all pages, not an input-row limit.
        #[arg(long, default_value_t=32, value_parser=clap::value_parser!(u32).range(1..=32))]
        limit: u32,
        /// For grouped summaries, prefix each bound with count:, sum: or avg:.
        /// Keep completed aggregate values >= VALUE, parsed as the exact result type.
        /// Signed sums accept negatives; Decimal results accept fractions at their scale.
        /// Note min/max bounds are literal text in UTF-8 byte order, without normalization.
        /// Integer results reject fractions; threshold text is bounded to 64 bytes.
        #[arg(long, allow_hyphen_values = true)]
        group_min: Option<String>,
        /// Keep completed aggregate values <= VALUE; same exact types as --group-min.
        /// Both bounds are inclusive and apply before ordering or output limits.
        #[arg(long, allow_hyphen_values = true)]
        group_max: Option<String>,
        /// For grouped summaries use count:asc, sum:desc, avg:desc or the opposite direction.
        /// Key/key-desc order group keys; asc/desc order aggregate values with ascending key ties.
        #[arg(long, value_parser=["key", "key-desc", "first", "asc", "desc", "count:asc", "count:desc", "sum:asc", "sum:desc", "avg:asc", "avg:desc"])]
        group_order: Option<String>,
        #[command(flatten)]
        order: OrderArgs,
        #[arg(long, default_value_t=2, value_parser=clap::value_parser!(u32).range(1..=512))]
        work: u32,
        #[arg(long, default_value_t=2, value_parser=clap::value_parser!(u32).range(1..=32))]
        page_size: u32,
    },
    /// Read one fluent page from a local fixture entity.
    #[command(
        after_help = "Order defaults to the primary key. --order-by prefers a complete ordered path; otherwise it scans all matches and retains the best requested rows (at most 32, 128 KiB retained buffers). --desc reverses every component.\nComplete sorting applies --limit after scanning; work exhaustion refuses. It has no cursor. Sorted DISTINCT requires an identity result or all varying projected fields first in --order-by (ignore fields fixed by a mandatory literal equality); the sorter keeps the best --limit distinct results. Duplicates consume scan work.\nUse the returned cursor with --after and the same entity, filters, order, projection and DISTINCT setting. An empty page may still continue.\n--limit counts returned rows; --work counts examined candidates, including duplicates.\n--select id works for all four query entities; id,amount and category work for Entry. Filters use the fields' exact declared types.\nCategory DISTINCT uses live pages with --order-by category even when individual rows in a category fail the filters. Without an explicit sort or an eligible live path, it collects one complete result in first-match order: at most 32 values, no --after, and no partial output when work, state, row or output bounds cannot fit.\n--select exists stops at the first validated match; false requires a proven end of input. --select count counts matching rows; category,count groups Entry rows. Aggregates must finish within --work, refuse --after, --distinct and input ordering, and never return partial aggregates. --limit selects completed groups, not input rows. Extrema return no rows when no present value exists."
    )]
    Query {
        #[arg(default_value="Entry", value_parser=["Entry", "Document", "DecimalRow", "BinaryObject"])]
        entity: String,
        #[command(flatten)]
        filters: FilterArgs,
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
        #[command(flatten)]
        order: OrderArgs,
        #[arg(long, default_value_t=128, value_parser=clap::value_parser!(u32).range(1..=512))]
        work: u32,
        /// full, id, id,amount, category, count, count:FIELD,
        /// category,count or category,count:FIELD. Field counts exclude absence.
        /// Entry supports distinct-count:amount or distinct-count:category, optionally grouped by category.
        /// Document supports distinct-count:note, optionally grouped by entry_id. Distinct tuples include absence.
        /// title or note projects a single Document text field.
        /// exists for any entity: one boolean, stopping at the first match.
        /// min:amount/max:amount for Entry; min:note/max:note and
        /// min:priority/max:priority for Document. Extrema ignore absence.
        /// sum:amount for Entry; sum:priority/sum:score for Document (128-bit results).
        /// category,summary:amount and zone,summary:value or zone,summary:maybe return count, sum and average.
        /// value/maybe project DecimalRow fields, optionally with --distinct.
        /// DecimalRow supports count/distinct-count/min/max/sum:value or :maybe,
        /// optionally grouped by zone. DISTINCT includes one absent value.
        /// avg:amount/category,avg:amount for Entry; avg:value/avg:maybe with optional
        /// zone, grouping for DecimalRow. Averages use Decimal scale 2, ties to even.
        /// Prefix sums/extrema with category, for Entry or entry_id, for Document
        /// to group results; all-absent groups display an absent value.
        /// BinaryObject: payload/optional projections, min/max:payload or :optional,
        /// distinct-count:payload or :optional; payload/optional,count or ,count:optional.
        /// Blob output uses lowercase 0x-prefixed hex; absence displays as —.
        /// DISTINCT counts include absence; count:optional counts only present values.
        #[arg(long, default_value = "full")]
        select: String,
        /// Deduplicate projections using live indexed pages or a bounded complete result.
        #[arg(long)]
        distinct: bool,
        /// For grouped summaries, prefix each bound with count:, sum: or avg:.
        /// Keep completed aggregate values >= VALUE, parsed as the exact result type.
        /// Signed sums accept negatives; Decimal results accept fractions at their scale.
        /// Note min/max bounds are literal text in UTF-8 byte order, without normalization.
        /// Integer results reject fractions; threshold text is bounded to 64 bytes.
        #[arg(long, allow_hyphen_values = true)]
        group_min: Option<String>,
        /// Keep completed aggregate values <= VALUE; same exact types as --group-min.
        /// Both bounds are inclusive and apply before ordering or output limits.
        #[arg(long, allow_hyphen_values = true)]
        group_max: Option<String>,
        /// For grouped summaries use count:asc, sum:desc, avg:desc or the opposite direction.
        /// Key/key-desc order group keys; asc/desc order aggregate values with ascending key ties.
        #[arg(long, value_parser=["key", "key-desc", "asc", "desc", "count:asc", "count:desc", "sum:asc", "sum:desc", "avg:asc", "avg:desc"])]
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
#[derive(clap::Args, Debug)]
struct OrderArgs {
    /// Comma-separated fields with optional :asc or :desc; primary key breaks ties.
    /// Unspecified directions and the implicit primary key ascend; --desc reverses all.
    #[arg(long, value_delimiter = ',', value_name = "FIELDS")]
    order_by: Vec<String>,
    #[arg(long)]
    desc: bool,
}

#[derive(clap::Args, Debug)]
struct FilterArgs {
    /// Match rows with a value in this optional field, including an empty value.
    #[arg(long, value_name = "FIELD")]
    present: Option<String>,
    /// Match rows whose optional field has no value.
    #[arg(long, value_name = "FIELD")]
    absent: Option<String>,
    /// Compare two fields using eq, ne, lt, le, gt or ge. Text and Blob also accept starts-with, ends-with and contains.
    #[arg(long, num_args=3, value_names=["LEFT", "OP", "RIGHT"], action=clap::ArgAction::Set)]
    compare: Vec<String>,
    /// Match a field at or between two other fields on the same row.
    #[arg(long, num_args=3, value_names=["FIELD", "LOWER_FIELD", "UPPER_FIELD"], action=clap::ArgAction::Set)]
    between_fields: Vec<String>,
    /// Match a field below the lower field or above the upper field on the same row.
    #[arg(long, num_args=3, value_names=["FIELD", "LOWER_FIELD", "UPPER_FIELD"], action=clap::ArgAction::Set)]
    not_between_fields: Vec<String>,
    /// Compare a field with literal text using eq, ne, lt, le, gt, ge, starts-with, ends-with or contains.
    #[arg(long, num_args=3, value_names=["FIELD", "OP", "VALUE"], action=clap::ArgAction::Set, allow_hyphen_values=true)]
    text: Vec<String>,
    /// Match a Boolean field against true or false.
    #[arg(long = "bool", num_args=2, value_names=["FIELD", "VALUE"], action=clap::ArgAction::Set)]
    boolean: Vec<String>,
    /// Compare UTF-8 bytes, Blob bytes or collection items using eq, ne, lt, le, gt or ge.
    #[arg(long, num_args=3, value_names=["FIELD", "OP", "N"], action=clap::ArgAction::Set)]
    length: Vec<String>,
    /// BinaryObject.payload >= HEX; use 0x00ff80, or 0x for empty bytes.
    /// Exactly two hex digits per byte, at most 1024 bytes; no text conversion.
    #[arg(long, value_name="HEX", value_parser=parse_blob)]
    payload_min: Option<BlobArg>,
    /// BinaryObject.payload <= HEX; same byte syntax as --payload-min.
    #[arg(long, value_name="HEX", value_parser=parse_blob)]
    payload_max: Option<BlobArg>,
    /// BinaryObject.payload starts with exact raw bytes; combines with range bounds.
    #[arg(long, value_name="HEX", value_parser=parse_blob)]
    payload_prefix: Option<BlobArg>,
    /// BinaryObject.payload ends with HEX; residual filtering consumes scan work.
    #[arg(long, value_name="HEX", value_parser=parse_blob)]
    payload_suffix: Option<BlobArg>,
    /// BinaryObject.payload contains HEX anywhere; 0x matches every present payload.
    #[arg(long, value_name="HEX", value_parser=parse_blob)]
    payload_contains: Option<BlobArg>,
}
impl FilterArgs {
    fn into_filters(self) -> Result<RowFilters, String> {
        fn operands<const N: usize>(args: Vec<String>) -> Result<Option<[String; N]>, String> {
            if args.is_empty() {
                return Ok(None);
            }

            args.try_into()
                .map(Some)
                .map_err(|_| format!("expected {N} filter arguments"))
        }
        let length = match operands(self.length)? {
            Some([field, op, n]) => Some((
                field,
                op,
                n.parse::<u32>()
                    .map_err(|_| "--length requires an unsigned 32-bit length")?,
            )),
            None => None,
        };
        let boolean = match operands(self.boolean)? {
            None => None,
            Some([field, value]) => Some((
                field,
                value
                    .parse::<bool>()
                    .map_err(|_| "--bool requires true or false")?,
            )),
        };

        Ok(RowFilters {
            payload: [
                self.payload_min,
                self.payload_max,
                self.payload_prefix,
                self.payload_suffix,
                self.payload_contains,
            ]
            .map(|arg| arg.map(|v| v.0)),
            compare: operands(self.compare)?,
            between_fields: operands(self.between_fields)?,
            not_between_fields: operands(self.not_between_fields)?,
            text: operands(self.text)?,
            boolean,
            length,
            present: self.present,
            absent: self.absent,
        })
    }
}
#[derive(Clone, Debug)]
struct BlobArg(Vec<u8>);
fn parse_blob(text: &str) -> Result<BlobArg, String> {
    let digits = text
        .strip_prefix("0x")
        .ok_or("Blob literals require a 0x prefix")?;
    hex_bytes(digits, 0..=2048)
        .map(BlobArg)
        .ok_or_else(|| "expected 0x followed by 0–1024 complete hexadecimal byte pairs".into())
}

fn parse_cursor(text: &str) -> Result<CursorArg, String> {
    hex_bytes(text, 70..=1092)
        .map(CursorArg)
        .ok_or_else(|| "expected a hexadecimal query cursor (70–1092 digits), not a row ID".into())
}

fn hex_bytes(digits: &str, length: std::ops::RangeInclusive<usize>) -> Option<Vec<u8>> {
    if !length.contains(&digits.len())
        || !digits.len().is_multiple_of(2)
        || !digits.bytes().all(|digit| digit.is_ascii_hexdigit())
    {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok())
        .collect()
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

fn print_schema_json(
    fingerprint: [u8; 32],
    entities: &[EntityView],
    types: &[TypeView],
) -> Result<(), String> {
    use serde_json::json;
    let entities: Vec<_> = entities
        .iter()
        .map(|(name, _, primary, fields, indexes)| {
            let fields: Vec<_> = fields
                .iter()
                .map(|(name, kind, optional, info, reference)| {
                    json!({"name": name, "type": kind, "optional": optional, "info": info,
                    "reference": reference.as_ref().map(|(target, index)|
                        json!({"target": target, "index": index}))})
                })
                .collect();
            let indexes: Vec<_> = indexes
                .iter()
                .map(|(name, fields, unique, present_only)| {
                    json!({"name": name, "fields": fields, "unique": unique,
                    "present_only": present_only})
                })
                .collect();

            json!({"name": name, "primary_key": primary, "fields": fields, "indexes": indexes})
        })
        .collect();
    let types: Vec<_> = types
        .iter()
        .map(|(name, kind, members)| {
            let members: Vec<_> = members.iter().map(|(name, kind, optional, info)| {
            json!({"name": name, "type": kind, "optional": optional, "info": info})
        }).collect();

            json!({"name": name, "kind": kind, "members": members})
        })
        .collect();
    let document = json!({
        "source": "local_fixture",
        "schema_fingerprint": fingerprint.map(|b| format!("{b:02x}")).concat(),
        "entities": entities,
        "types": types,
    });
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &document).map_err(|e| e.to_string())?;

    writeln!(output).map_err(|e| e.to_string())
}

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
        println!("{}", escape(&name));
        println!(
            "Primary key: {} (supplied by the application)\n",
            escape(&primary)
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
                    row.extend(match reference {
                        Some((target, index)) => {
                            [target, format!("({})", index.replace(',', ", "))]
                        }
                        None => ["-".into(), "-".into()],
                    });
                }

                row
            })
            .collect();
        let mut columns = vec!["Field", "Type", "Optional", "Key", "Info"];
        if references {
            columns.extend(["References", "Reference index"]);
        }
        println!("{}", bordered_table(&columns, &rows));

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
                bordered_table(
                    &["Name", "Fields (in order)", "Unique", "Membership"],
                    &rows
                )
            );
        }
    }
    for (name, kind, members) in types {
        println!("{}: {}", escape(&kind), escape(&name));
        let rows: Vec<Vec<String>> = members
            .into_iter()
            .map(|(name, ty, optional, info)| {
                vec![
                    name,
                    ty,
                    if optional { "Yes" } else { "No" }.into(),
                    if info.is_empty() { "-".into() } else { info },
                ]
            })
            .collect();
        println!(
            "{}\n",
            bordered_table(
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
        vec!["Format".into(), "ICDBMF02".into()],
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
            "Wasm linear memory".into(),
            format!("{pages} pages (64 KiB each)"),
        ],
    ];
    println!("\n{}", bordered_table(&["Property", "Value"], &rows));
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Help)) {
        help();

        return Ok(());
    }
    if matches!(cli.command, Some(Command::Quit)) {
        return Ok(());
    }
    if std::env::var_os("IC_TESTKIT_POCKET_IC_URL").is_none() {
        // The prepared launcher owns the server. Re-enter this binary with its
        // explicit endpoint; the async driver never discovers/spawns a server.
        let runner =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/with-pocketic.sh");
        if !runner.is_file() {
            return Err("set IC_TESTKIT_POCKET_IC_URL to a prepared PocketIC server or install from the matching checkout with make db".into());
        }
        let mut launcher = std::process::Command::new("bash");
        launcher.arg(runner);
        if cli.command.is_none() {
            launcher.arg("--console");
        }
        launcher
            .arg(std::env::current_exe().map_err(|e| e.to_string())?)
            .args(std::env::args_os().skip(1));
        let interactive = io::stdin().is_terminal() && cli.command.is_none();
        let mut editor = interactive
            .then(rustyline::DefaultEditor::new)
            .transpose()
            .map_err(|e| e.to_string())?;
        if interactive {
            // Testkit owns a separate child process group. Only this foreground
            // process may read the terminal; the console receives a pipe instead.
            launcher
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .env("IC_DB_TERMINAL_INPUT", "1");
        }
        let mut child = launcher
            .spawn()
            .map_err(|e| format!("cannot run prepared simulator launcher: {e}"))?;
        let input_result = editor
            .as_mut()
            .map_or(Ok(()), |editor| terminal::relay(&mut child, editor));
        let status = child
            .wait()
            .map_err(|e| format!("cannot wait for simulator launcher: {e}"))?;
        input_result?;

        return if status.success() {
            Ok(())
        } else {
            Err(format!("db session failed: {status}"))
        };
    }

    tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(run(cli))
}

async fn run(cli: Cli) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let prefix = executable
        .parent()
        .and_then(|bin| bin.parent())
        .ok_or("cannot locate installation prefix; reinstall with make db")?;
    let fixture = prefix.join("share/ic-db/m1/records.wasm");
    let wasm = std::fs::read(&fixture)
        .map_err(|e| format!("{}: {e}; reinstall with make db", fixture.display()))?;
    let owned = instance().await?;
    let outcome = session(cli, &owned, wasm).await;
    owned.drop().await;

    outcome
}

async fn session(
    cli: Cli,
    pic: &pocket_ic::nonblocking::PocketIc,
    wasm: Vec<u8>,
) -> Result<(), String> {
    let canister = pic.create_canister().await;
    pic.add_cycles(canister, 2_000_000_000_000).await;
    pic.install_canister(canister, wasm.clone(), vec![], None)
        .await;
    // Private CLI sample data; ordinary Wasm probes supply their own decimal rows.
    let decimal_rows = vec![
        (1_u64, None::<String>, "1.25".to_owned()),
        (2, None, "2.50".to_owned()),
        (3, None, "-0.75".to_owned()),
    ];
    let reply = pic
        .update_call(
            canister,
            canister,
            "decimal_batch",
            ic_wire::encode(
                &(decimal_rows, false),
                Encoding::Frame {
                    method: 67,
                    direction: Direction::Request,
                },
                LIMITS,
            )
            .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    ic_wire::decode::<Result<(), u8>>(
        &reply,
        Encoding::Frame {
            method: 67,
            direction: Direction::Response,
        },
        LIMITS,
    )
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("decimal fixture setup failed: {e}"))?;
    if !matches!(
        cli.command,
        Some(Command::Schema { json: true, .. } | Command::Entities { json: true })
    ) {
        println!("IC DB — LOCAL M1 FIXTURE (fresh temporary database; discarded on exit)");
    }
    let interactive = cli.command.is_none()
        && (io::stdin().is_terminal()
            || std::env::var_os("IC_DB_TERMINAL_INPUT").as_deref()
                == Some(std::ffi::OsStr::new("1")));
    let execute = async |command: Command| -> Result<(), String> {
        let json = matches!(
            command,
            Command::Schema { json: true, .. } | Command::Entities { json: true }
        );
        let mut calls = FixtureCalls {
            pic,
            canister,
            statistics: Vec::new(),
        };
        let outcome = async {
        match command {
            Command::Help => help(),
            Command::Quit => {}
            Command::MigrationDemo => commands::migration(&mut calls, &wasm).await?,
            Command::Aggregate {
                entity,
                select,
                filters,
                minimum,
                category,
                entry_id,
                limit,
                group_min,
                group_max,
                group_order,
                order,
                work,
                page_size,
            } => {
                let selection: ic_db_cli::RetainedSelection = (
                    entity,
                    select,
                    (minimum, category, entry_id),
                    limit,
                    (group_min, group_max),
                    group_order,
                    filters.into_filters()?,
                    (order.order_by, order.desc),
                );
                let handle = calls.framed::<Result<Vec<u8>, String>>(CallKind::Update, "aggregate_start", 120, &selection).await??;
                println!("\n{} — {}", selection.0, selection.1);
                let run = async {
                    let mut sequence = 0;
                    let mut after: Option<Vec<u8>> = None;
                    // At most six rows per entity and two groups. A finite host bound
                    // prevents transport/progress regressions from looping forever.
                    for call in 1..=32 {
                        type Output = (Vec<String>, Vec<Vec<String>>, Option<Vec<u8>>);
                        type Progress = (u64, String, u64, u64, u64, Output);
                        let result: Result<Progress, String> = calls.framed(CallKind::Update, "aggregate_resume", 121,
                            &(&selection, &handle, sequence, work, FIXTURE_OUTPUT_BYTES, page_size, &after)).await?;
                        let (saved, phase, examined, input_bytes, output_bytes, (columns, rows, next)) = result?;
                        println!("Call {call}: {phase}; sequence {saved}; {examined} examined; {input_bytes} input bytes; {output_bytes} output bytes.");
                        sequence = saved;

                        match phase.as_str() {
                            "Running" => {},
                            "Complete" => {
                                println!("{}", bordered_table(&columns, &rows));
                                if next.is_none() { return Ok(()); }
                                after = next;
                            }
                            "Expired" | "Invalidated" => return Err(format!("aggregate {phase}")),
                            _ => return Err("unsupported aggregate progress".into()),
                        }
                    }

                    Err("aggregate exceeded the fixture's 32-call bound".into())
                }.await;
                // Cleanup also runs after an output/resource refusal. No automatic
                // restart, lost cursor, or client-side aggregation is hidden here.
                let cleanup = calls.framed::<Result<(), String>>(CallKind::Update, "aggregate_cancel", 122, &handle)
                    .await.and_then(|result| result);

                match (run, cleanup) {
                    (Ok(()), Ok(())) => {}
                    (Err(error), Ok(())) | (Ok(()), Err(error)) => return Err(error),
                    (Err(error), Err(cleanup)) => {
                        return Err(format!("{error}; cleanup: {cleanup}"));
                    }
                }
            }
            Command::Count { entity } => commands::count(&mut calls, &entity).await?,
            Command::Schema { entity, json } => commands::schema(&mut calls, entity, false, json).await?,
            Command::Entities { json } => commands::schema(&mut calls, None, true, json).await?,
            Command::Status => print_status(calls.framed(CallKind::Query, "inspect", 1, &()).await?),
            Command::Query {
                entity,
                filters,
                after,
                limit,
                minimum,
                category,
                entry_id,
                order: OrderArgs { order_by, desc },
                work,
                select,
                distinct,
                group_min,
                group_max,
                group_order,
            } => {
                let reply = calls.invoke(
                    CallKind::Query,
                    "query",
                    ic_wire::encode(
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
                            FIXTURE_OUTPUT_BYTES,
                            select,
                            (distinct, (group_min, group_max), group_order,
                             filters.into_filters()?),
                        ),
                        Encoding::Frame {
                            method: 3,
                            direction: Direction::Request,
                        },
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
                let result: Result<Page, String> = ic_wire::decode(
                    &reply,
                    Encoding::Frame {
                        method: 3,
                        direction: Direction::Response,
                    },
                    LIMITS,
                )
                .map_err(|e| e.to_string())?;
                let (columns, rows, next, examined, fetched, bytes) = result?;
                let count = rows.len();
                println!("\n{entity}\n{}", bordered_table(&columns, &rows));
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
        }.await;
        for line in calls.statistics {
            if json || outcome.is_err() {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
        }

        outcome
    };

    if let Some(command) = cli.command {
        execute(command).await
    } else {
        let mut editor = io::stdin()
            .is_terminal()
            .then(rustyline::DefaultEditor::new)
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut input = io::stdin().lock();

        loop {
            let line = if let Some(editor) = editor.as_mut() {
                let Some(line) = terminal::readline(editor)? else {
                    break Ok(());
                };

                line
            } else {
                if interactive {
                    terminal::request_input().map_err(|e| e.to_string())?;
                }
                let mut line = String::new();
                if std::io::Read::take(&mut input, 4097)
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?
                    == 0
                {
                    break Ok(());
                }

                line
            };
            if line.len() > 4096 {
                break Err("command exceeds 4096 bytes".into());
            }
            if line.trim().is_empty() {
                continue;
            }

            // Clap owns the same grammar and validation as argv.
            match parse_console_line(&line) {
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
    }
}
