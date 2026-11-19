//! Read-only Canic inspection. Applications retain query/write and exposure policy.
#[cfg(test)]
mod tests;

use canic::db::{RPC_LIMITS, inspection as protocol};
use canic_wire::{Direction, Encoding};
use clap::Args;
use ic_host_text::{bordered_table, escape};

#[derive(Args)]
pub(super) struct Options {
    #[command(flatten)]
    target: super::remote::Target,
    /// Selected logical entity; omit for the complete exposed catalogue.
    #[arg(long)]
    entity: Option<String>,
    /// Request maintained cardinality, subject to application permission and admission.
    #[arg(long, requires = "entity")]
    count: bool,
    /// Emit one JSON observation; call statistics go to stderr.
    #[arg(long)]
    json: bool,
}

fn decode(bytes: &[u8], request: &protocol::Request) -> Result<protocol::Response, String> {
    let response = canic_wire::decode::<protocol::Reply>(
        bytes,
        Encoding::Frame {
            method: protocol::INSPECT,
            direction: Direction::Response,
        },
        RPC_LIMITS,
    )
    .map_err(|e| format!("invalid Canic inspection v1 reply: {e}"))?
    .map_err(|e| format!("inspection refused: {e:?}"))?;
    match (request, &response.selection) {
        (protocol::Request::Catalogue, protocol::Selection::Catalogue(_)) => {}
        (
            protocol::Request::Entity {
                schema,
                name,
                count,
            },
            protocol::Selection::Entity(entity),
        ) if *schema == response.schema && *name == entity.name => {
            if matches!(entity.cardinality, protocol::Cardinality::Available(_))
                && (!count || !matches!(response.admission, protocol::Admission::Readable { .. }))
            {
                return Err("unexpected cardinality without readable admission and request".into());
            }
        }
        _ => {
            return Err(
                "inspection reply does not match the requested schema/entity selection".into(),
            );
        }
    }
    Ok(response)
}

async fn call(
    connection: &super::remote::Connection,
    request: &protocol::Request,
    statistics: &mut Vec<String>,
) -> Result<protocol::Response, String> {
    let payload = canic_wire::encode(
        request,
        Encoding::Frame {
            method: protocol::INSPECT,
            direction: Direction::Request,
        },
        RPC_LIMITS,
    )
    .map_err(|e| e.to_string())?;
    decode(
        &connection.query("db_inspect", payload, statistics).await?,
        request,
    )
}

pub(super) async fn run(options: &Options) -> Result<(), String> {
    let name = options
        .entity
        .as_ref()
        .map(|name| {
            protocol::Name::new(name.clone()).map_err(|_| "entity name exceeds inspection bounds")
        })
        .transpose()?;
    let connection =
        super::remote::Connection::connect(&options.target, "APPLICATION INSPECTION", options.json)
            .await?;
    let mut statistics = Vec::new();
    let outcome = async {
        let catalogue = call(&connection, &protocol::Request::Catalogue, &mut statistics).await?;
        if let Some(name) = name {
            let protocol::Selection::Catalogue(names) = &catalogue.selection else { unreachable!() };
            if !names.as_slice().contains(&name) { return Err("entity is not in the exposed catalogue".into()); }
            let selected = call(&connection, &protocol::Request::Entity { schema: catalogue.schema, name, count: options.count }, &mut statistics).await?;
            if selected.admission != catalogue.admission {
                return Err("database admission or incarnation changed during inspection; repeat the command".into());
            }
            Ok(selected)
        } else { Ok(catalogue) }
    }.await;
    if let Ok(response) = &outcome {
        if options.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&connection.envelope(
                    "inspection",
                    serde_json::to_value(response).map_err(|e| e.to_string())?
                ))
                .map_err(|e| e.to_string())?
            );
        } else {
            print!("{}", render(response));
        }
    }
    super::remote::report(statistics, options.json || outcome.is_err());
    outcome.map(|_| ())
}

fn kind(kind: &protocol::Kind) -> (String, String) {
    use protocol::Kind as K;
    match kind {
        K::Unsigned(bits) => (format!("Nat{bits}"), String::new()),
        K::Signed(bits) => (format!("Int{bits}"), String::new()),
        K::Float(bits) => (format!("Float{bits}"), String::new()),
        K::Text(bytes) => ("Text".into(), format!("max {bytes} UTF-8 bytes")),
        K::Blob(bytes) => ("Blob".into(), format!("max {bytes} bytes")),
        K::Decimal(scale) => ("Decimal".into(), format!("scale {scale}")),
        K::Named(name) => (escape(name.as_str()).into_owned(), String::new()),
        K::List { item, max_items } => {
            let (name, info) = self::kind(item);
            (
                format!("List<{name}>"),
                format!("max {max_items} items; {info}"),
            )
        }
        K::Bool
        | K::Principal
        | K::Account
        | K::Subaccount
        | K::Ulid
        | K::Date
        | K::Timestamp
        | K::Duration => (format!("{kind:?}"), String::new()),
    }
}

