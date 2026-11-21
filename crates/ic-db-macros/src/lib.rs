#![forbid(unsafe_code)]
//! Attribute macros for IC DB schema declarations.
//! Generates typed records/codecs and metadata, never endpoints or executors.
//! All declarations use empty, nongeneric struct bodies without `where` clauses;
//! unsupported Rust constraints are rejected rather than discarded during expansion.

mod enumeration;
mod field;
mod insert;
mod map;
mod ownership;
mod record;
mod set;
#[cfg(test)]
mod tests;
mod unique;
mod wrapped;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use std::collections::BTreeSet;
use syn::{Expr, ItemStruct, Lit, Meta, Token, parse::Parser, punctuated::Punctuated};

use field::Field;

/// Author a typed record with named fields, a supplied scalar key and indexes.
///
/// Each field has an associated name constant, e.g. `User::NAME` for `name`.
/// Field types follow the implemented `ic_db::FieldType` contract, with explicit
/// bounds for variable-size values. Secondary indexes admit supported ordered
/// scalar fields and unit enums, including optional forms. Optional unique indexes require
/// `present_only`, omitting a row when any optional key component is absent. Unsupported shapes
/// refuse explicitly; this is not a retirement of the broader planned type surface.
/// Entity names default to the type name; `name = "StableName"` overrides it.
/// A nonunique single-component index can name a required indexable scalar
/// in a required record, e.g. `index(field = "labels.text")`.
/// Index names default to comma-separated ordered fields; `name` overrides them.
/// A unique index on `User(issuer, subject)` generates `UserByIssuerAndSubject`
/// with named fields for `session.get_unique(&key)`. A sparse unique key takes
/// the present component types. Equivalent unique indexes share a key type.
/// Required or optional supported key fields may declare `references(entity = "path::Target")`;
/// select the sole unfiltered index leading with this field, or use `index = "name"`.
/// `None` has no target; every `Some(id)` requires that target in the final batch state.
/// `entity = "path::Target"` lets Rust resolve an entity type (including imports
/// and `Self`) to its logical identity. Numeric authoring IDs are unsupported.
/// With the host-only `ic-db/build` feature, declarations also register their
/// full Rust paths for second-stage schema assembly.
/// Fields may declare `default = LITERAL` for integers, Bool, bounded Text and Blob.
/// Blob defaults use Rust byte strings, e.g. `default = b"\x00\xff"`, never text/hex coercion.
/// `Entity::insert()` builds typed omission-aware input for `Batch::insert_input`;
/// explicit optional `None` is never replaced by a default. Primary-key and nested
/// record defaults are not implemented. Defaults never run on decode/replacement.
/// Required Timestamp fields and their scalar newtypes may declare `managed = "created_at"` or
/// `managed = "updated_at"`, once per role. These fields have no insert setters;
/// use `Batch::at(time)` to supply one operation time for the entire batch.
#[proc_macro_attribute]
pub fn entity(args: TokenStream, item: TokenStream) -> TokenStream {
    expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a named enum using an empty struct and `variant(name = "...")` attributes.
/// Supports 1–64 unit or typed payload variants. Canonical name order determines
/// stored ordinals; source declaration order does not. Use item(indirect, is = "Self")
/// for boxed recursion, with a finite base alternative.
/// All-unit enums also convert by reference into `ic_db::IndexValue`, using the
/// same canonical order. Any payload variant excludes the whole enum from indexes.
#[proc_macro_attribute]
pub fn enum_(args: TokenStream, item: TokenStream) -> TokenStream {
    enumeration::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a reusable ordered list with an item type and a shared maximum length.
#[proc_macro_attribute]
pub fn list(args: TokenStream, item: TokenStream) -> TokenStream {
    wrapped::expand(args.into(), item.into(), true)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a distinct Rust and schema type around one primitive, including its bounds.
#[proc_macro_attribute]
pub fn newtype(args: TokenStream, item: TokenStream) -> TokenStream {
    wrapped::expand(args.into(), item.into(), false)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a bounded ordered set with `item(...)` and `max_items = N` (0..256).
/// Elements can be primitives, scalar newtypes or unit enums. The generated
/// wrapper contains a `BTreeSet<T>`. Decoding requires strictly increasing keys.
#[proc_macro_attribute]
pub fn set(args: TokenStream, item: TokenStream) -> TokenStream {
    set::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a named `BTreeMap` wrapper using `key(...)`, `value(...)` and
/// `max_items = N` (0..256). Keys are primitives, scalar newtypes or unit enums. Values use the
/// ordinary optional/list/named field rules; stored duplicate keys refuse.
#[proc_macro_attribute]
pub fn map(args: TokenStream, item: TokenStream) -> TokenStream {
    map::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a named stored record using attribute-owned fields and an empty struct.
#[proc_macro_attribute]
pub fn record(args: TokenStream, item: TokenStream) -> TokenStream {
    record::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a logical database canister.
#[proc_macro_attribute]
pub fn canister(args: TokenStream, item: TokenStream) -> TokenStream {
    ownership::expand(args.into(), item.into(), false)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declare a store with a typed canister owner and an IC Storage allocation key.
#[proc_macro_attribute]
pub fn store(args: TokenStream, item: TokenStream) -> TokenStream {
    ownership::expand(args.into(), item.into(), true)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn membership(name: &syn::Ident) -> Tokens {
    quote! {
            const _: () = {
                if let ::std::option::Option::Some(root) = ::std::option_env!("IC_DB_SCHEMA_ROOT") {
                    const PATH: &str = ::std::concat!(::std::module_path!(), "::", ::std::stringify!(#name));
                    ::std::assert!(
                        ::ic_db::__private::check_schema_member(
                            PATH, root,
                            ::std::option_env!(::std::concat!("IC_DB_SCHEMA_MEMBER_", ::std::stringify!(#name))),
                        ),
                        ::std::concat!("declaration missing from host schema: ", ::std::module_path!(), "::", ::std::stringify!(#name)),
                    );
                }
            };
    }
}

fn owner_path(meta: &Meta) -> syn::Result<syn::Path> {
    if let Meta::NameValue(pair) = meta
        && let Expr::Path(path) = &pair.value
    {
        return Ok(path.path.clone());
    }
    Err(syn::Error::new_spanned(meta, "expected a Rust type path"))
}

struct Index {
    name: String,
    fields: Vec<String>,
    unique: bool,
    present_only: bool,
}

// Supply Serde through ic-db even in schema crates without a direct serde dependency.
fn schema_attrs(attrs: &[syn::Attribute]) -> syn::Result<Tokens> {
    let mut serialize = true;
    let mut deserialize = true;
    let mut supply_crate = true;
    for attr in attrs {
        if attr.path().is_ident("derive") {
            for path in
                attr.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)?
            {
                if let Some(name) = path.segments.last() {
                    serialize &= name.ident != "Serialize";
                    deserialize &= name.ident != "Deserialize";
                }
            }
        } else if attr.path().is_ident("serde") {
            supply_crate &= !meta_items(&attr.meta)?
                .iter()
                .any(|m| m.path().is_ident("crate"));
        }
    }
    let ser = serialize.then(|| quote!(#[derive(::ic_db::__private::serde::Serialize)]));
    let de = deserialize.then(|| quote!(#[derive(::ic_db::__private::serde::Deserialize)]));
    let path = (supply_crate && (serialize || deserialize))
        .then(|| quote!(#[serde(crate = "::ic_db::__private::serde")]));

    Ok(quote!(#ser #de #(#attrs)* #path))
}

fn metas(tokens: Tokens) -> syn::Result<Vec<Meta>> {
    Ok(Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(tokens)?
        .into_iter()
        .collect())
}

fn meta_items(meta: &Meta) -> syn::Result<Vec<Meta>> {
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

fn declaration_name(name: Option<String>, ident: &syn::Ident, kind: &str) -> syn::Result<String> {
    let name = name.unwrap_or_else(|| ident.to_string());
    if !(1..=64).contains(&name.len()) {
        return Err(syn::Error::new_spanned(
            ident,
            format!("{kind} name must contain 1..64 UTF-8 bytes"),
        ));
    }

    Ok(name)
}

fn declaration(item: Tokens, kind: &str) -> syn::Result<ItemStruct> {
    let item: ItemStruct = syn::parse2(item)?;
    if !matches!(&item.fields, syn::Fields::Named(fields) if fields.named.is_empty())
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            &item,
            format!("{kind} requires an empty, nongeneric struct body without a where clause"),
        ));
    }

    Ok(item)
}

fn field_constants(fields: &[Field], owner: &syn::Ident) -> syn::Result<Vec<Tokens>> {
    let mut constant_names = BTreeSet::new();
    let mut field_constants = Vec::with_capacity(fields.len());
    for field in fields {
        let source = &field.name;
        let constant = source
            .strip_prefix("r#")
            .unwrap_or(source)
            .to_ascii_uppercase();
        if !constant_names.insert(constant.clone()) {
            return Err(syn::Error::new_spanned(
                owner,
                format!("fields generate the same associated constant `{constant}`"),
            ));
        }
        let ident = syn::parse_str::<syn::Ident>(&constant)?;
        let doc = format!("Logical field name `{source}` for fluent queries.");
        field_constants.push(quote! { #[doc = #doc] pub const #ident: &'static str = #source; });
    }
    Ok(field_constants)
}

fn expand(args: Tokens, item: Tokens) -> syn::Result<Tokens> {
    let item = declaration(item, "entity")?;
    let mut logical_name = None;
    let mut primary = None;
    let mut store = None;
    let mut authored = None;
    let mut timestamps = None;
    let mut indexes = Vec::new();
    for meta in metas(args)? {
        if meta.path().is_ident("name") {
            once(&mut logical_name, text(&meta)?, &meta)?;
        } else if meta.path().is_ident("store") {
            once(&mut store, owner_path(&meta)?, &meta)?;
        } else if matches!(&meta, Meta::Path(path) if path.is_ident("timestamps")) {
            once(&mut timestamps, (), &meta)?;
        } else if meta.path().is_ident("pk") {
            let mut key = None;
            for part in meta_items(&meta)? {
                if part.path().is_ident("field") {
                    once(&mut key, text(&part)?, &part)?;
                } else {
                    return Err(syn::Error::new_spanned(
                        part,
                        "pk accepts only field = \"name\"",
                    ));
                }
            }
            let key = key.ok_or_else(|| syn::Error::new_spanned(&meta, "pk requires field"))?;
            once(&mut primary, key, &meta)?;
        } else if meta.path().is_ident("fields") {
            let values = meta_items(&meta)?
                .iter()
                .map(Field::parse)
                .collect::<syn::Result<Vec<_>>>()?;
            once(&mut authored, values, &meta)?;
        } else if meta.path().is_ident("index") {
            let mut index_name = None;
            let mut names = None;
            let mut unique = false;
            let mut present_only = false;
            for part in meta_items(&meta)? {
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
    let logical_name = declaration_name(logical_name, &item.ident, "entity")?;
    let store = store.ok_or_else(|| fail("entity requires store = StoreType"))?;
    let primary = primary.ok_or_else(|| fail("entity requires pk"))?;
    let mut authored = authored.ok_or_else(|| fail("entity requires fields"))?;
    if timestamps.is_some() {
        for (name, role) in [
            ("created_at", field::Management::CreatedAt),
            ("updated_at", field::Management::UpdatedAt),
        ] {
            authored.push(Field {
                managed: Some(role),
                ..Field::nested(name, field::Kind::Timestamp, false, None)
            });
        }
    }
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
    let field_constants = field_constants(&authored, &item.ident)?;
    let key_type = authored
        .iter()
        .find(|field| field.name == primary && field.required_key())
        .ok_or_else(|| {
            fail("primary key currently requires a non-optional integer, U256, Ulid, Principal, Account or Subaccount")
        })?
        .rust_type();
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
            let candidates: Vec<_> = indexes
                .iter()
                .filter(|index| !index.present_only && index.fields.first() == Some(&field.name))
                .map(|index| index.name.as_str())
                .collect();
            match (selected.as_deref(), candidates.as_slice()) {
                (Some(name), _) if candidates.contains(&name) => {}
                (None, [name]) => *selected = Some((*name).to_owned()),
                (requested, eligible) => {
                    let reason = match requested {
                        Some(name) => format!("cannot use index {name:?}"),
                        None => "requires one matching index".to_owned(),
                    };
                    let hint = if eligible.is_empty() {
                        format!(
                            "declare an unfiltered index beginning with {:?}",
                            field.name
                        )
                    } else {
                        format!("select one with `index = ...` from {eligible:?}")
                    };
                    return Err(fail(&format!(
                        "reference {}.{} {reason}; {hint}",
                        item.ident, field.name
                    )));
                }
            }
        }
    }
    let mut unique_keys = unique::Keys::default();
    for index in indexes {
        let index_name = &index.name;
        let unique = index.unique;
        let present_only = index.present_only;
        let names = &index.fields;
        descriptions.push(quote! { ::ic_db::IndexSchema { id: ::ic_db::__private::handle(#index_name), name: #index_name, fields: &[#(#names),*], unique: #unique, present_only: #present_only } });
        if let [path] = index.fields.as_slice()
            && let Some((parent, member)) = path.split_once('.')
        {
            if index.unique
                || index.present_only
                || !authored.iter().any(|f| f.name == parent && !f.optional)
            {
                return Err(fail(
                    "a record-member index requires a single required indexable scalar member on a required record and cannot be unique",
                ));
            }
            let parent: syn::Ident = syn::parse_str(parent)?;
            let member: syn::Ident = syn::parse_str(member)?;
            encoded_indexes.push(quote! {{
                use ::ic_db::__private::MemberOperand as _;
                ::ic_db::__private::encode_index(&[(row.#parent.#member.__ic_db_member_operand(), false)], false)
            }});
            continue;
        }
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
                "index requires 1..4 distinct supported scalar or unit-enum fields",
            ));
        }
        let fields: Vec<syn::Ident> = index
            .fields
            .iter()
            .map(|name| syn::parse_str(name))
            .collect::<syn::Result<_>>()?;
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
        if unique {
            unique_keys.add(&item, index_name, &fields, &authored, &index.fields);
        }
        encoded_indexes
            .push(quote! { ::ic_db::__private::encode_index(&[#(#operands),*], #present_only) });
    }
    let lookup_keys = unique_keys.declarations;
    let name = &item.ident;
    let attrs = crate::schema_attrs(&item.attrs)?;
    let visibility = &item.vis;
    let mut ordered: Vec<_> = authored.iter().collect();
    ordered.sort_by_key(|field| &field.name);
    let ordered_ids: Vec<syn::Ident> = ordered
        .iter()
        .map(|field| syn::parse_str(&field.name))
        .collect::<syn::Result<_>>()?;
    let nested_writes: Vec<_> = ordered
        .iter()
        .zip(&ordered_ids)
        .filter(|(f, _)| f.many.is_some() || matches!(f.kind, field::Kind::Named(..)))
        .map(|(f, id)| f.encode(id))
        .collect();
    let nested_validation = if nested_writes.is_empty() {
        quote!()
    } else {
        quote! { let mut writer = ::ic_db::__private::Encoder::new(); #(#nested_writes)* }
    };
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
    if authored
        .iter()
        .any(|field| field.name == primary && field.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "supplied primary keys cannot declare an insert default",
        ));
    }
    let insert_input = insert::emit(&item, &authored)?;
    let mut managed_fields = [None, None];
    for (field, id) in authored.iter().zip(&field_ids) {
        if let Some(policy) = field.managed {
            let slot = usize::from(policy == field::Management::UpdatedAt);
            let timestamp = if matches!(field.kind, field::Kind::Named(..)) {
                quote!(#id.0)
            } else {
                quote!(#id)
            };
            if managed_fields[slot].replace(timestamp).is_some() {
                return Err(syn::Error::new_spanned(
                    id,
                    "each managed timestamp role may appear once per entity",
                ));
            }
        }
    }
    let managed_check = |owner: Tokens| {
        if managed_fields.iter().all(Option::is_none) {
            return quote!();
        }
        let values = managed_fields.iter().map(|field| {
            field
                .as_ref()
                .map_or_else(|| quote!(None), |field| quote!(Some(#owner.#field)))
        });

        quote!(::ic_db::__private::validate_managed_order([#(#values),*])?;)
    };
    let membership = membership(name);
    let reference_checks = authored.iter().filter_map(|field| field.reference.as_ref()).map(|(target,_)| quote! {
        { fn same_store<T: ::ic_db::Entity<Store = #store>>() {} let _ = same_store::<#target>; }
    });
    let validate_managed = managed_check(quote!(self));
    let decode_managed = managed_check(quote!(row));

    Ok(quote! {
        #insert_input
        #(#lookup_keys)*
        #attrs
        #[derive(Clone, Debug, PartialEq, Eq)]
        #visibility struct #name { #(#[doc = "Authored stored field."] pub #field_ids: #authored_types,)* }

        impl #name { #(#field_constants)* }

        impl ::ic_db::Entity for #name {
            type Store = #store;
            type Key = #key_type;
            fn declaration_path() -> &'static str { #(#reference_checks)* concat!(module_path!(), "::", stringify!(#name)) }
            const SCHEMA: &'static ::ic_db::EntitySchema = &::ic_db::EntitySchema {
                id: ::ic_db::__private::handle(#logical_name), name: #logical_name, fields: &[#(#descriptions_fields),*], primary_key: #primary,
                indexes: &[#(#descriptions),*],
            };
            fn key(&self) -> Self::Key { self.#key_id }

            fn __row_info(&self) -> ::ic_db::__private::RowInfo {
                let row = self;

                ::ic_db::__private::RowInfo {
                    key: ::ic_db::PrimaryKey::into_row_key(<Self as ::ic_db::Entity>::key(row)),
                    indexes: ::std::vec![#(#encoded_indexes),*]
                }
            }

            fn validate(&self) -> Result<(), ::ic_db::Error> {
                #validate_managed
                #(#validations)*
                #nested_validation

                Ok(())
            }

            fn encode(&self) -> Result<::std::vec::Vec<u8>, ::ic_db::Error> {
                #validate_managed
                let mut writer = ::ic_db::__private::Encoder::new();
                #(#writes)*

                Ok(writer.finish())
            }

            fn decode(bytes: &[u8]) -> Result<Self, ::ic_db::Error> {
                let mut reader = ::ic_db::__private::Decoder::new(bytes)?;
                let row = Self { #(#reads)* };
                reader.finish()?;
                #decode_managed

                Ok(row)
            }
        }

        impl ::ic_db::__private::ReferenceTarget for #name {
            const NAME: &'static str = #logical_name;
        }

        #membership
        ::ic_db::__register_schema!(#name);
    })
}
