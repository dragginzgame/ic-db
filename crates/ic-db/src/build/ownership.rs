//! Validate typed ownership and emit the target roster for each canister.

use super::{Error, Registry, StoreDeclaration};
use std::fmt::Write;

type Owners = (Vec<&'static str>, Vec<StoreDeclaration>);

impl Registry {
    pub(super) fn ownership(&self, subtree: &str) -> Result<Owners, Error> {
        let mut canisters: Vec<_> = self
            .canisters
            .iter()
            .copied()
            .filter(|path| path.starts_with(subtree))
            .collect();
        canisters.sort_unstable();
        if canisters.is_empty() {
            return Err(Error::Ownership(format!("{subtree} declares no canisters")));
        }
        let mut stores: Vec<_> = self
            .stores
            .iter()
            .copied()
            .filter(|store| store.path.starts_with(subtree))
            .collect();
        stores.sort_unstable_by_key(|store| (store.canister, store.key, store.path));
        for store in &stores {
            if !canisters.contains(&store.canister) {
                return Err(Error::Ownership(format!(
                    "{} names missing canister {}",
                    store.path, store.canister
                )));
            }
            ic_storage::StableKey::parse(store.key).map_err(|e| {
                Error::Ownership(format!(
                    "{} has invalid stable key {}: {e}",
                    store.path, store.key
                ))
            })?;
        }
        for (left, right) in stores.iter().zip(stores.iter().skip(1)) {
            if left.canister == right.canister && left.key == right.key {
                return Err(Error::Ownership(format!(
                    "{} and {} both declare {} in {}",
                    left.path, right.path, left.key, left.canister
                )));
            }
        }
        for declaration in &self.nodes {
            let entity = declaration.path;
            let owner = declaration.store;
            if entity.starts_with(subtree) && !stores.iter().any(|s| s.path == owner) {
                return Err(Error::Ownership(format!(
                    "{entity} names missing store {owner}"
                )));
            }
        }

        Ok((canisters, stores))
    }

    pub(super) fn owned_assembly(
        &mut self,
        source: &str,
        target: &str,
        module: &str,
    ) -> Result<String, Error> {
        let prefix = format!("::{module}::");
        let mut members = std::collections::BTreeMap::<String, Vec<String>>::new();

        let subtree = format!("{source}::");
        let mut include = |path: &str| {
            let name = path.rsplit("::").next().unwrap_or(path);
            members
                .entry(name.to_owned())
                .or_default()
                .push(format!("{prefix}{path}"));
        };
        let qualified = |path: &str| {
            format!(
                "{target}::{}",
                path.strip_prefix(&subtree).expect("selected declaration")
            )
        };
        let (canisters, stores) = self.ownership(&subtree)?;
        if let Some(first) = stores.first() {
            self.prepare(source, target)
                .map_err(|e| Error::Ownership(format!("{}: {e}", first.path)))?;
        }
        let mut assembly = String::new();
        for canister in canisters {
            include(canister.strip_prefix(&subtree).unwrap());
            let mut entries = String::new();
            let canister_path = qualified(canister);
            for (slot, declaration) in stores.iter().filter(|s| s.canister == canister).enumerate()
            {
                let StoreDeclaration {
                    path: store, key, ..
                } = *declaration;
                include(store.strip_prefix(&subtree).unwrap());
                let store_path = qualified(store);
                let code = self
                    .assembly(source, target, Some(store), |path| {
                        include(path);
                        writeln!(assembly,
                        "const _: () = {{ fn member<E: ::ic_db::Entity<Store = {store_path}>>() {{}}
                         let _ = member::<{target}::{path}>; }};"
                    )
                        .unwrap();
                    })
                    .map_err(|e| Error::Ownership(format!("{store}: {e}")))?;
                writeln!(assembly, "impl {store_path} {{ {code} }}").unwrap();
                writeln!(assembly,
                    "const _: () = {{ fn owner<S: ::ic_db::Store<Canister = {canister_path}>>() {{}}
                     let _ = owner::<{store_path}>;
                     assert!(matches!(<{store_path} as ::ic_db::Store>::KEY.as_bytes(), {:?}),
                         \"store key differs from host\"); }};", key.as_bytes()
                )
                .unwrap();
                writeln!(
                    assembly,
                    "impl ::ic_db::MountedStore for {store_path} {{ const SLOT: usize = {slot}; }}"
                )
                .unwrap();
                writeln!(entries,
                    "::ic_db::CompiledStore {{ key: <{store_path} as ::ic_db::Store>::KEY, schema: {store_path}::compiled }},"
                ).unwrap();
            }
            if entries.is_empty() {
                return Err(Error::Ownership(format!("{canister} owns no stores")));
            }
            writeln!(assembly,"impl ::ic_db::CompiledCanister for {canister_path} {{ const STORES: &'static [::ic_db::CompiledStore] = &[{entries}]; }}").unwrap();
        }

        println!("cargo::rustc-env=IC_DB_SCHEMA_ROOT={prefix}");
        for (name, paths) in members {
            println!(
                "cargo::rustc-env=IC_DB_SCHEMA_MEMBER_{name}={}",
                paths.join("|")
            );
        }

        self.sealed = true;
        Ok(assembly)
    }
}
