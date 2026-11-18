//! Named record boundary; field codecs and type rules stay in `field`.
use super::{
    declaration,
    field::{Field, Kind},
    meta_items, metas, once, text,
};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let item = declaration(item, "record")?;
    let fail = |message| syn::Error::new_spanned(&item.ident, message);
    let mut name = None;
    let mut fields = None;
    for meta in metas(args)? {
        if meta.path().is_ident("name") {
            once(&mut name, text(&meta)?, &meta)?;
        } else if meta.path().is_ident("fields") {
            once(
                &mut fields,
                meta_items(&meta)?
                    .iter()
                    .map(Field::parse)
                    .collect::<syn::Result<Vec<_>>>()?,
                &meta,
            )?;
        } else {
            return Err(syn::Error::new_spanned(
                meta,
                "record accepts name and fields",
            ));
        }
    }
    let name = name.unwrap_or_else(|| item.ident.to_string());
    let mut fields = fields.ok_or_else(|| fail("record requires fields"))?;
    if fields
        .iter()
        .any(|f| f.default.is_some() || f.managed.is_some())
    {
        return Err(fail(
            "defaults and managed times currently belong to entity fields, not nested record fields",
        ));
    }
    fields.sort_by(|a, b| a.name.cmp(&b.name));
    if name.is_empty()
        || name.len() > 64
        || fields.len() > 16
        || fields.windows(2).any(|w| w[0].name == w[1].name)
        || fields
            .iter()
            .any(|f| f.reference.is_some() || f.name.is_empty() || f.name.len() > 64)
    {
        return Err(fail(
            "record requires bounded distinct field names and no nested entity references",
        ));
    }
    for f in &mut fields {
        resolve_self(f, &item.ident);
    }
    let ids = fields
        .iter()
        .map(|f| syn::parse_str::<syn::Ident>(&f.name))
        .collect::<syn::Result<Vec<_>>>()?;
    let types: Vec<_> = fields.iter().map(Field::rust_type).collect();
    let descriptions: Vec<_> = fields.iter().map(Field::description).collect();
    let writes: Vec<_> = fields
        .iter()
        .zip(&ids)
        .map(|(f, id)| f.encode(id))
        .collect();
    let reads: Vec<_> = fields
        .iter()
        .zip(&ids)
        .map(|(f, id)| f.decode(id))
        .collect();
    let writer = if fields.is_empty() {
        quote!(_writer)
    } else {
        quote!(writer)
    };
    let reader = if fields.is_empty() {
        quote!(_reader)
    } else {
        quote!(reader)
    };
    let ident = item.ident;
    let visibility = item.vis;
    let attrs = item.attrs;
    Ok(quote! {
        #(#attrs)*
        #[derive(Clone, Debug, PartialEq, Eq)]
        #visibility struct #ident { #(#[doc="Authored stored field."] pub #ids: #types,)* }
        impl ::ic_db::Value for #ident {
            const NAME: &'static str = #name;
            fn definition() -> &'static ::ic_db::TypeSchema {
                static DEFINITION: ::ic_db::TypeSchema = ::ic_db::TypeSchema {name:#name, body: ::ic_db::TypeBody::Record(&[#(#descriptions),*])};
                &DEFINITION
            }

            fn encode_into(&self, #writer:&mut ::ic_db::__private::Encoder) -> Result<(),::ic_db::Error> { #(#writes)* Ok(()) }

            fn decode_from(#reader:&mut ::ic_db::__private::Decoder<'_>) -> Result<Self,::ic_db::Error> { Ok(Self { #(#reads)* }) }
        }
    })
}

pub(super) fn resolve_self(field: &mut Field, ident: &syn::Ident) {
    if let Kind::Named(path, _) = &mut field.kind
        && path.is_ident("Self")
    {
        *path = syn::parse_quote!(#ident);
    }
}
