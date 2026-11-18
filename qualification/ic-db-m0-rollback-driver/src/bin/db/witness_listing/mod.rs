//! Explicit native binding to canic-db-witness.query.v1.
//! These DTOs describe that application's Wire contract, not an IC DB query API.
//! Importing its witness package would enable db-schema and an unnecessary engine.
use super::{CursorArg, parse_cursor, remote};
use canic::db::RPC_LIMITS;
use clap::Args;
use ic_host_text::bordered_table;
use ic_wire::{Direction, Encoding};
use serde::{Deserialize, Serialize};

const QUERY: u64 = ic_wire::method_id("canic-db-witness.query.v1");

#[derive(Args)]
#[command(
    after_help = "This command only supports Canic's qualification application: Item rows in ID order, filtered by minimum score. It is not a general application query protocol. Each invocation reads one bounded page. Empty rows can still have a next cursor; forward it with the same minimum score. No client filtering, automatic pagination or retry occurs."
)]
pub(super) struct Options {
    #[command(flatten)]
    target: remote::Target,
    /// Inclusive minimum Item.score.
    #[arg(long, default_value_t = 0)]
    minimum: u64,
    /// Maximum returned rows in this page (1–3).
    #[arg(long, default_value_t=3, value_parser=clap::value_parser!(u8).range(1..=3))]
    limit: u8,
    /// Maximum candidates examined by the application (1–4).
    #[arg(long, default_value_t=4, value_parser=clap::value_parser!(u8).range(1..=4))]
    work: u8,
    /// Opaque hexadecimal continuation; forwarded without decoding its contents.
    #[arg(long, value_parser=parse_cursor)]
    after: Option<CursorArg>,
    /// Emit one JSON page with a hexadecimal next cursor; statistics go to stderr.
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct Request<'a> {
    minimum: u64,
    limit: u8,
    work: u8,
    after: Option<&'a [u8]>,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Row {
    id: u64,
    score: u64,
}
#[derive(Debug, Deserialize)]
struct Page {
    rows: Vec<Row>,
    next: Option<Vec<u8>>,
    examined: u32,
}
#[derive(Debug, Deserialize)]
enum Refusal {
    Limit,
    Exists,
    DuplicateTarget,
    Maintenance,
    Cursor,
}

fn decode(bytes: &[u8], limit: u8, work: u8) -> Result<Page, String> {
    let page = ic_wire::decode::<Result<Page, Refusal>>(
        bytes,
        Encoding::Frame {
            method: QUERY,
            direction: Direction::Response,
        },
        RPC_LIMITS,
    )
    .map_err(|e| format!("invalid witness listing v1 reply: {e}"))?
    .map_err(|e| format!("listing refused: {e:?}"))?;
    if page.rows.len() > usize::from(limit)
        || page.examined > u32::from(work)
        || page.rows.len() > page.examined as usize
        || page
            .next
            .as_ref()
            .is_some_and(|next| !(35..=546).contains(&next.len()))
    {
        return Err("listing reply exceeds the application's page bounds".into());
    }
    Ok(page)
}

pub(super) async fn run(options: &Options) -> Result<(), String> {
    let request = Request {
        minimum: options.minimum,
        limit: options.limit,
        work: options.work,
        after: options.after.as_ref().map(|v| v.0.as_slice()),
    };
    let payload = ic_wire::encode(
        &request,
        Encoding::Frame {
            method: QUERY,
            direction: Direction::Request,
        },
        RPC_LIMITS,
    )
    .map_err(|e| e.to_string())?;
    let connection =
        remote::Connection::connect(&options.target, "CANIC WITNESS LISTING", options.json).await?;
    let mut statistics = Vec::new();
    let outcome = connection
        .query("db_query", payload, &mut statistics)
        .await
        .and_then(|bytes| decode(&bytes, options.limit, options.work));
    if let Ok(page) = &outcome {
        let next = page
            .next
            .as_ref()
            .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
        if options.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&connection.envelope(
                    "page",
                    serde_json::json!({"rows":page.rows,"next":next,"examined":page.examined})
                ))
                .map_err(|e| e.to_string())?
            );
        } else {
            println!(
                "{}",
                bordered_table(
                    &["id", "score"],
                    &page
                        .rows
                        .iter()
                        .map(|r| vec![r.id.to_string(), r.score.to_string()])
                        .collect::<Vec<_>>()
                )
            );
            println!(
                "{} rows; {} examined.\nNext cursor: {}",
                page.rows.len(),
                page.examined,
                next.as_deref().unwrap_or("none")
            );
        }
    }
    remote::report(statistics, options.json || outcome.is_err());
    outcome.map(|_| ())
}

#[cfg(test)]
mod tests;
