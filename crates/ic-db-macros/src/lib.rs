//! Small attribute authoring boundary for the first persistent-record slice.
//! Generates typed records/codecs and metadata, never endpoints or executors.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use std::collections::BTreeSet;
use syn::{Expr, ItemStruct, Lit, Meta, Token, parse::Parser, punctuated::Punctuated};

mod field;
use field::Field;

/// Author a scalar record with named fields, an external Nat64 key and indexes.
///
/// Supports Nat64/Int64, Nat128/Int128, Bool and bounded Text, optionally wrapped by `opt`.
/// Primary/index fields currently require non-optional Nat64. Unsupported shapes
/// refuse explicitly; this is not a retirement of the broader planned type surface.
/// Required Nat64 fields may declare `references(entity = 1, index = 1)`;
/// the selected index must belong to this entity and lead with this field.
#[proc_macro_attribute]
pub fn entity(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct Index {
    id: u32,
    fields: Vec<String>,
    unique: bool,
}

fn metas(tokens: Tokens) -> syn::Result<Vec<Meta>> {
    Ok(Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(tokens)?
        .into_iter()
        .collect())
}
fn list(meta: &Meta) -> syn::Result<Vec<Meta>> {
    match meta {
        Meta::List(value) => metas(value.tokens.clone()),
        _ => Err(syn::Error::new_spanned(meta, "expected attribute list")),
    }
}
fn literal(meta: &Meta) -> syn::Result<&Lit> {
    if let Meta::NameValue(value) = meta
        && let Expr::Lit(value) = &value.value
    {
        return Ok(&value.lit);
    }
    Err(syn::Error::new_spanned(meta, "expected literal value"))
}
fn text(meta: &Meta) -> syn::Result<String> {
    if let Lit::Str(value) = literal(meta)? {
        return Ok(value.value());
    }
    Err(syn::Error::new_spanned(meta, "expected a string"))
}
fn identity(meta: &Meta) -> syn::Result<u32> {
    if let Lit::Int(value) = literal(meta)? {
        let id = value.base10_parse()?;
        if id != 0 {
            return Ok(id);
        }
    }
    Err(syn::Error::new_spanned(
        meta,
        "identity must be a nonzero u32",
    ))
}
fn once<T>(slot: &mut Option<T>, value: T, meta: &Meta) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        return Err(syn::Error::new_spanned(meta, "duplicate attribute"));
    }
    Ok(())
}
fn fields(meta: &Meta) -> syn::Result<Vec<String>> {
    if let Meta::NameValue(value) = meta
        && let Expr::Array(values) = &value.value
    {
        return values
            .elems
            .iter()
            .map(|value| {
                if let Expr::Lit(value) = value
                    && let Lit::Str(value) = &value.lit
                {
                    return Ok(value.value());
                }
                Err(syn::Error::new_spanned(value, "expected field name"))
            })
            .collect();
    }
    Err(syn::Error::new_spanned(
        meta,
        "expected an array of field names",
    ))
}

