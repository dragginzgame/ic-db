//! Parse and project stored field types at the macro boundary.
use super::{identity, list, literal, once, text};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Lit, Meta};

pub(super) enum Kind {
    Nat8,
    Nat16,
    Nat32,
    Int8,
    Int16,
    Int32,
    Nat64,
    Int64,
    Nat128,
    Int128,
    Bool,
    Principal,
    Text(u32),
    Blob(u32),
}
pub(super) struct Field {
    pub name: String,
    pub kind: Kind,
    pub optional: bool,
    pub reference: Option<(u32, u32)>,
}
impl Field {
    pub fn parse(meta: &Meta) -> syn::Result<Self> {
        if !meta.path().is_ident("field") {
            return Err(syn::Error::new_spanned(meta, "expected field(...)"));
        }
        let mut name = None;
        let mut value = None;
        let mut reference = None;
        for part in list(meta)? {
            if part.path().is_ident("name") {
                once(&mut name, text(&part)?, &part)?;
            } else if part.path().is_ident("value") {
                once(&mut value, parse_value(&part)?, &part)?;
            } else if part.path().is_ident("references") {
                let mut entity = None;
                let mut index = None;
                for option in list(&part)? {
                    if option.path().is_ident("entity") {
                        once(&mut entity, identity(&option)?, &option)?;
                    } else if option.path().is_ident("index") {
                        once(&mut index, identity(&option)?, &option)?;
                    } else {
                        return Err(syn::Error::new_spanned(
                            option,
                            "references requires entity and index",
                        ));
                    }
                }
                let target = entity.zip(index).ok_or_else(|| {
                    syn::Error::new_spanned(&part, "references requires entity and index")
                })?;
                once(&mut reference, target, &part)?;
            } else {
                return Err(syn::Error::new_spanned(
                    part,
                    "unsupported field option; fields use names, not numeric IDs",
                ));
            }
        }
        let name = name.ok_or_else(|| syn::Error::new_spanned(meta, "field requires name"))?;
        let (kind, optional) =
            value.ok_or_else(|| syn::Error::new_spanned(meta, "field requires value"))?;
        if reference.is_some() && !matches!(kind, Kind::Nat64) {
            return Err(syn::Error::new_spanned(
                meta,
                "references currently require Nat64",
            ));
        }
        Ok(Self {
            name,
            kind,
            optional,
            reference,
        })
    }
    pub fn required_nat64(&self) -> bool {
        matches!(self.kind, Kind::Nat64) && !self.optional
    }
    pub fn indexable(&self) -> bool {
        matches!(
            self.kind,
            Kind::Nat8
                | Kind::Nat16
                | Kind::Nat32
                | Kind::Nat64
                | Kind::Nat128
                | Kind::Int8
                | Kind::Int16
                | Kind::Int32
                | Kind::Int64
                | Kind::Int128
                | Kind::Principal
                | Kind::Bool
                | Kind::Text(_)
        )
    }
    pub fn index_value(&self, ident: &syn::Ident) -> TokenStream {
        if self.optional {
            if matches!(self.kind, Kind::Text(_)) {
                quote!(row.#ident.as_deref().map(::ic_db::IndexValue::from).unwrap_or(::ic_db::IndexValue::Absent))
            } else {
                quote!(row.#ident.map(::ic_db::IndexValue::from).unwrap_or(::ic_db::IndexValue::Absent))
            }
        } else if matches!(self.kind, Kind::Text(_)) {
            quote!(::ic_db::IndexValue::from(row.#ident.as_str()))
        } else {
            quote!(::ic_db::IndexValue::from(row.#ident))
        }
    }
    pub fn rust_type(&self) -> TokenStream {
        let base = match self.kind {
            Kind::Nat8 => quote!(u8),
            Kind::Nat16 => quote!(u16),
            Kind::Nat32 => quote!(u32),
            Kind::Int8 => quote!(i8),
            Kind::Int16 => quote!(i16),
            Kind::Int32 => quote!(i32),
            Kind::Nat64 => quote!(u64),
            Kind::Int64 => quote!(i64),
            Kind::Nat128 => quote!(u128),
            Kind::Int128 => quote!(i128),
            Kind::Bool => quote!(bool),
            Kind::Principal => quote!(::ic_db::Principal),
            Kind::Text(_) => quote!(::std::string::String),
            Kind::Blob(_) => quote!(::ic_db::Blob),
        };
        if self.optional {
            quote!(::std::option::Option<#base>)
        } else {
            base
        }
    }
    pub fn description(&self) -> TokenStream {
        let name = &self.name;
        let optional = self.optional;
        let value = match self.kind {
            Kind::Nat8 => quote!(::ic_db::FieldType::Nat8),
            Kind::Nat16 => quote!(::ic_db::FieldType::Nat16),
            Kind::Nat32 => quote!(::ic_db::FieldType::Nat32),
            Kind::Int8 => quote!(::ic_db::FieldType::Int8),
            Kind::Int16 => quote!(::ic_db::FieldType::Int16),
            Kind::Int32 => quote!(::ic_db::FieldType::Int32),
            Kind::Nat64 => quote!(::ic_db::FieldType::Nat64),
            Kind::Int64 => quote!(::ic_db::FieldType::Int64),
            Kind::Nat128 => quote!(::ic_db::FieldType::Nat128),
            Kind::Int128 => quote!(::ic_db::FieldType::Int128),
            Kind::Bool => quote!(::ic_db::FieldType::Bool),
            Kind::Principal => quote!(::ic_db::FieldType::Principal),
            Kind::Text(max_bytes) => quote!(::ic_db::FieldType::Text {max_bytes:#max_bytes}),
            Kind::Blob(max_bytes) => quote!(::ic_db::FieldType::Blob {max_bytes:#max_bytes}),
        };
        let reference = match self.reference {
            None => quote!(None),
            Some((entity, index)) => {
                quote!(Some(::ic_db::ReferenceSchema {entity:#entity,index:#index}))
            }
        };
        quote!(::ic_db::FieldSchema { name:#name, value:#value, optional:#optional, reference:#reference })
    }
    pub fn validation(&self, ident: &syn::Ident) -> TokenStream {
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
        let write = match self.kind {
            Kind::Nat8 => quote!(writer.nat8(*value)?),
            Kind::Nat16 => quote!(writer.nat16(*value)?),
            Kind::Nat32 => quote!(writer.nat32(*value)?),
            Kind::Int8 => quote!(writer.int8(*value)?),
            Kind::Int16 => quote!(writer.int16(*value)?),
            Kind::Int32 => quote!(writer.int32(*value)?),
            Kind::Nat64 => quote!(writer.nat64(*value)?),
            Kind::Nat128 => quote!(writer.nat128(*value)?),
            Kind::Int64 => quote!(writer.int64(*value)?),
            Kind::Int128 => quote!(writer.int128(*value)?),
            Kind::Bool => quote!(writer.boolean(*value)?),
            Kind::Principal => quote!(writer.principal(value)?),
            Kind::Text(_) => quote!(writer.bytes(value.as_bytes())?),
            Kind::Blob(_) => quote!(writer.bytes(value)?),
        };
        if self.optional {
            quote!(match &self.#ident {
                Some(value) => { writer.boolean(true)?; #write; }
                None => writer.boolean(false)?,
            })
        } else {
            quote!({ let value = &self.#ident; #write; })
        }
    }
    pub fn decode(&self, ident: &syn::Ident) -> TokenStream {
        let read = match self.kind {
            Kind::Nat8 => quote!(reader.nat8()?),
            Kind::Nat16 => quote!(reader.nat16()?),
            Kind::Nat32 => quote!(reader.nat32()?),
            Kind::Int8 => quote!(reader.int8()?),
            Kind::Int16 => quote!(reader.int16()?),
            Kind::Int32 => quote!(reader.int32()?),
            Kind::Nat64 => quote!(reader.nat64()?),
            Kind::Nat128 => quote!(reader.nat128()?),
            Kind::Int64 => quote!(reader.int64()?),
            Kind::Int128 => quote!(reader.int128()?),
            Kind::Bool => quote!(reader.boolean()?),
            Kind::Principal => quote!(reader.principal()?),
            Kind::Text(max) => quote!(reader.text(#max as usize)?),
            Kind::Blob(max) => quote!(reader.blob(#max as usize)?),
        };
        if self.optional {
            quote!(#ident: if reader.boolean()? { Some(#read) } else { None },)
        } else {
            quote!(#ident: #read,)
        }
    }
}
fn parse_value(meta: &Meta) -> syn::Result<(Kind, bool)> {
    let mut item = None;
    let mut optional = false;
    for part in list(meta)? {
        if matches!(&part, Meta::Path(path) if path.is_ident("opt")) && !optional {
            optional = true;
        } else if part.path().is_ident("item") {
            once(&mut item, parse_item(&part)?, &part)?;
        } else {
            return Err(syn::Error::new_spanned(
                part,
                "expected one item(...) and optional opt",
            ));
        }
    }
    Ok((
        item.ok_or_else(|| syn::Error::new_spanned(meta, "value requires item(...)"))?,
        optional,
    ))
}
fn parse_item(meta: &Meta) -> syn::Result<Kind> {
    let mut prim = None;
    let mut max = None;
    for part in list(meta)? {
        if part.path().is_ident("prim") {
            once(&mut prim, text(&part)?, &part)?;
        } else if part.path().is_ident("max_bytes") {
            let Lit::Int(value) = literal(&part)? else {
                return Err(syn::Error::new_spanned(part, "expected byte limit"));
            };
            let value: u32 = value.base10_parse()?;
            if value > 4096 {
                return Err(syn::Error::new_spanned(
                    part,
                    "max_bytes must not exceed 4096",
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
    match (prim.as_deref(), max) {
        (Some("Nat8"), None) => Ok(Kind::Nat8),
        (Some("Nat16"), None) => Ok(Kind::Nat16),
        (Some("Nat32"), None) => Ok(Kind::Nat32),
        (Some("Int8"), None) => Ok(Kind::Int8),
        (Some("Int16"), None) => Ok(Kind::Int16),
        (Some("Int32"), None) => Ok(Kind::Int32),
        (Some("Nat64"), None) => Ok(Kind::Nat64),
        (Some("Int64"), None) => Ok(Kind::Int64),
        (Some("Nat128"), None) => Ok(Kind::Nat128),
        (Some("Int128"), None) => Ok(Kind::Int128),
        (Some("Bool"), None) => Ok(Kind::Bool),
        (Some("Principal"), None) => Ok(Kind::Principal),
        (Some("Text"), Some(max)) => Ok(Kind::Text(max)),
        (Some("Text"), None) => Err(syn::Error::new_spanned(meta, "Text requires max_bytes")),
        (Some("Blob"), Some(max)) => Ok(Kind::Blob(max)),
        (Some("Blob"), None) => Err(syn::Error::new_spanned(meta, "Blob requires max_bytes")),
        _ => Err(syn::Error::new_spanned(
            meta,
            "supported here: Nat/Int8/16/32/64/128, Bool, Principal, Text(max_bytes), Blob(max_bytes); other types remain unimplemented",
        )),
    }
}
