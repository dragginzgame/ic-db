//! Typed store and canister declarations.
use super::{Tokens, declaration, membership, metas, once, owner_path, text};
use quote::quote;

pub(super) fn expand(args: Tokens, item: Tokens, store: bool) -> syn::Result<Tokens> {
    let item = declaration(item, if store { "store" } else { "canister" })?;
    let name = &item.ident;
    let membership = membership(name);
    let path = quote!(concat!(module_path!(), "::", stringify!(#name)));
    let mut owner = None;
    let mut key = None;
    for meta in metas(args)? {
        if store && meta.path().is_ident("canister") {
            once(&mut owner, owner_path(&meta)?, &meta)?;
        } else if store && meta.path().is_ident("key") {
            once(&mut key, text(&meta)?, &meta)?;
        } else {
            return Err(syn::Error::new_spanned(meta, "unknown ownership attribute"));
        }
    }
    if store {
        let owner =
            owner.ok_or_else(|| syn::Error::new_spanned(name, "store requires canister"))?;
        let key = key.ok_or_else(|| syn::Error::new_spanned(name, "store requires key"))?;
        Ok(quote!(#item #membership impl ::ic_db::Store for #name {
            type Canister = #owner;
            const KEY: &'static str = #key;
            const DECLARATION: &'static str = #path;
        }
        ::ic_db::__register_schema!(store #name);))
    } else {
        Ok(quote!(#item #membership impl ::ic_db::Canister for #name {
            const DECLARATION: &'static str = #path;
        }
        ::ic_db::__register_schema!(canister #name);))
    }
}
