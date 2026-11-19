//! Validate typed ownership and emit the target roster for each canister.

use super::{Error, Registry};
use std::fmt::Write;

type StoreDeclaration = (&'static str, &'static str, &'static str);
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
            .filter(|(path, _, _)| path.starts_with(subtree))
            .collect();
        stores.sort_unstable_by_key(|&(path, owner, key)| (owner, key, path));
        for &(path, owner, key) in &stores {
            if !canisters.contains(&owner) {
                return Err(Error::Ownership(format!(
                    "{path} names missing canister {owner}"
                )));
            }
            ic_storage::StableKey::parse(key).map_err(|e| {
                Error::Ownership(format!("{path} has invalid stable key {key}: {e}"))
            })?;
        }
        for pair in stores.windows(2) {
            let (left, owner, key) = pair[0];
            let (right, other, next) = pair[1];
            if owner == other && key == next {
                return Err(Error::Ownership(format!(
                    "{left} and {right} both declare {key} in {owner}"
                )));
            }
        }
        for &(entity, owner, _) in &self.nodes {
            if entity.starts_with(subtree) && !stores.iter().any(|s| s.0 == owner) {
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
        let mut assembly = String::new();
        for canister in canisters {
            include(canister.strip_prefix(&subtree).unwrap());
            let mut entries = String::new();
            let canister_path = qualified(canister);
            for &(store, _, key) in stores.iter().filter(|s| s.1 == canister) {
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
