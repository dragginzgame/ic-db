//! Named scalar sets; field parsing and element codecs retain their existing owner.
use super::{
    field::{Field, Kind, parse_value},
    metas, once, text,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemStruct;

pub(super) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let item: ItemStruct = syn::parse2(item)?;
    let fail = |message| syn::Error::new_spanned(&item.ident, message);
    if !item.generics.params.is_empty()
        || !matches!(&item.fields, syn::Fields::Named(f) if f.named.is_empty())
    {
        return Err(fail("set requires an empty, nongeneric struct body"));
    }
    let mut name = None;
    let mut parts = Vec::new();
    for meta in metas(args)? {
        if meta.path().is_ident("name") {
            once(&mut name, text(&meta)?, &meta)?;
        } else if meta.path().is_ident("item") || meta.path().is_ident("max_items") {
            parts.push(meta);
        } else {
            return Err(syn::Error::new_spanned(
                meta,
                "set accepts name, item and max_items",
            ));
        }
    }
    let (kind, optional, max) = parse_value(&syn::parse_quote!(value(many, #(#parts),*)))?;
    if matches!(kind, Kind::Named(..)) {
        return Err(fail(
            "set elements currently require an ordered scalar; named values remain unimplemented",
        ));
    }
    let name = name.unwrap_or_else(|| item.ident.to_string());
    if name.is_empty() || name.len() > 64 {
        return Err(fail("set name must contain 1..64 UTF-8 bytes"));
    }
    let max = max.expect("many requires max_items");
    let field = Field {
        default: None,
        managed: None,
        name: "item".into(),
        kind,
        optional,
        many: None,
        reference: None,
    };
    let ty = field.rust_type();
    let description = field.type_description();
    let write = field.encode_with(&quote!(value));
    let read = field.read_result();
    let ident = item.ident;
    let visibility = item.vis;
    let attrs = item.attrs;
    Ok(quote! {
        #(#attrs)*
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        #visibility struct #ident(#[doc="Authored scalar set."] pub ::std::collections::BTreeSet<#ty>);
        impl ::ic_db::Value for #ident {
            const NAME: &'static str = #name;
            fn definition() -> &'static ::ic_db::TypeSchema {
                static DEFINITION: ::ic_db::TypeSchema = ::ic_db::TypeSchema {
                    name: #name, body: ::ic_db::TypeBody::Set { item: #description, max_items: #max }
                };
                &DEFINITION
            }
            fn encode_into(&self, writer: &mut ::ic_db::__private::Encoder) -> Result<(), ::ic_db::Error> {
                writer.set(&self.0, #max, |writer, value| { #write; Ok(()) })
            }
            fn decode_from(reader: &mut ::ic_db::__private::Decoder<'_>) -> Result<Self, ::ic_db::Error> {
                reader.set(#max, |reader| #read).map(Self)
            }
        }
    })
}
