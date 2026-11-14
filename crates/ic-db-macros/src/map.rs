//! Named maps with scalar keys; ordinary field rules own value composition.
use super::{
    field::{Field, Kind, parse_value},
    list, metas, once,
    record::resolve_self,
    text,
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
        return Err(fail("map requires an empty, nongeneric struct body"));
    }
    let (mut name, mut key, mut value, mut max) = (None, None, None, None);
    for meta in metas(args)? {
        if meta.path().is_ident("name") {
            once(&mut name, text(&meta)?, &meta)?;
        } else if meta.path().is_ident("key") {
            once(&mut key, list(&meta)?, &meta)?;
        } else if meta.path().is_ident("value") {
            once(&mut value, parse_value(&meta)?, &meta)?;
        } else if meta.path().is_ident("max_items") {
            once(&mut max, meta.clone(), &meta)?;
        } else {
            return Err(syn::Error::new_spanned(
                meta,
                "map accepts name, key, value and max_items",
            ));
        }
    }
    let key = key.ok_or_else(|| fail("map requires key(...)"))?;
    let max = max.ok_or_else(|| fail("map requires max_items"))?;
    let (kind, optional, max) =
        parse_value(&syn::parse_quote!(value(many, #max, item(#(#key),*))))?;
    if matches!(kind, Kind::Named(..)) {
        return Err(fail("map keys currently require an ordered scalar"));
    }
    let max = max.expect("many requires max_items");
    let key = Field {
        default: None,
        managed: None,
        name: "key".into(),
        kind,
        optional,
        many: None,
        reference: None,
    };
    let (kind, optional, many) = value.ok_or_else(|| fail("map requires value(...)"))?;
    let mut value = Field {
        default: None,
        managed: None,
        name: "value".into(),
        kind,
        optional,
        many,
        reference: None,
    };
    resolve_self(&mut value, &item.ident);
    let name = name.unwrap_or_else(|| item.ident.to_string());
    if name.is_empty() || name.len() > 64 {
        return Err(fail("map name must contain 1..64 UTF-8 bytes"));
    }
    let kt = key.rust_type();
    let vt = value.rust_type();
    let kd = key.type_description();
    let vd = value.type_description();
    let kw = key.encode_with(&quote!(key));
    let vw = value.encode_with(&quote!(value));
    let kr = key.read_result();
    let vr = value.read_result();
    let ident = item.ident;
    let visibility = item.vis;
    let attrs = item.attrs;
    Ok(quote! {
        #(#attrs)*
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        #visibility struct #ident(#[doc="Authored bounded map."] pub ::std::collections::BTreeMap<#kt,#vt>);
        impl ::ic_db::Value for #ident {
            const NAME: &'static str = #name;
            fn definition() -> &'static ::ic_db::TypeSchema {
                static DEFINITION: ::ic_db::TypeSchema = ::ic_db::TypeSchema {
                    name: #name, body: ::ic_db::TypeBody::Map {
                        key: #kd, value: ::ic_db::ValueSchema { value: #vd, optional: #optional }, max_items: #max
                    }
                }; &DEFINITION
            }
            fn encode_into(&self, writer: &mut ::ic_db::__private::Encoder) -> Result<(), ::ic_db::Error> {
                writer.map(&self.0, #max, |writer, key, value| { #kw; #vw; Ok(()) })
            }
            fn decode_from(reader: &mut ::ic_db::__private::Decoder<'_>) -> Result<Self, ::ic_db::Error> {
                reader.map(#max, |reader| #kr, |reader| #vr).map(Self)
            }
        }
    })
}
