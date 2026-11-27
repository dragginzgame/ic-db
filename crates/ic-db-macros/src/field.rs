//! Parse and project stored field types at the macro boundary.

use super::{literal, meta_items, once, text};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Lit, Meta};

pub(super) struct Integer {
    schema: &'static str,
    rust: &'static str,
    codec: &'static str,
    pub(super) signed: bool,
    pub(super) bits: u32,
}

macro_rules! integer {
    ($schema:ident, $rust:ty, $codec:ident) => {
        Integer {
            schema: stringify!($schema),
            rust: stringify!($rust),
            codec: stringify!($codec),
            signed: <$rust>::MIN != 0,
            bits: <$rust>::BITS,
        }
    };
}

const INTEGERS: &[Integer] = &[
    integer!(Nat8, u8, nat8),
    integer!(Nat16, u16, nat16),
    integer!(Nat32, u32, nat32),
    integer!(Nat64, u64, nat64),
    integer!(Nat128, u128, nat128),
    integer!(Int8, i8, int8),
    integer!(Int16, i16, int16),
    integer!(Int32, i32, int32),
    integer!(Int64, i64, int64),
    integer!(Int128, i128, int128),
];

pub(super) enum Kind {
    Integer(&'static Integer),
    Named(syn::Path, bool),
    Float32,
    Float64,
    Subaccount,
    Account,
    U256,
    Ulid,
    Date,
    Duration,
    Decimal(u32),
    Timestamp,
    Bool,
    Principal,
    Text(u32),
    Blob(u32),
}
pub(super) struct Field {
    pub managed: Option<Management>,
    pub default: Option<super::insert::Literal>,
    pub name: String,
    pub kind: Kind,
    pub optional: bool,
    pub many: Option<u32>,
    pub reference: Option<(syn::Path, Option<String>)>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Management {
    CreatedAt,
    UpdatedAt,
}
impl Management {
    pub fn description(self) -> TokenStream {
        match self {
            Self::CreatedAt => quote!(::ic_db::ManagedTimestamp::CreatedAt),
            Self::UpdatedAt => quote!(::ic_db::ManagedTimestamp::UpdatedAt),
        }
    }
}
impl Field {
    pub(super) fn nested(name: &str, kind: Kind, optional: bool, many: Option<u32>) -> Self {
        Self {
            name: name.into(),
            kind,
            optional,
            many,
            reference: None,
            default: None,
            managed: None,
        }
    }