fn fields(fields: &[protocol::Field]) -> String {
    bordered_table(
        &[
            "Field",
            "Type",
            "Optional",
            "Info",
            "Reference target",
            "Reference index",
        ],
        &fields
            .iter()
            .map(|f| {
                let (name, info) = kind(&f.value.kind);
                vec![
                    escape(f.name.as_str()).into_owned(),
                    name,
                    f.value.optional.to_string(),
                    info,
                    f.reference
                        .as_ref()
                        .map_or_else(String::new, |r| escape(r.target.as_str()).into_owned()),
                    f.reference
                        .as_ref()
                        .map_or_else(String::new, |r| escape(r.index.as_str()).into_owned()),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

fn render(response: &protocol::Response) -> String {
    let (admission, incarnation) = match response.admission {
        protocol::Admission::Readable { incarnation } => {
            ("Readable", incarnation.map(|v| format!("{v:02x}")).concat())
        }
        protocol::Admission::Maintenance => ("Maintenance", "—".into()),
        protocol::Admission::Incompatible => ("Incompatible", "—".into()),
        protocol::Admission::Unavailable => ("Unavailable", "—".into()),
    };
    let mut out = format!(
        "{}\n",
        bordered_table(
            &["Property", "Value"],
            &[
                vec![
                    "Executable schema".into(),
                    response.schema.map(|v| format!("{v:02x}")).concat()
                ],
                vec!["Stored admission".into(), admission.into()],
                vec!["Incarnation".into(), incarnation],
            ]
        )
    );
    match &response.selection {
        protocol::Selection::Catalogue(names) => out.push_str(&bordered_table(
            &["Entity"],
            &names
                .as_slice()
                .iter()
                .map(|n| vec![escape(n.as_str()).into_owned()])
                .collect::<Vec<_>>(),
        )),
        protocol::Selection::Entity(entity) => {
            let cardinality = match entity.cardinality {
                protocol::Cardinality::Available(n) => n.to_string(),
                protocol::Cardinality::NotExposed => "Not exposed".into(),
                protocol::Cardinality::Unavailable => "Unavailable".into(),
            };
            out.push_str(&format!(
                "{}\n{}\n",
                bordered_table(
                    &["Entity", "Primary key", "Maintained count"],
                    &[vec![
                        escape(entity.name.as_str()).into_owned(),
                        escape(entity.primary_key.as_str()).into_owned(),
                        cardinality
                    ]]
                ),
                fields(entity.fields.as_slice())
            ));
            out.push_str(&bordered_table(
                &["Index", "Fields", "Unique", "Present only"],
                &entity
                    .indexes
                    .as_slice()
                    .iter()
                    .map(|i| {
                        vec![
                            escape(i.name.as_str()).into_owned(),
                            i.fields
                                .as_slice()
                                .iter()
                                .map(|f| escape(f.as_str()).into_owned())
                                .collect::<Vec<_>>()
                                .join(", "),
                            i.unique.to_string(),
                            i.present_only.to_string(),
                        ]
                    })
                    .collect::<Vec<_>>(),
            ));
            for definition in entity.types.as_slice().iter() {
                out.push_str(&format!(
                    "\n{}\n",
                    escape(definition.name.as_str()).into_owned()
                ));
                match &definition.body {
                    protocol::Body::Record(items) => out.push_str(&fields(items.as_slice())),
                    protocol::Body::Enum(items) => out.push_str(&bordered_table(
                        &["Variant", "Payload", "Optional", "Info"],
                        &items
                            .as_slice()
                            .iter()
                            .map(|v| {
                                let (name, info) = v
                                    .payload
                                    .as_ref()
                                    .map_or_else(|| ("—".into(), String::new()), |p| kind(&p.kind));
                                vec![
                                    escape(v.name.as_str()).into_owned(),
                                    name,
                                    v.payload.as_ref().is_some_and(|p| p.optional).to_string(),
                                    info,
                                ]
                            })
                            .collect::<Vec<_>>(),
                    )),
                    protocol::Body::Set { item, max_items } => {
                        let (name, info) = kind(item);
                        out.push_str(&bordered_table(
                            &["Item", "Max items", "Info"],
                            &[vec![name, max_items.to_string(), info]],
                        ));
                    }
                    protocol::Body::Map {
                        key,
                        value,
                        max_items,
                    } => {
                        let (k, ki) = kind(key);
                        let (v, vi) = kind(&value.kind);
                        out.push_str(&bordered_table(
                            &["Key", "Value", "Optional value", "Max items", "Info"],
                            &[vec![
                                k,
                                v,
                                value.optional.to_string(),
                                max_items.to_string(),
                                format!("key: {ki}; value: {vi}"),
                            ]],
                        ));
                    }
                }
            }
        }
    }
    out.push('\n');
    out
}
