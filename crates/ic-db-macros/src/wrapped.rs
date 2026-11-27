//! Named lists and primitive newtypes reuse field descriptors and codecs.

use super::{
    declaration,
    field::{Field, Kind, parse_item, parse_max_items},
    once, text,
};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn expand(args: TokenStream, item: TokenStream, many: bool) -> syn::Result<TokenStream> {
    let label = if many { "list" } else { "newtype" };
    let mut item = declaration(item, label)?;
    let mut name = None;
    let mut validation = None;
    let mut kind = None;
    let mut bound = None;
    let (wire, options) = crate::declaration_options(args)?;
    for meta in options {
        if meta.path().is_ident("name") {
            once(&mut name, text(&meta)?, &meta)?;
        } else if !many && meta.path().is_ident("validate") {
            let rules = super::meta_items(&meta)?;
            if rules.is_empty() {
                return Err(syn::Error::new_spanned(
                    meta,
                    "expected at least one validation rule",
                ));
            }
            once(&mut validation, rules, &meta)?;
        } else if meta.path().is_ident("item") {
            once(&mut kind, parse_item(&meta)?, &meta)?;
        } else if many && meta.path().is_ident("max_items") {
            once(&mut bound, parse_max_items(&meta)?, &meta)?;
        } else {
            return Err(syn::Error::new_spanned(
                meta,
                format!("unsupported {label} option"),
            ));
        }
    }
    let name = super::declaration_name(name, &item.ident, label)?;
    let kind = kind.ok_or_else(|| syn::Error::new_spanned(&item.ident, "expected item(...)"))?;
    if many && bound.is_none() {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "list requires max_items",
        ));
    }
    if !many && matches!(kind, Kind::Named(..)) {
        return Err(syn::Error::new_spanned(
            item.ident,
            "newtype requires a primitive item",
        ));
    }
    let ident = item.ident;
    let mut field = Field::nested("value", kind, false, bound);
    field.resolve_self(&ident);
    let checked_principal = validation.is_some() && matches!(field.kind, Kind::Principal);
    let (ty, body, write, read, inner_vis, constructor) = if let Some(rules) = validation {
        let mut non_anonymous = None;
        let mut non_blank = None;
        let mut max_chars = None;
        for rule in rules {
            match (&field.kind, &rule) {
                (Kind::Principal, syn::Meta::Path(path)) if path.is_ident("non_anonymous") => {
                    once(&mut non_anonymous, (), &rule)?
                }
                (Kind::Text(_), syn::Meta::Path(path)) if path.is_ident("non_blank") => {
                    once(&mut non_blank, (), &rule)?
                }
                (Kind::Text(_), _) if rule.path().is_ident("max_chars") => {
                    let syn::Lit::Int(value) = super::literal(&rule)? else {
                        return Err(syn::Error::new_spanned(
                            rule,
                            "expected a Unicode scalar count",
                        ));
                    };
                    once(&mut max_chars, value.base10_parse::<u32>()?, &rule)?;
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        rule,
                        "unsupported validation rule for this primitive",
                    ));
                }
            }
        }
        let (storage, rule, write, read, unwrap) = match field.kind {
            Kind::Text(bytes) => {
                let non_blank = non_blank.is_some();
                let chars = max_chars.unwrap_or(bytes);
                let limit = max_chars.map_or_else(|| quote!(None), |v| quote!(Some(#v)));
                (
                    quote!(::ic_db::__private::CheckedText<#bytes, #non_blank, #chars>),
                    quote!(::ic_db::NewtypeRule::Text { max_bytes: #bytes, non_blank: #non_blank, max_chars: #limit }),
                    quote!(writer.bytes(self.0.as_str().as_bytes())?),
                    quote!(reader.text(#bytes as usize).and_then(|v| v.try_into().map_err(|_| ::ic_db::Error::Corrupt))),
                    quote!(value.0.into_string()),
                )
            }
            Kind::Principal => (
                quote!(::ic_db::__private::CheckedPrincipal),
                quote!(::ic_db::NewtypeRule::NonAnonymous),
                quote!(writer.principal(&::ic_db::Principal::from(self.0))?),
                quote!(
                    reader
                        .principal()
                        .and_then(|v| v.try_into().map_err(|_| ::ic_db::Error::Corrupt))
                ),
                quote!(value.0.into()),
            ),
            _ => unreachable!("all rules were checked against their primitive"),
        };
        let inner = field.rust_type();
        (
            storage,
            quote!(::ic_db::TypeBody::Checked(#rule)),
            write,
            read,
            quote!(),
            quote! {
                impl ::core::convert::TryFrom<#inner> for #ident {
                    type Error = ::ic_db::Error;
                    fn try_from(value: #inner) -> Result<Self, Self::Error> { value.try_into().map(Self) }
                }
                impl From<#ident> for #inner {
                    fn from(value: #ident) -> Self { #unwrap }
                }
            },
        )
    } else {
        let desc = field.type_description();
        (
            field.rust_type(),
            quote!(::ic_db::TypeBody::Wrapped(#desc)),
            field.encode_with(&quote!(&self.0)),
            field.read_result(),
            quote!(pub),
            quote!(),
        )
    };
    let vis = item.vis;
    let projection = crate::wire_as(wire && !many, &item.attrs, &ident, &ty)?;
    if !many {
        default_transparent(&mut item.attrs)?;
    }
    let attrs = crate::schema_attrs(&item.attrs, wire && many)?;
    let order = (!many).then(|| quote!(#[derive(PartialOrd, Ord)]));
    let operand = if many {
        None
    } else {
        let inner = match field.kind {
            Kind::Text(_) => quote!(value.0.as_str()),
            Kind::Blob(_) => quote!(value.0.as_slice()),
            Kind::Principal if checked_principal => quote!(::ic_db::Principal::from(value.0)),
            _ => quote!(value.0),
        };

        Some(quote! {
            impl<'a> From<&'a #ident> for ::ic_db::IndexValue<'a> {
                fn from(value: &'a #ident) -> Self { Self::from(#inner) }
            }
        })
    };

    let owned_operand = checked_principal.then(|| {
        quote! {
            impl From<#ident> for ::ic_db::IndexValue<'_> {
                fn from(value: #ident) -> Self { Self::from(::ic_db::Principal::from(value)) }
            }
        }
    });
    Ok(quote! {
        #attrs
        #order
        #[derive(Clone, Debug, PartialEq, Eq)]
        #vis struct #ident(#[doc="Wrapped value."] #inner_vis #ty);
        #projection
        #constructor
        impl ::ic_db::Value for #ident {
            const NAME: &'static str = #name;
            fn definition() -> &'static ::ic_db::TypeSchema {
                static DEFINITION: ::ic_db::TypeSchema = ::ic_db::TypeSchema {name:#name, body: #body};
                &DEFINITION
            }

            fn encode_into(&self, writer: &mut ::ic_db::__private::Encoder) -> Result<(), ::ic_db::Error> { #write; Ok(()) }

            fn decode_from(reader: &mut ::ic_db::__private::Decoder<'_>) -> Result<Self, ::ic_db::Error> { (#read).map(Self) }
        }
        #operand
        #owned_operand
    })
}

// An explicit Serde conversion owns the representation in place of the default.
fn default_transparent(attrs: &mut Vec<syn::Attribute>) -> syn::Result<()> {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        for option in super::meta_items(&attr.meta)? {
            if ["transparent", "from", "try_from", "into"]
                .iter()
                .any(|name| option.path().is_ident(name))
            {
                return Ok(());
            }
        }
    }
    attrs.push(syn::parse_quote!(#[serde(transparent)]));
    Ok(())
}