    pub(super) fn resolve_self(&mut self, ident: &syn::Ident) {
        if let Kind::Named(path, _) = &mut self.kind
            && path.is_ident("Self")
        {
            *path = syn::parse_quote!(#ident);
        }
    }

    pub fn parse(meta: &Meta) -> syn::Result<Self> {
        if !meta.path().is_ident("field") {
            return Err(syn::Error::new_spanned(meta, "expected field(...)"));
        }
        let mut name = None;
        let mut value = None;
        let mut reference = None;
        let mut default = None;
        let mut managed = None;
        for part in meta_items(meta)? {
            if part.path().is_ident("name") {
                once(&mut name, text(&part)?, &part)?;
            } else if part.path().is_ident("value") {
                once(&mut value, parse_value(&part)?, &part)?;
            } else if part.path().is_ident("default") {
                once(&mut default, part.clone(), &part)?;
            } else if part.path().is_ident("managed") {
                let policy = match text(&part)?.as_str() {
                    "created_at" => Management::CreatedAt,
                    "updated_at" => Management::UpdatedAt,
                    _ => {
                        return Err(syn::Error::new_spanned(
                            &part,
                            "managed requires created_at or updated_at",
                        ));
                    }
                };
                once(&mut managed, policy, &part)?;
            } else if part.path().is_ident("references") {
                let mut entity = None;
                let mut index = None;
                for option in meta_items(&part)? {
                    if option.path().is_ident("entity") {
                        let target = syn::parse_str(&text(&option)?)?;
                        once(&mut entity, target, &option)?;
                    } else if option.path().is_ident("index") {
                        once(&mut index, text(&option)?, &option)?;
                    } else {
                        return Err(syn::Error::new_spanned(
                            option,
                            "references accepts an entity type path and optional index name",
                        ));
                    }
                }
                let target = (
                    entity.ok_or_else(|| {
                        syn::Error::new_spanned(&part, "references requires an entity type path")
                    })?,
                    index,
                );
                once(&mut reference, target, &part)?;
            } else {
                return Err(syn::Error::new_spanned(
                    part,
                    "unsupported field option; fields use names, not numeric IDs",
                ));
            }
        }
        let name = name.ok_or_else(|| syn::Error::new_spanned(meta, "field requires name"))?;
        let (kind, optional, many) =
            value.ok_or_else(|| syn::Error::new_spanned(meta, "field requires value"))?;
        if managed.is_some()
            && (!matches!(kind, Kind::Timestamp | Kind::Named(..))
                || optional
                || many.is_some()
                || default.is_some()
                || reference.is_some())
        {
            return Err(syn::Error::new_spanned(
                meta,
                "managed fields require a nonoptional scalar Timestamp without a default or reference",
            ));
        }
        if reference.is_some() && !kind.primary_key() {
            return Err(syn::Error::new_spanned(
                meta,
                "references currently require an integer, U256, Ulid, Principal, Account or Subaccount",
            ));
        }

        Ok(Self {
            managed,
            default: default
                .as_ref()
                .map(|meta| super::insert::Literal::parse(meta, &kind, many))
                .transpose()?,
            name,
            kind,
            optional,
            many,
            reference,
        })
    }

    pub fn required_key(&self) -> bool {
        self.kind.primary_key() && !self.optional && self.many.is_none()
    }

    pub fn indexable(&self) -> bool {
        self.many.is_none()
            && matches!(
                self.kind,
                Kind::Integer(_)
                    | Kind::Float32
                    | Kind::Float64
                    | Kind::Subaccount
                    | Kind::Account
                    | Kind::U256
                    | Kind::Ulid
                    | Kind::Date
                    | Kind::Decimal(_)
                    | Kind::Duration
                    | Kind::Timestamp
                    | Kind::Principal
                    | Kind::Bool
                    | Kind::Text(_)
                    | Kind::Blob(_)
                    | Kind::Named(_, _)
            )
    }

    pub fn index_value(&self, ident: &syn::Ident) -> TokenStream {
        self.index_operand(ident, self.optional)
    }

    pub fn index_operand(&self, ident: &syn::Ident, optional: bool) -> TokenStream {
        if let Kind::Named(_, indirect) = &self.kind {
            return match (optional, indirect) {
                (true, true) => {
                    quote!(row.#ident.as_deref().map(::ic_db::IndexValue::from).unwrap_or(::ic_db::IndexValue::Absent))
                }
                (true, false) => {
                    quote!(row.#ident.as_ref().map(::ic_db::IndexValue::from).unwrap_or(::ic_db::IndexValue::Absent))
                }
                (false, true) => quote!(::ic_db::IndexValue::from(row.#ident.as_ref())),
                (false, false) => quote!(::ic_db::IndexValue::from(&row.#ident)),
            };
        }

        if optional {
            if matches!(self.kind, Kind::Text(_) | Kind::Blob(_)) {
                quote!(row.#ident.as_deref().map(::ic_db::IndexValue::from).unwrap_or(::ic_db::IndexValue::Absent))
            } else {
                quote!(row.#ident.map(::ic_db::IndexValue::from).unwrap_or(::ic_db::IndexValue::Absent))
            }
        } else if matches!(self.kind, Kind::Text(_)) {
            quote!(::ic_db::IndexValue::from(row.#ident.as_str()))
        } else if matches!(self.kind, Kind::Blob(_)) {
            quote!(::ic_db::IndexValue::from(row.#ident.as_slice()))
        } else {
            quote!(::ic_db::IndexValue::from(row.#ident))
        }
    }

    pub fn rust_type(&self) -> TokenStream {
        let base = self.item_type();
        let base = if self.many.is_some() {
            quote!(::std::vec::Vec<#base>)
        } else {
            base
        };

        if self.optional {
            quote!(::std::option::Option<#base>)
        } else {
            base
        }
    }

    pub fn item_type(&self) -> TokenStream {
        match &self.kind {
            Kind::Named(path, indirect) => {
                if *indirect {
                    quote!(::std::boxed::Box<#path>)
                } else {
                    quote!(#path)
                }
            }
            Kind::Integer(integer) => {
                let name = quote::format_ident!("{}", integer.rust);
                quote!(#name)
            }
            Kind::Float32 => quote!(::ic_db::Float32),
            Kind::Float64 => quote!(::ic_db::Float64),
            Kind::Subaccount => quote!(::ic_db::Subaccount),
            Kind::Account => quote!(::ic_db::Account),
            Kind::U256 => quote!(::ic_db::U256),
            Kind::Ulid => quote!(::ic_db::Ulid),
            Kind::Date => quote!(::ic_db::Date),
            Kind::Decimal(_) => quote!(::ic_db::Decimal),
            Kind::Duration => quote!(::ic_db::Duration),
            Kind::Timestamp => quote!(::ic_db::Timestamp),
            Kind::Bool => quote!(bool),
            Kind::Principal => quote!(::ic_db::Principal),
            Kind::Text(_) => quote!(::std::string::String),
            Kind::Blob(_) => quote!(::ic_db::Blob),
        }
    }

    pub fn description(&self) -> TokenStream {
        let managed = self.managed.map_or_else(
            || quote!(None),
            |value| {
                let value = value.description();

                quote!(Some(#value))
            },
        );
        let name = &self.name;
        let optional = self.optional;
        let value = self.type_description();
        let default = self.default.as_ref().map_or_else(
            || quote!(None),
            |value| {
                let value = value.description();

                quote!(Some(&#value))
            },
        );
        let reference = match &self.reference {
            None => quote!(None),
            Some((entity, index)) => {
                let index = index.as_ref().expect("reverse index was resolved");

                quote!(Some(::ic_db::ReferenceSchema {
                    entity: ::ic_db::__private::handle(<#entity as ::ic_db::__private::ReferenceTarget>::NAME),
                    entity_name: <#entity as ::ic_db::__private::ReferenceTarget>::NAME,
                    index: ::ic_db::__private::handle(#index)
                }))
            }
        };

        quote!(::ic_db::FieldSchema { name:#name, value:#value, optional:#optional, reference:#reference, default:#default, managed:#managed })
    }

    pub fn type_description(&self) -> TokenStream {
        let item = match &self.kind {
            Kind::Named(path, _) => {
                quote!(::ic_db::FieldType::Named(::ic_db::TypeLink { name: <#path as ::ic_db::Value>::NAME, definition: <#path as ::ic_db::Value>::definition }))
            }
            Kind::Integer(integer) => {
                let name = quote::format_ident!("{}", integer.schema);
                quote!(::ic_db::FieldType::#name)
            }
            Kind::Float32 => quote!(::ic_db::FieldType::Float32),
            Kind::Float64 => quote!(::ic_db::FieldType::Float64),
            Kind::Subaccount => quote!(::ic_db::FieldType::Subaccount),
            Kind::Account => quote!(::ic_db::FieldType::Account),
            Kind::U256 => quote!(::ic_db::FieldType::U256),
            Kind::Ulid => quote!(::ic_db::FieldType::Ulid),
            Kind::Date => quote!(::ic_db::FieldType::Date),
            Kind::Decimal(scale) => quote!(::ic_db::FieldType::Decimal { scale: #scale }),
            Kind::Duration => quote!(::ic_db::FieldType::Duration),
            Kind::Timestamp => quote!(::ic_db::FieldType::Timestamp),
            Kind::Bool => quote!(::ic_db::FieldType::Bool),
            Kind::Principal => quote!(::ic_db::FieldType::Principal),
            Kind::Text(max_bytes) => quote!(::ic_db::FieldType::Text {max_bytes:#max_bytes}),
            Kind::Blob(max_bytes) => quote!(::ic_db::FieldType::Blob {max_bytes:#max_bytes}),
        };

        if let Some(max) = self.many {
            quote!(::ic_db::FieldType::List { item: &#item, max_items: #max })
        } else {
            item
        }
    }

    pub fn validation(&self, ident: &syn::Ident) -> TokenStream {
        if self.many.is_some() {
            return quote!();
        }
        if let Kind::Decimal(scale) = self.kind {
            let invalid = if self.optional {
                quote!(self.#ident.is_some_and(|v| v.coefficient_at_scale(#scale).is_none()))
            } else {
                quote!(self.#ident.coefficient_at_scale(#scale).is_none())
            };

            return quote!(if #invalid { return Err(::ic_db::Error::InvalidOperand); });
        }
        let (Kind::Text(max) | Kind::Blob(max)) = self.kind else {
            return quote!();
        };
        let invalid = if self.optional {
            quote!(self.#ident.as_ref().is_some_and(|value| value.len() > #max as usize))
        } else {
            quote!(self.#ident.len() > #max as usize)
        };

        quote!(if #invalid { return Err(::ic_db::Error::Limit); })
    }

    pub fn encode(&self, ident: &syn::Ident) -> TokenStream {
        self.encode_with(&quote!(&self.#ident))
    }

    pub fn encode_with(&self, value: &TokenStream) -> TokenStream {
        let write = match &self.kind {
            Kind::Named(path, indirect) => {
                if *indirect {
                    quote!(writer.named::<#path>(value.as_ref())?)
                } else {
                    quote!(writer.named::<#path>(value)?)
                }
            }
            Kind::Integer(integer) => {
                let name = quote::format_ident!("{}", integer.codec);
                quote!(writer.#name(*value)?)
            }
            Kind::Float32 => quote!(writer.float32(*value)?),
            Kind::Float64 => quote!(writer.float64(*value)?),
            Kind::Subaccount => quote!(writer.subaccount(*value)?),
            Kind::Account => quote!(writer.account(*value)?),
            Kind::U256 => quote!(writer.u256(*value)?),
            Kind::Ulid => quote!(writer.ulid(*value)?),
            Kind::Date => quote!(writer.date(*value)?),
            Kind::Decimal(scale) => quote!(writer.decimal(*value, #scale)?),
            Kind::Duration => quote!(writer.duration(*value)?),
            Kind::Timestamp => quote!(writer.timestamp(*value)?),
            Kind::Bool => quote!(writer.boolean(*value)?),
            Kind::Principal => quote!(writer.principal(value)?),
            Kind::Text(max) => {
                quote!({ if value.len() > #max as usize { return Err(::ic_db::Error::Limit); } writer.bytes(value.as_bytes())? })
            }
            Kind::Blob(max) => {
                quote!({ if value.len() > #max as usize { return Err(::ic_db::Error::Limit); } writer.bytes(value)? })
            }
        };
        let write = if let Some(max) = self.many {
            quote!(writer.list(value, #max, |writer, value| { #write; Ok(()) })?)
        } else {
            write
        };

        if self.optional {
            quote!(match #value {
                Some(value) => { writer.boolean(true)?; #write; }
                None => writer.boolean(false)?,
            })
        } else {
            quote!({ let value = #value; #write; })
        }
    }

    pub fn decode(&self, ident: &syn::Ident) -> TokenStream {
        let value = self.read_value();

        quote!(#ident: #value,)
    }

    fn read_item_result(&self) -> TokenStream {
        match &self.kind {
            Kind::Named(path, indirect) => {
                if *indirect {
                    quote!(reader.named::<#path>().map(::std::boxed::Box::new))
                } else {
                    quote!(reader.named::<#path>())
                }
            }
            Kind::Integer(integer) => {
                let name = quote::format_ident!("{}", integer.codec);
                quote!(reader.#name())
            }
            Kind::Float32 => quote!(reader.float32()),
            Kind::Float64 => quote!(reader.float64()),
            Kind::Subaccount => quote!(reader.subaccount()),
            Kind::Account => quote!(reader.account()),
            Kind::U256 => quote!(reader.u256()),
            Kind::Ulid => quote!(reader.ulid()),
            Kind::Date => quote!(reader.date()),
            Kind::Decimal(scale) => quote!(reader.decimal(#scale)),
            Kind::Duration => quote!(reader.duration()),
            Kind::Timestamp => quote!(reader.timestamp()),
            Kind::Bool => quote!(reader.boolean()),
            Kind::Principal => quote!(reader.principal()),
            Kind::Text(max) => quote!(reader.text(#max as usize)),
            Kind::Blob(max) => quote!(reader.blob(#max as usize)),
        }
    }

    pub fn read_result(&self) -> TokenStream {
        let item = self.read_item_result();
        let read = if let Some(max) = self.many {
            quote!(reader.list(#max, |reader| #item))
        } else {
            item
        };

        if self.optional {
            quote!(if reader.boolean()? { (#read).map(Some) } else { Ok(None) })
        } else {
            read
        }
    }

    pub fn read_value(&self) -> TokenStream {
        let read = self.read_result();

        quote!((#read)?)
    }
}
pub(super) fn parse_value(meta: &Meta) -> syn::Result<(Kind, bool, Option<u32>)> {
    let mut item = None;
    let mut optional = false;
    let mut many = false;
    let mut max_items = None;
    for part in meta_items(meta)? {
        if matches!(&part, Meta::Path(path) if path.is_ident("opt")) && !optional {
            optional = true;
        } else if matches!(&part, Meta::Path(path) if path.is_ident("many")) && !many {
            many = true;
        } else if part.path().is_ident("max_items") {
            once(&mut max_items, parse_max_items(&part)?, &part)?;
        } else if part.path().is_ident("item") {
            once(&mut item, parse_item(&part)?, &part)?;
        } else {
            return Err(syn::Error::new_spanned(
                part,
                "expected item(...), optional opt, or many with max_items",
            ));
        }
    }
    if many != max_items.is_some() {
        return Err(syn::Error::new_spanned(
            meta,
            "many requires max_items; max_items requires many",
        ));
    }

    Ok((
        item.ok_or_else(|| syn::Error::new_spanned(meta, "value requires item(...)"))?,
        optional,
        max_items,
    ))
}

pub(super) fn parse_max_items(part: &Meta) -> syn::Result<u32> {
    let Lit::Int(value) = literal(part)? else {
        return Err(syn::Error::new_spanned(part, "expected item count"));
    };
    let max: u32 = value.base10_parse()?;
    if max > 256 {
        return Err(syn::Error::new_spanned(
            part,
            "max_items must not exceed 256",
        ));
    }

    Ok(max)
}

pub(super) fn parse_item(meta: &Meta) -> syn::Result<Kind> {
    let mut prim = None;
    let mut named = None;
    let mut indirect = false;
    let mut max = None;
    let mut scale = None;
    for part in meta_items(meta)? {
        if matches!(&part, Meta::Path(path) if path.is_ident("indirect")) && !indirect {
            indirect = true;
        } else if part.path().is_ident("is") {
            once(
                &mut named,
                syn::parse_str::<syn::Path>(&text(&part)?)?,
                &part,
            )?;
        } else if part.path().is_ident("prim") {
            once(&mut prim, text(&part)?, &part)?;
        } else if part.path().is_ident("scale") {
            let Lit::Int(value) = literal(&part)? else {
                return Err(syn::Error::new_spanned(part, "expected decimal scale"));
            };
            let value: u32 = value.base10_parse()?;
            if value > 28 {
                return Err(syn::Error::new_spanned(part, "scale must be 0 through 28"));
            }
            once(&mut scale, value, &part)?;
        } else if part.path().is_ident("max_bytes") {
            let Lit::Int(value) = literal(&part)? else {
                return Err(syn::Error::new_spanned(part, "expected byte limit"));
            };
            let value: u32 = value.base10_parse()?;
            if value > 32768 {
                return Err(syn::Error::new_spanned(
                    part,
                    "max_bytes must not exceed 32768",
                ));
            }
            once(&mut max, value, &part)?;
        } else {
            return Err(syn::Error::new_spanned(
                part,
                "unsupported primitive option",
            ));
        }
    }
    if let Some(path) = named {
        if prim.is_some() || max.is_some() || scale.is_some() {
            return Err(syn::Error::new_spanned(
                meta,
                "named types cannot also declare prim, max_bytes or scale",
            ));
        }

        return Ok(Kind::Named(path, indirect));
    }
    if indirect {
        return Err(syn::Error::new_spanned(
            meta,
            "indirect currently requires a named type",
        ));
    }
    if prim.as_deref() == Some("Decimal") && max.is_none() {
        return scale
            .map(Kind::Decimal)
            .ok_or_else(|| syn::Error::new_spanned(meta, "Decimal requires scale"));
    }
    if scale.is_some() {
        return Err(syn::Error::new_spanned(
            meta,
            "scale is only supported for Decimal without max_bytes",
        ));
    }

    if max.is_none()
        && let Some(integer) = INTEGERS
            .iter()
            .find(|kind| Some(kind.schema) == prim.as_deref())
    {
        return Ok(Kind::Integer(integer));
    }
    match (prim.as_deref(), max) {
        (Some("Float32"), None) => Ok(Kind::Float32),
        (Some("Float64"), None) => Ok(Kind::Float64),
        (Some("Subaccount"), None) => Ok(Kind::Subaccount),
        (Some("Account"), None) => Ok(Kind::Account),
        (Some("U256"), None) => Ok(Kind::U256),
        (Some("Ulid"), None) => Ok(Kind::Ulid),
        (Some("Date"), None) => Ok(Kind::Date),
        (Some("Duration"), None) => Ok(Kind::Duration),
        (Some("Timestamp"), None) => Ok(Kind::Timestamp),
        (Some("Bool"), None) => Ok(Kind::Bool),
        (Some("Principal"), None) => Ok(Kind::Principal),
        (Some("Text"), Some(max)) => Ok(Kind::Text(max)),
        (Some("Text"), None) => Err(syn::Error::new_spanned(meta, "Text requires max_bytes")),
        (Some("Blob"), Some(max)) => Ok(Kind::Blob(max)),
        (Some("Blob"), None) => Err(syn::Error::new_spanned(meta, "Blob requires max_bytes")),
        _ => Err(syn::Error::new_spanned(
            meta,
            "supported here: Nat/Int8/16/32/64/128, U256, Ulid, Date, Timestamp, Duration, Decimal(scale), Float32/Float64, Bool, Principal, Subaccount, Account, Text(max_bytes), Blob(max_bytes); other types remain unimplemented",
        )),
    }
}

impl Kind {
    fn primary_key(&self) -> bool {
        matches!(
            self,
            Self::Integer(_)
                | Self::U256
                | Self::Ulid
                | Self::Principal
                | Self::Account
                | Self::Subaccount
        )
    }
}
