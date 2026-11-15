//! Attribute-owned enums with unit or typed payload variants.
use super::{
    field::{Field, parse_value},
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
        || !matches!(&item.fields, syn::Fields::Named(fields) if fields.named.is_empty())
    {
        return Err(fail("enum_ requires an empty, nongeneric struct body"));
    }
    let mut name = None;
    let mut variants = Vec::new();
    for meta in metas(args)? {
        if meta.path().is_ident("name") {
            once(&mut name, text(&meta)?, &meta)?;
        } else if meta.path().is_ident("variant") {
            let mut variant = None;
            let mut payload = None;
            for part in list(&meta)? {
                if part.path().is_ident("name") {
                    once(&mut variant, text(&part)?, &part)?;
                } else if part.path().is_ident("value") {
                    once(&mut payload, parse_value(&part)?, &part)?;
                } else {
                    return Err(syn::Error::new_spanned(
                        part,
                        "variant accepts name and optional value",
                    ));
                }
            }
            let payload = payload.map(|(kind, optional, many)| {
                let mut f = Field {
                    default: None,
                    managed: None,
                    name: "payload".into(),
                    kind,
                    optional,
                    many,
                    reference: None,
                };
                resolve_self(&mut f, &item.ident);
                f
            });
            variants.push((
                variant.ok_or_else(|| fail("variant requires name"))?,
                payload,
            ));
        } else {
            return Err(syn::Error::new_spanned(meta, "expected name or variant"));
        }
    }
    let name = name.unwrap_or_else(|| item.ident.to_string());
    variants.sort_by(|a, b| a.0.cmp(&b.0));
    if name.is_empty()
        || name.len() > 64
        || variants.is_empty()
        || variants.len() > 64
        || variants.iter().any(|(n, _)| n.is_empty() || n.len() > 64)
        || variants.windows(2).any(|w| w[0].0 == w[1].0)
    {
        return Err(fail(
            "enum requires a 1..64 byte name and 1..64 distinct named variants",
        ));
    }
    let mut declarations = Vec::new();
    let mut descriptions = Vec::new();
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    for (ordinal, (name, payload)) in variants.iter().enumerate() {
        let id: syn::Ident = syn::parse_str(name)?;
        let ordinal = ordinal as u8;
        if let Some(field) = payload {
            let ty = field.rust_type();
            let value = field.type_description();
            let optional = field.optional;
            let write = field.encode_with(&quote!(value));
            let read = field.read_value();
            declarations.push(quote!(#[doc="Authored payload variant."] #id(#ty)));
            descriptions.push(quote!(::ic_db::VariantSchema {name:#name,payload:Some(::ic_db::ValueSchema {value:#value,optional:#optional})}));
            writes.push(quote!(Self::#id(value) => { writer.nat8(#ordinal)?; #write; }));
            reads.push(quote!(#ordinal => Ok(Self::#id(#read))));
        } else {
            declarations.push(quote!(#[doc="Authored unit variant."] #id));
            descriptions.push(quote!(::ic_db::VariantSchema {name:#name,payload:None}));
            writes.push(quote!(Self::#id => writer.nat8(#ordinal)?));
            reads.push(quote!(#ordinal => Ok(Self::#id)));
        }
    }
    let copy = if variants.iter().all(|(_, p)| p.is_none()) {
        quote!(#[derive(Copy, PartialOrd, Ord)])
    } else {
        quote!()
    };
    let ident = item.ident;
    let index_value = if variants.iter().all(|(_, p)| p.is_none()) {
        let arms = variants.iter().enumerate().map(|(ordinal, (name, _))| {
            let variant: syn::Ident = syn::parse_str(name).expect("validated variant");
            let ordinal = ordinal as u8;
            quote!(#ident::#variant => #ordinal)
        });
        quote! {
            impl From<&#ident> for ::ic_db::IndexValue<'_> {
                fn from(value: &#ident) -> Self {
                    Self::UnitEnum {
                        definition: <#ident as ::ic_db::Value>::definition(),
                        ordinal: match value { #(#arms,)* },
                    }
                }
            }
        }
    } else {
        quote!()
    };
    let visibility = item.vis;
    let attrs = item.attrs;
    Ok(quote! {
        #(#attrs)* #copy #[derive(Clone,Debug,PartialEq,Eq)]
        #visibility enum #ident { #(#declarations,)* }
        #index_value
        impl ::ic_db::Value for #ident {
            const NAME: &'static str = #name;
            fn definition() -> &'static ::ic_db::TypeSchema {
                static DEFINITION: ::ic_db::TypeSchema = ::ic_db::TypeSchema {name:#name,body: ::ic_db::TypeBody::Enum(&[#(#descriptions),*])};
                &DEFINITION
            }
            fn encode_into(&self,writer:&mut ::ic_db::__private::Encoder) -> Result<(),::ic_db::Error> { match self {#(#writes,)*} Ok(()) }
            fn decode_from(reader:&mut ::ic_db::__private::Decoder<'_>) -> Result<Self,::ic_db::Error> { match reader.nat8()? {#(#reads,)* _=>Err(::ic_db::Error::Corrupt)} }
        }
    })
}
