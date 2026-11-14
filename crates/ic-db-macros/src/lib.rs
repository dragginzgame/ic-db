#![forbid(unsafe_code)]
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
/// Supports Nat/Int8/16/32/64/128, Bool, Principal and bounded Text/Blob, optionally wrapped by `opt`.
/// Primary keys require Nat64; secondary indexes admit integers, Principal,
/// Bool and bounded Text, including optional forms. Optional unique indexes require
/// `present_only`, omitting a row when any optional key component is absent. Unsupported shapes
/// refuse explicitly; this is not a retirement of the broader planned type surface.
/// Entity names default to the type name; `name = "StableName"` overrides it.
/// Index names default to comma-separated ordered fields; `name` overrides them.
/// Required or optional Nat64 fields may declare `references(entity = "path::Target")`;
/// select the sole unfiltered index leading with this field, or use `index = "name"`.
/// `None` has no target; every `Some(id)` requires that target in the final batch state.
/// `entity = "path::Target"` lets Rust resolve an entity type (including imports
/// and `Self`) to its logical identity. Numeric authoring IDs are unsupported.
/// With the host-only `ic-db/build` feature, declarations also register their
/// full Rust paths for second-stage schema assembly.
#[proc_macro_attribute]
pub fn entity(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct Index {
    name: String,
    fields: Vec<String>,
    unique: bool,
    present_only: bool,
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
    let mut logical_name = None;
    let mut primary = None;
    let mut authored = None;
    let mut indexes = Vec::new();
    for meta in metas(args)? {
        if meta.path().is_ident("name") {
            once(&mut logical_name, text(&meta)?, &meta)?;
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
            let mut index_name = None;
            let mut names = None;
            let mut unique = false;
            let mut present_only = false;
            for part in list(&meta)? {
                if part.path().is_ident("name") {
                    once(&mut index_name, text(&part)?, &part)?;
                } else if part.path().is_ident("fields") {
                    once(&mut names, fields(&part)?, &part)?;
                } else if part.path().is_ident("field") {
                    once(&mut names, vec![text(&part)?], &part)?;
                } else if matches!(&part, Meta::Path(path) if path.is_ident("unique")) && !unique {
                    unique = true;
                } else if matches!(&part, Meta::Path(path) if path.is_ident("present_only"))
                    && !present_only
                {
                    present_only = true;
                } else {
                    return Err(syn::Error::new_spanned(
                        part,
                        "unsupported or duplicate index option",
                    ));
                }
            }
            let fields =
                names.ok_or_else(|| syn::Error::new_spanned(&meta, "index requires fields"))?;
            indexes.push(Index {
                name: index_name.unwrap_or_else(|| fields.join(",")),
                fields,
                unique,
                present_only,
            });
        } else {
            return Err(syn::Error::new_spanned(meta, "unsupported entity option"));
        }
    }
    let fail = |message| syn::Error::new_spanned(&item.ident, message);
    let logical_name = logical_name.unwrap_or_else(|| item.ident.to_string());
    if logical_name.is_empty() || logical_name.len() > 64 {
        return Err(fail("entity name requires 1..64 UTF-8 bytes"));
    }
    let primary = primary.ok_or_else(|| fail("entity requires pk"))?;
    let mut authored = authored.ok_or_else(|| fail("entity requires fields"))?;
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
    let key_id: syn::Ident = syn::parse_str(&primary)?;
    indexes.sort_by(|left, right| left.name.cmp(&right.name));
    if indexes.len() > 8
        || indexes.windows(2).any(|pair| pair[0].name == pair[1].name)
        || indexes
            .iter()
            .any(|index| index.name.is_empty() || index.name.len() > 64)
    {
        return Err(fail(
            "indexes require distinct names of 1..64 bytes (maximum eight); use name to disambiguate",
        ));
    }
    let mut descriptions = Vec::new();
    let mut encoded_indexes = Vec::new();
    for field in &mut authored {
        if let Some((_, selected)) = &mut field.reference {
            let mut candidates = indexes.iter().filter(|index| {
                !index.present_only
                    && index.fields.first() == Some(&field.name)
                    && selected.as_ref().is_none_or(|name| *name == index.name)
            });
            let Some(index) = candidates.next() else {
                return Err(fail(
                    "reference requires an unfiltered declared index beginning with its field",
                ));
            };
            if candidates.next().is_some() {
                return Err(fail(
                    "ambiguous reference access path; select an index by name",
                ));
            }
            *selected = Some(index.name.clone());
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
                    .any(|field| &field.name == name && field.indexable())
            })
        {
            return Err(fail(
                "index requires 1..4 distinct integer, Principal, Bool or Text fields",
            ));
        }
        let fields: Vec<syn::Ident> = index
            .fields
            .iter()
            .map(|name| syn::parse_str(name))
            .collect::<syn::Result<_>>()?;
        let index_name = index.name;
        let unique = index.unique;
        let present_only = index.present_only;
        let has_optional = index.fields.iter().any(|name| {
            authored
                .iter()
                .any(|field| &field.name == name && field.optional)
        });
        if (unique && has_optional && !present_only) || (present_only && (!unique || !has_optional))
        {
            return Err(fail(
                "optional unique indexes require present_only; present_only requires uniqueness and an optional component",
            ));
        }
        let operands: Vec<_> = index
            .fields
            .iter()
            .zip(&fields)
            .map(|(name, ident)| {
                let field = authored
                    .iter()
                    .find(|field| &field.name == name)
                    .expect("admitted index field");
                let value = field.index_value(ident);
                let optional = field.optional;
                quote!((#value, #optional))
            })
            .collect();
        let names = index.fields;
        descriptions.push(quote! { ::ic_db::IndexSchema { id: ::ic_db::__private::handle(#index_name), name: #index_name, fields: &[#(#names),*], unique: #unique, present_only: #present_only } });
        encoded_indexes
            .push(quote! { ::ic_db::__private::encode_index(&[#(#operands),*], #present_only) });
    }
    let name = &item.ident;
    let attrs = &item.attrs;
    let visibility = &item.vis;
    let mut ordered: Vec<_> = authored.iter().collect();
    ordered.sort_by_key(|field| &field.name);
    let ordered_ids: Vec<syn::Ident> = ordered
        .iter()
        .map(|field| syn::parse_str(&field.name))
        .collect::<syn::Result<_>>()?;
    let writes: Vec<_> = ordered
        .iter()
        .zip(&ordered_ids)
        .map(|(field, id)| field.encode(id))
        .collect();
    let reads: Vec<_> = ordered
        .iter()
        .zip(&ordered_ids)
        .map(|(field, id)| field.decode(id))
        .collect();
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
            const ID: u32 = ::ic_db::__private::handle(#logical_name);
            const NAME: &'static str = #logical_name;
            const PATH: &'static str = concat!(module_path!(), "::", stringify!(#name));
            const SCHEMA: &'static ::ic_db::EntitySchema = &::ic_db::EntitySchema {
                id: Self::ID, name: Self::NAME, fields: &[#(#descriptions_fields),*], primary_key: #primary,
                indexes: &[#(#descriptions),*],
            };
            fn key(&self) -> u64 { self.#key_id }
            fn __row_info(&self) -> ::ic_db::__private::RowInfo {
                let row = self;
                ::ic_db::__private::RowInfo {
                    key: <Self as ::ic_db::Entity>::key(row),
                    indexes: ::std::vec![#(#encoded_indexes),*]
                }
            }
            fn validate(&self) -> Result<(), ::ic_db::Error> {
                #(#validations)*
                Ok(())
            }
            fn encode(&self) -> Result<::std::vec::Vec<u8>, ::ic_db::Error> {
                <Self as ::ic_db::Entity>::validate(self)?;
                let mut writer = ::ic_db::__private::Encoder::new();
                #(#writes)*
                Ok(writer.finish())
            }
            fn decode(bytes: &[u8]) -> Result<Self, ::ic_db::Error> {
                let mut reader = ::ic_db::__private::Decoder::new(bytes)?;
                let row = Self { #(#reads)* };
                reader.finish()?;
                <Self as ::ic_db::Entity>::validate(&row).map_err(|_| ::ic_db::Error::Corrupt)?;
                Ok(row)
            }
        }
        ::ic_db::__register_schema! {
            const _: () = {
                #[::ic_db::build::__ctor::ctor(unsafe, anonymous, crate_path = ::ic_db::build::__ctor)]
                fn register() {
                    ::ic_db::build::__register(<#name as ::ic_db::Entity>::PATH, <#name as ::ic_db::Entity>::SCHEMA);
                }
            };
        }
    })
}

#[cfg(test)]
mod tests;
