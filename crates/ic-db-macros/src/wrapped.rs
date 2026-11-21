//! Named lists and primitive newtypes reuse field descriptors and codecs.

use super::{
    declaration,
    field::{Field, Kind, parse_value},
    once, text,
};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn expand(args: TokenStream, item: TokenStream, many: bool) -> syn::Result<TokenStream> {
    let label = if many { "list" } else { "newtype" };
    let item = declaration(item, label)?;
    let mut name = None;
    let mut parts = Vec::new();
    let (wire, options) = crate::declaration_options(args)?;
    for meta in options {
        if meta.path().is_ident("name") {
            once(&mut name, text(&meta)?, &meta)?;
        } else if meta.path().is_ident("item") || (many && meta.path().is_ident("max_items")) {
            parts.push(meta);
        } else {
            return Err(syn::Error::new_spanned(
                meta,
                format!("unsupported {label} option"),
            ));
        }
    }
    let name = super::declaration_name(name, &item.ident, label)?;
    let cardinality = many.then(|| quote!(many,));
    let (kind, optional, bound) = parse_value(&syn::parse_quote!(value(#cardinality #(#parts),*)))?;
    if !many && matches!(kind, Kind::Named(..)) {
        return Err(syn::Error::new_spanned(
            item.ident,
            "newtype requires a primitive item",
        ));
    }
    let ident = item.ident;
    let mut field = Field::nested("value", kind, optional, bound);
    field.resolve_self(&ident);
    let ty = field.rust_type();
    let desc = field.type_description();
    let write = field.encode_with(&quote!(&self.0));
    let read = field.read_result();
    let vis = item.vis;
    let attrs = crate::schema_attrs(&item.attrs, wire)?;
    let order = (!many).then(|| quote!(#[derive(PartialOrd, Ord)]));
    let operand = if many {
        None
    } else {
        let inner = match field.kind {
            Kind::Text(_) => quote!(value.0.as_str()),
            Kind::Blob(_) => quote!(value.0.as_slice()),
            _ => quote!(value.0),
        };

        Some(quote! {
            impl<'a> From<&'a #ident> for ::ic_db::IndexValue<'a> {
                fn from(value: &'a #ident) -> Self { Self::from(#inner) }
            }
        })
    };

    Ok(quote! {
        #attrs
        #order
        #[derive(Clone, Debug, PartialEq, Eq)]
        #vis struct #ident(#[doc="Wrapped value."] pub #ty);
        impl ::ic_db::Value for #ident {
            const NAME: &'static str = #name;
            fn definition() -> &'static ::ic_db::TypeSchema {
                static DEFINITION: ::ic_db::TypeSchema = ::ic_db::TypeSchema {name:#name, body: ::ic_db::TypeBody::Wrapped(#desc)};
                &DEFINITION
            }

            fn encode_into(&self, writer: &mut ::ic_db::__private::Encoder) -> Result<(), ::ic_db::Error> { #write; Ok(()) }

            fn decode_from(reader: &mut ::ic_db::__private::Decoder<'_>) -> Result<Self, ::ic_db::Error> { (#read).map(Self) }
        }
        #operand
    })
}
