//! Local fixture commands using typed Wire replies.

use super::{CallKind, FixtureCalls, print_schema, print_schema_json};
use ic_host_text::{bordered_table, escape};

pub(super) async fn count(calls: &mut FixtureCalls<'_>, entity: &str) -> Result<(), String> {
    let count: Result<u64, String> = calls
        .framed(CallKind::Query, "cardinality", 148, entity)
        .await?;
    println!(
        "{}",
        bordered_table(&["count"], &[vec![count?.to_string()]])
    );
    Ok(())
}

pub(super) async fn schema(
    calls: &mut FixtureCalls<'_>,
    selected: Option<String>,
    json: bool,
) -> Result<(), String> {
    let ic_db_cli::SchemaResponse {
        status,
        entities,
        types,
    } = fetch_schema(calls, &selected).await?;
    if entities.is_empty()
        && let Some(name) = selected
    {
        return Err(format!(
            "Unknown entity '{}'. Use db entities to list entities.",
            escape(&name)
        ));
    }

    if json {
        print_schema_json(status.schema, &entities, &types)?;
    } else {
        print_schema(entities, types);
    }

    Ok(())
}

async fn fetch_schema(
    calls: &mut FixtureCalls<'_>,
    selected: &Option<String>,
) -> Result<ic_db_cli::SchemaResponse, String> {
    calls.framed(CallKind::Query, "schema", 2, selected).await
}

pub(super) async fn entities(calls: &mut FixtureCalls<'_>, json: bool) -> Result<(), String> {
    let entities = fetch_schema(calls, &None).await?.entities;
    let mut summaries: Vec<_> = entities
        .into_iter()
        .map(|entity| (entity.name, entity.fields.len()))
        .collect();
    summaries.sort_by(|a, b| a.0.cmp(&b.0));

    if json {
        let rows: Vec<_> = summaries
            .into_iter()
            .map(|(name, columns)| serde_json::json!({"name": name, "columns": columns}))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?
        );
    } else {
        let rows: Vec<_> = summaries
            .into_iter()
            .map(|(name, columns)| vec![name, columns.to_string()])
            .collect();
        println!("{}", bordered_table(&["Entity", "Columns"], &rows));
    }
    Ok(())
}

pub(super) async fn migration(calls: &mut FixtureCalls<'_>, wasm: &[u8]) -> Result<(), String> {
    let pic = calls.pic;
    let canister = calls.canister;
    let mut step = async |action: u8| -> Result<(bool, Vec<Vec<String>>), String> {
        calls
            .framed(CallKind::Update, "migration_demo", 142, &action)
            .await?
    };
    let (_, rows) = step(0).await?;
    println!(
        "\nItem — source\n{}",
        bordered_table(&["id", "amount"], &rows)
    );
    println!("\nMigration: console-item-label v1");
    for (action, phase) in [
        (1, "Freeze"),
        (2, "Copy"),
        (3, "Validate"),
        (4, "Publish"),
        (5, "Cleanup"),
    ] {
        // Fixture-size bound only; product checkpoints own progress.
        for call in 1..=32 {
            let (complete, rows) = step(action).await?;
            println!(
                "{phase} {call}: {}",
                if complete { "complete" } else { "in progress" }
            );
            if action == 2 {
                println!("{}", bordered_table(&["Copied rows"], &rows));
                if call == 1 {
                    ic_db_cli::host::upgrade_stopped(pic, canister, wasm.to_vec(), vec![], None)
                        .await
                        .map_err(|e| format!("{e:?}"))?;
                    println!("Upgrade: complete");
                }
            }
            if complete {
                break;
            }
            if call == 32 {
                return Err(format!("{phase} exceeded the fixture's 32-call bound"));
            }
        }
    }
    let (_, rows) = step(6).await?;
    println!(
        "\nItem — target (label index)\n{}",
        bordered_table(&["id", "amount", "label"], &rows)
    );
    Ok(())
}
