//! Named scalar/unit-enum sets; field parsing and codecs retain their existing owner.

use super::{
    declaration,
    field::{Field, parse_max_items},
    wrapped::{WrapperOptions, options},
};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let item = declaration(item, "set")?;
    let WrapperOptions {
        name,
        kind,
        extra: max,
    } = options(args, &item.ident, "set", "max_items", parse_max_items)?;
    let max = max.ok_or_else(|| syn::Error::new_spanned(&item.ident, "set requires max_items"))?;
    let field = Field::nested("item", kind, false, None);
    let ty = field.rust_type();
    let description = field.type_description();
    let write = field.encode_with(&quote!(value));
    let read = field.read_result();
    let ident = item.ident;
    let visibility = item.vis;
    let attrs = crate::schema_attrs(&item.attrs)?;

    Ok(quote! {
        #attrs
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        #visibility struct #ident(#[doc="Authored bounded set."] pub ::std::collections::BTreeSet<#ty>);
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
