//! Typed input records for complete unique index lookups.
use super::Field;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeSet;
use syn::{Ident, ItemStruct};

#[derive(Default)]
pub(super) struct Keys {
    pub declarations: Vec<TokenStream>,
    components: BTreeSet<Vec<String>>,
    names: BTreeSet<String>,
}

impl Keys {
    pub fn add(
        &mut self,
        item: &ItemStruct,
        index: &str,
        ids: &[Ident],
        authored: &[Field],
        fields: &[String],
    ) {
        // Equivalent named indexes share a lookup shape. Indexes arrive in logical name order.
        if !self.components.insert(fields.to_vec()) {
            return;
        }
        let entity = &item.ident;
        let suffix = fields
            .iter()
            .map(|field| {
                field
                    .trim_start_matches("r#")
                    .split('_')
                    .filter_map(|word| {
                        let mut chars = word.chars();
                        Some(
                            chars
                                .next()?
                                .to_uppercase()
                                .chain(chars)
                                .collect::<String>(),
                        )
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("And");
        let mut name = format!("{entity}By{suffix}");
        // Distinct legal field spellings can have the same Rust type capitalization.
        while !self.names.insert(name.clone()) {
            name.push('_');
        }
        let key = format_ident!("{name}");
        let vis = &item.vis;
        let components: Vec<_> = fields
            .iter()
            .map(|name| {
                authored
                    .iter()
                    .find(|f| &f.name == name)
                    .expect("admitted index field")
            })
            .collect();
        let types = components.iter().map(|field| field.item_type());
        let operands = components
            .iter()
            .zip(ids)
            .map(|(field, id)| field.index_operand(id, false));
        let doc = format!("Named key for the unique index `{index}` on `{entity}`.");
        self.declarations.push(quote! {
            #[doc = #doc]
            #vis struct #key { #(#[doc = "Present lookup component."] pub #ids: #types,)* }
            impl ::ic_db::UniqueKey for #key {
                type Entity = #entity;
                const INDEX: &'static str = #index;
                fn components(&self) -> impl ::core::convert::AsRef<[::ic_db::IndexValue<'_>]> {
                    let row = self;
                    [#(#operands),*]
                }
            }
        });
    }
}
