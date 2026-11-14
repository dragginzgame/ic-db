//! Parse and project stored field types at the macro boundary.
use super::{identity, list, literal, once, text};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Lit, Meta};

pub(super) enum Kind {
    Nat64,
    Int64,
    Nat128,
    Int128,
    Bool,
    Text(u32),
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
        if reference.is_some() && (optional || !matches!(kind, Kind::Nat64)) {
            return Err(syn::Error::new_spanned(
                meta,
                "references currently require non-optional Nat64",
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
    pub fn rust_type(&self) -> TokenStream {
        let base = match self.kind {
            Kind::Nat64 => quote!(u64),
            Kind::Int64 => quote!(i64),
            Kind::Nat128 => quote!(u128),
            Kind::Int128 => quote!(i128),
            Kind::Bool => quote!(bool),
            Kind::Text(_) => quote!(::std::string::String),
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
            Kind::Nat64 => quote!(::ic_db::FieldType::Nat64),
            Kind::Int64 => quote!(::ic_db::FieldType::Int64),
            Kind::Nat128 => quote!(::ic_db::FieldType::Nat128),
            Kind::Int128 => quote!(::ic_db::FieldType::Int128),
            Kind::Bool => quote!(::ic_db::FieldType::Bool),
            Kind::Text(max_bytes) => quote!(::ic_db::FieldType::Text {max_bytes:#max_bytes}),
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
        let Kind::Text(max) = self.kind else {
            return quote!();
        };
        let invalid = if self.optional {
            quote!(self.#ident.as_ref().is_some_and(|value| value.len() > #max as usize))
        } else {
            quote!(self.#ident.len() > #max as usize)
        };
        quote!(if #invalid { return Err(::ic_db::Error::Limit); })
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
            if !(1..=4096).contains(&value) {
                return Err(syn::Error::new_spanned(
                    part,
                    "text max_bytes must be 1..=4096",
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
        (Some("Nat64"), None) => Ok(Kind::Nat64),
        (Some("Int64"), None) => Ok(Kind::Int64),
        (Some("Nat128"), None) => Ok(Kind::Nat128),
        (Some("Int128"), None) => Ok(Kind::Int128),
        (Some("Bool"), None) => Ok(Kind::Bool),
        (Some("Text"), Some(max)) => Ok(Kind::Text(max)),
        (Some("Text"), None) => Err(syn::Error::new_spanned(meta, "Text requires max_bytes")),
        _ => Err(syn::Error::new_spanned(
            meta,
            "supported here: Nat64, Int64, Nat128, Int128, Bool, Text(max_bytes); other types remain unimplemented",
        )),
    }
}