fn expand(args: Tokens, item: Tokens) -> syn::Result<Tokens> {
    let item: ItemStruct = syn::parse2(item)?;
    if !matches!(&item.fields, syn::Fields::Named(fields) if fields.named.is_empty())
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            &item,
            "entity requires an empty, nongeneric struct body",
        ));
    }
    let mut id = None;
    let mut primary = None;
    let mut authored = None;
    let mut indexes = Vec::new();
    for meta in metas(args)? {
        if meta.path().is_ident("id") {
            once(&mut id, identity(&meta)?, &meta)?;
        } else if meta.path().is_ident("pk") {
            let mut key = None;
            let mut source = None;
            for part in list(&meta)? {
                if part.path().is_ident("field") {
                    once(&mut key, text(&part)?, &part)?;
                } else if part.path().is_ident("source") {
                    once(&mut source, text(&part)?, &part)?;
                } else {
                    return Err(syn::Error::new_spanned(
                        part,
                        "unsupported primary-key option",
                    ));
                }
            }
            if source.as_deref().is_some_and(|source| source != "external") {
                return Err(syn::Error::new_spanned(
                    meta,
                    "this slice requires externally supplied keys",
                ));
            }
            let key = key.ok_or_else(|| syn::Error::new_spanned(&meta, "pk requires field"))?;
            once(&mut primary, key, &meta)?;
        } else if meta.path().is_ident("fields") {
            let values = list(&meta)?
                .iter()
                .map(Field::parse)
                .collect::<syn::Result<Vec<_>>>()?;
            once(&mut authored, values, &meta)?;
        } else if meta.path().is_ident("index") {
            let mut id = None;
            let mut names = None;
            let mut unique = false;
            for part in list(&meta)? {
                if part.path().is_ident("id") {
                    once(&mut id, identity(&part)?, &part)?;
                } else if part.path().is_ident("fields") {
                    once(&mut names, fields(&part)?, &part)?;
                } else if part.path().is_ident("field") {
                    once(&mut names, vec![text(&part)?], &part)?;
                } else if matches!(&part, Meta::Path(path) if path.is_ident("unique")) && !unique {
                    unique = true;
                } else {
                    return Err(syn::Error::new_spanned(
                        part,
                        "unsupported or duplicate index option",
                    ));
                }
            }
            indexes.push(Index {
                id: id.ok_or_else(|| syn::Error::new_spanned(&meta, "index requires id"))?,
                fields: names
                    .ok_or_else(|| syn::Error::new_spanned(&meta, "index requires fields"))?,
                unique,
            });
        } else {
            return Err(syn::Error::new_spanned(meta, "unsupported entity option"));
        }
    }
    let fail = |message| syn::Error::new_spanned(&item.ident, message);
    let id = id.ok_or_else(|| fail("entity requires id"))?;
    let primary = primary.ok_or_else(|| fail("entity requires pk"))?;
    let authored = authored.ok_or_else(|| fail("entity requires fields"))?;
    let names: BTreeSet<_> = authored.iter().map(|field| &field.name).collect();
    if names.len() != authored.len()
        || authored.is_empty()
        || authored.len() > 16
        || !names.contains(&primary)
        || authored.iter().any(|field| field.name.len() > 64)
    {
        return Err(fail(
            "invalid, duplicate or excessive fields/primary key (1..16 fields)",
        ));
    }
    let field_ids: Vec<syn::Ident> = authored
        .iter()
        .map(|field| syn::parse_str(&field.name))
        .collect::<syn::Result<_>>()?;
    if !authored
        .iter()
        .any(|field| field.name == primary && field.required_nat64())
    {
        return Err(fail("primary key currently requires non-optional Nat64"));
    }
    let mut ordered: Vec<_> = authored.iter().collect();
    ordered.sort_by_key(|field| &field.name);
    let ordered_ids: Vec<syn::Ident> = ordered
        .iter()
        .map(|field| syn::parse_str(&field.name))
        .collect::<syn::Result<_>>()?;
    let key_id: syn::Ident = syn::parse_str(&primary)?;
    indexes.sort_by_key(|index| index.id);
    if indexes.len() > 8 || indexes.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(fail(
            "duplicate or excessive index identities (maximum eight)",
        ));
    }
    let mut descriptions = Vec::new();
    let mut encoded_indexes = Vec::new();
    for field in &authored {
        if let Some((_, id)) = field.reference
            && !indexes
                .iter()
                .any(|index| index.id == id && index.fields.first() == Some(&field.name))
        {
            return Err(fail(
                "reference requires a declared index beginning with its field",
            ));
        }
    }
    for index in indexes {
        let unique_fields: BTreeSet<_> = index.fields.iter().collect();
        if index.fields.is_empty()
            || index.fields.len() > 4
            || unique_fields.len() != index.fields.len()
            || index.fields.iter().any(|name| {
                !authored
                    .iter()
                    .any(|field| &field.name == name && field.required_nat64())
            })
        {
            return Err(fail(
                "index currently requires 1..4 distinct non-optional Nat64 fields",
            ));
        }
        let fields: Vec<syn::Ident> = index
            .fields
            .iter()
            .map(|name| syn::parse_str(name))
            .collect::<syn::Result<_>>()?;
        let index_id = index.id;
        let unique = index.unique;
        let names = index.fields;
        descriptions.push(quote! { ::ic_db::IndexSchema { id: #index_id, fields: &[#(#names),*], unique: #unique } });
        encoded_indexes.push(quote! {{ let mut bytes = ::std::vec::Vec::new(); #(bytes.extend(row.#fields.to_be_bytes());)* bytes }});
    }
    let name = &item.ident;
    let attrs = &item.attrs;
    let visibility = &item.vis;
    let types: Vec<_> = ordered.iter().map(|field| field.rust_type()).collect();
    let authored_types: Vec<_> = authored.iter().map(Field::rust_type).collect();
    let descriptions_fields: Vec<_> = ordered.iter().map(|field| field.description()).collect();
    let validations: Vec<_> = authored
        .iter()
        .zip(&field_ids)
        .map(|(field, id)| field.validation(id))
        .collect();
    Ok(quote! {
        #(#attrs)*
        #[derive(Clone, Debug, PartialEq, Eq)]
        #visibility struct #name { #(#[doc = "Authored stored field."] pub #field_ids: #authored_types,)* }
        impl ::ic_db::Entity for #name {
            const SCHEMA: &'static ::ic_db::EntitySchema = &::ic_db::EntitySchema {
                id: #id, name: stringify!(#name), fields: &[#(#descriptions_fields),*], primary_key: #primary,
                indexes: &[#(#descriptions),*],
                inspect_row: |bytes| {
                    let row = <Self as ::ic_db::Entity>::decode(bytes)?;
                    Ok(::ic_db::__private::RowInfo { key: row.#key_id, indexes: ::std::vec![#(#encoded_indexes),*] })
                },
            };
            fn validate(&self) -> Result<(), ::ic_db::Error> {
                #(#validations)*
                Ok(())
            }
            fn encode(&self) -> Result<::std::vec::Vec<u8>, ::ic_db::Error> {
                <Self as ::ic_db::Entity>::validate(self)?;
                ::ic_db::__private::native::encode(&(#(&self.#ordered_ids,)*), ::ic_db::__private::ROW_LIMITS)
                    .map_err(|_| ::ic_db::Error::Limit)
            }
            fn decode(bytes: &[u8]) -> Result<Self, ::ic_db::Error> {
                let (#(#ordered_ids,)*) : (#(#types,)*) =
                    ::ic_db::__private::native::decode(bytes, ::ic_db::__private::ROW_LIMITS)
                    .map_err(|_| ::ic_db::Error::Corrupt)?;
                let row = Self { #(#ordered_ids,)* };
                <Self as ::ic_db::Entity>::validate(&row).map_err(|_| ::ic_db::Error::Corrupt)?;
                Ok(row)
            }
        }
    })
}

#[cfg(test)]
mod tests;
