//! Typed omission at the construction boundary; no second mutation executor.

use super::field::{Field, Kind};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{Expr, ItemStruct, Lit, Meta};

pub(super) enum Literal {
    Nat(u128),
    Int(i128),
    Bool(bool),
    Text(String),
    Blob(Vec<u8>),
}

impl Literal {
    pub fn parse(meta: &Meta, kind: &Kind, many: Option<u32>) -> syn::Result<Self> {
        let fail = || {
            syn::Error::new_spanned(
                meta,
                "default requires an in-range unsuffixed integer, Bool, bounded Text string or Blob byte-string literal matching the field; other default types are not implemented",
            )
        };
        if many.is_some() {
            return Err(fail());
        }
        let Meta::NameValue(value) = meta else {
            return Err(fail());
        };
        let (literal, negative) = match &value.value {
            Expr::Lit(value) => (&value.lit, false),
            Expr::Unary(value) if matches!(value.op, syn::UnOp::Neg(_)) => {
                let Expr::Lit(value) = value.expr.as_ref() else {
                    return Err(fail());
                };

                (&value.lit, true)
            }
            _ => return Err(fail()),
        };
        match (kind, literal, negative) {
            (Kind::Bool, Lit::Bool(value), false) => return Ok(Self::Bool(value.value)),
            (Kind::Text(max), Lit::Str(value), false) if value.value().len() <= *max as usize => {
                return Ok(Self::Text(value.value()));
            }
            (Kind::Blob(max), Lit::ByteStr(value), false)
                if value.value().len() <= *max as usize =>
            {
                return Ok(Self::Blob(value.value()));
            }
            _ => {}
        }
        let Lit::Int(value) = literal else {
            return Err(fail());
        };
        if !value.suffix().is_empty() {
            return Err(fail());
        }
        // Syn may represent a negative integer inside LitInt or as unary negation.
        let digits = value.base10_digits();
        let (digits, negative) = if let Some(digits) = digits.strip_prefix('-') {
            if negative {
                return Err(fail());
            }

            (digits, true)
        } else {
            (digits, negative)
        };
        let magnitude = digits.parse::<u128>().map_err(|_| fail())?;
        let (signed, bits) = match kind {
            Kind::Nat8 => (false, 8),
            Kind::Nat16 => (false, 16),
            Kind::Nat32 => (false, 32),
            Kind::Nat64 => (false, 64),
            Kind::Nat128 => (false, 128),
            Kind::Int8 => (true, 8),
            Kind::Int16 => (true, 16),
            Kind::Int32 => (true, 32),
            Kind::Int64 => (true, 64),
            Kind::Int128 => (true, 128),
            _ => return Err(fail()),
        };

        if signed {
            let boundary = 1_u128 << (bits - 1);
            if magnitude > boundary - u128::from(!negative) {
                return Err(fail());
            }
            let value = if negative {
                if magnitude == (1_u128 << 127) {
                    i128::MIN
                } else {
                    -(magnitude as i128)
                }
            } else {
                magnitude as i128
            };

            Ok(Self::Int(value))
        } else {
            if negative || (bits < 128 && magnitude >= (1_u128 << bits)) {
                return Err(fail());
            }

            Ok(Self::Nat(magnitude))
        }
    }

    pub fn description(&self) -> TokenStream {
        match self {
            Self::Nat(v) => quote!(::ic_db::LiteralDefault::Nat(#v)),
            Self::Int(v) => quote!(::ic_db::LiteralDefault::Int(#v)),
            Self::Bool(v) => quote!(::ic_db::LiteralDefault::Bool(#v)),
            Self::Text(v) => quote!(::ic_db::LiteralDefault::Text(#v)),
            Self::Blob(v) => {
                let bytes = syn::LitByteStr::new(v, proc_macro2::Span::call_site());

                quote!(::ic_db::LiteralDefault::Blob(#bytes))
            }
        }
    }

    fn value(&self) -> TokenStream {
        match self {
            Self::Nat(v) => syn::parse_str::<Expr>(&v.to_string())
                .expect("admitted integer literal")
                .into_token_stream(),
            Self::Int(v) => syn::parse_str::<Expr>(&v.to_string())
                .expect("admitted integer literal")
                .into_token_stream(),
            Self::Bool(v) => quote!(#v),
            Self::Text(v) => quote!(::std::string::String::from(#v)),
            Self::Blob(v) => {
                let bytes = syn::LitByteStr::new(v, proc_macro2::Span::call_site());

                quote!(#bytes.to_vec())
            }
        }
    }
}

pub(super) fn emit(item: &ItemStruct, fields: &[Field]) -> syn::Result<TokenStream> {
    let entity = &item.ident;
    let visibility = &item.vis;
    let input = format_ident!("{}Insert", entity);
    let authored: Vec<_> = fields
        .iter()
        .filter(|field| field.managed.is_none())
        .collect();
    let ids = authored
        .iter()
        .map(|f| syn::parse_str::<syn::Ident>(&f.name))
        .collect::<syn::Result<Vec<_>>>()?;
    let types: Vec<_> = authored.iter().map(|field| field.rust_type()).collect();
    let all_ids = fields
        .iter()
        .map(|f| syn::parse_str::<syn::Ident>(&f.name))
        .collect::<syn::Result<Vec<_>>>()?;
    let resolved = fields.iter().zip(&all_ids).map(|(field, id)| {
        if field.managed.is_some() {
            let time = quote!(_time.ok_or(::ic_db::Error::MissingOperationTime)?);
            let value = match &field.kind {
                Kind::Named(path, false) => quote!(#path { 0: #time }),
                Kind::Named(path, true) => quote!(::std::boxed::Box::new(#path { 0: #time })),
                _ => time,
            };

            return quote!(#id: #value);
        }
        let name = &field.name;
        let omitted = match &field.default {
            Some(default) => {
                let value = default.value();

                if field.optional {
                    quote!(Some(#value))
                } else {
                    value
                }
            }
            None if field.optional => quote!(None),
            None => quote!(return Err(::ic_db::Error::MissingField(#name))),
        };

        quote!(#id: match self.#id { Some(value) => value, None => #omitted })
    });

    let input_doc = format!(
        "\n{input}\nDistinguishes omitted fields from explicit values when inserting `{entity}`.\n"
    );

    Ok(quote! {
        #[doc = #input_doc]
        #visibility struct #input { #(#ids: Option<#types>,)* }
        impl #entity {
            #[doc = "Begin a typed insert input. Resolve it through Batch::insert_input."]
            #[must_use]
            pub fn insert() -> #input { #input { #(#ids: None,)* } }
        }
        impl #input {
            #(
                #[doc = "Supply this field explicitly, including optional absence."]
                #[must_use]
                pub fn #ids(mut self, value: #types) -> Self { self.#ids = Some(value); self }
            )*
        }
        impl ::ic_db::InsertInput for #input {
            type Entity = #entity;
            fn __into_entity(self, _time: Option<::ic_db::Timestamp>) -> Result<#entity, ::ic_db::Error> {
                Ok(#entity { #(#resolved,)* })
            }
        }
    })
}
