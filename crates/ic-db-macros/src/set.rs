//! Named scalar/unit-enum sets; field parsing and codecs retain their existing owner.

use super::{
    declaration,
    field::{Field, parse_value},
    once, text,
};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let item = declaration(item, "set")?;
    let mut name = None;
    let mut parts = Vec::new();
    let (wire, options) = crate::declaration_options(args)?;
    for meta in options {
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
    let name = super::declaration_name(name, &item.ident, "set")?;
    let max = max.expect("many requires max_items");
    let field = Field::nested("item", kind, optional, None);
    let ty = field.rust_type();
    let description = field.type_description();
    let write = field.encode_with(&quote!(value));
    let read = field.read_result();
    let ident = item.ident;
    let visibility = item.vis;
    let attrs = crate::schema_attrs(&item.attrs, false)?;
    let projection = crate::wire_as(wire, &item.attrs, &ident, &quote!(::std::vec::Vec<#ty>))?;

    Ok(quote! {
        #attrs
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        #visibility struct #ident(#[doc="Authored bounded set."] pub ::std::collections::BTreeSet<#ty>);
        #projection
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
