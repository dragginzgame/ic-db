//! Host-only second stage: collect compiler-resolved entities, validate the complete
//! selected schema and emit its typed assembly for the target build.
//!
//! Adapted from IcyDB 6d421e3's constructor registration and graph sealing. No
//! registration state or constructor dependency is included in Wasm builds.

mod ownership;
#[cfg(test)]
mod tests;

use crate::{EntitySchema, Schema};
use std::{fmt::Write, sync::Mutex};

#[doc(hidden)]
pub use ctor as __ctor;

/// Emit the linker constructor for compiled host schema builds.
/// Bind the typed function outside ctor so its platform branches carry only
/// generated tokens. The authored identifier keeps its own editor diagnostics.
#[doc(hidden)]
#[macro_export]
macro_rules! __register_schema {
    ($entity:ident) => {
        $crate::__register_schema!(@call $crate::__private::__entity::<$entity>);
    };
    (store $name:ident) => {
        $crate::__register_schema!(@call $crate::__private::__store::<$name>);
    };
    (canister $name:ident) => {
        $crate::__register_schema!(@call $crate::__private::__canister::<$name>);
    };
    (@call $function:expr) => {
        const _: () = {
            const REGISTER: fn() = $function;
            $crate::__private::__ctor::declarative::ctor! {
                #[ctor(unsafe, anonymous)]
                fn register() { REGISTER(); }
            }
        };
    };
}

/// Refusal to generate a schema from host declarations.
#[derive(Debug)]
pub enum Error {
    /// Host collection lock was poisoned by an earlier failure.
    Poisoned,
    /// A constructor attempted registration after successful generation.
    LateRegistration,
    /// Two declarations have the same fully resolved Rust path.
    DuplicatePath(&'static str),
    /// The requested source/target module path is empty.
    InvalidModule,
    /// No declarations belong to the requested source module.
    EmptyModule,
    /// A declared reference has no target in the selected schema or has a different key type.
    InvalidReference(String),
    /// Invalid ownership or stable allocation key.
    Ownership(String),
    /// The collected schema violates the ordinary product admission rules.
    Schema(crate::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicatePath(path) => write!(f, "duplicate declaration path: {path}"),
            Self::InvalidReference(message) | Self::Ownership(message) => f.write_str(message),
            _ => write!(f, "{self:?}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Schema(error) => Some(error),
            _ => None,
        }
    }
}

///
/// EntityDeclaration
/// Associates a resolved host path with its owning store and executable schema.
///
struct EntityDeclaration {
    path: &'static str,
    store: &'static str,
    schema: &'static EntitySchema,
}

///
/// StoreDeclaration
/// Binds a store path to its canister and stable allocation key.
///
#[derive(Clone, Copy)]
struct StoreDeclaration {
    path: &'static str,
    canister: &'static str,
    key: &'static str,
}

#[derive(Default)]
struct Registry {
    nodes: Vec<EntityDeclaration>,
    stores: Vec<StoreDeclaration>,
    canisters: Vec<&'static str>,
    sealed: bool,
    late: bool,
}
impl Registry {
    fn register(&mut self, path: &'static str, store: &'static str, schema: &'static EntitySchema) {
        self.collect(|registry| {
            registry.nodes.push(EntityDeclaration {
                path,
                store,
                schema,
            })
        });
    }

    fn collect(&mut self, add: impl FnOnce(&mut Self)) {
        if self.sealed {
            self.late = true;
        } else {
            add(self);
        }
    }

    fn generate(&mut self, source: &str, target: &str) -> Result<String, Error> {
        let prefix = format!("{source}::");
        if !self
            .nodes
            .iter()
            .any(|entity| entity.path.starts_with(&prefix))
        {
            return Err(Error::EmptyModule);
        }
        let (_, stores) = self.ownership(&prefix)?;
        if stores.len() != 1 {
            return Err(Error::Ownership(format!(
                "{source} requires one local store; use the producer-owned build for multiple stores"
            )));
        }
        self.prepare(source, target)?;
        let code = self.assembly(source, target, None, |_| {})?;
        self.sealed = true;
        Ok(code)
    }

    fn prepare(&mut self, source: &str, target: &str) -> Result<(), Error> {
        if self.late {
            return Err(Error::LateRegistration);
        }
        if source.is_empty() || target.is_empty() {
            return Err(Error::InvalidModule);
        }
        self.nodes.sort_by_key(|entity| entity.path);
        if let Some(pair) = self
            .nodes
            .windows(2)
            .find(|pair| pair[0].path == pair[1].path)
        {
            return Err(Error::DuplicatePath(pair[0].path));
        }
        Ok(())
    }

    fn assembly(
        &self,
        source: &str,
        target: &str,
        store: Option<&str>,
        mut member: impl FnMut(&str),
    ) -> Result<String, Error> {
        let prefix = format!("{source}::");
        let mut selected: Vec<_> = self
            .nodes
            .iter()
            .filter(|entity| store.is_none_or(|store| store == entity.store))
            .filter_map(|entity| {
                entity
                    .path
                    .strip_prefix(&prefix)
                    .map(|suffix| (suffix, entity.schema))
            })
            .collect();
        if selected.is_empty() {
            return Err(Error::EmptyModule);
        }
        selected.sort_by_key(|(_, schema)| schema.name);
        if let Some(pair) = selected
            .windows(2)
            .find(|pair| pair[0].1.name == pair[1].1.name)
        {
            let [(left, entity), (right, _)] = pair else {
                unreachable!()
            };
            return Err(Error::Ownership(format!(
                "{left} and {right} both declare entity {}",
                entity.name
            )));
        }
        let mut reference_error = None;
        let schema = Schema::admit_schema(
            &selected
                .iter()
                .map(|(_, schema)| *schema)
                .collect::<Vec<_>>(),
            |entity, field, target, key| {
                reference_error = Some(Error::InvalidReference(match key {
                    Some((key, expected)) => format!(
                        "{entity}.{} has type {:?} but references {target}.{key}, which requires {expected:?}", field.name, field.value
                    ),
                    None => format!(
                        "{}.{} references {target}, which is missing from the selected schema module {source}", entity, field.name
                    ),
                }));
            },
        )
        .map_err(|error| reference_error.unwrap_or(Error::Schema(error)))?;
        let visibility = if store.is_some() { "pub " } else { "" };
        let mut code = String::from(
            "// Generated from the validated host schema; do not edit.\n\
            /// Obtain this root’s validated executable schema.\n",
        );
        code.push_str(visibility);
        code.push_str(
            "fn compiled() -> ::ic_db::Schema {\n\
            ::std::thread_local! { static SCHEMA: ::ic_db::Schema = {\n\
            let schema = ::ic_db::Schema::new(&[\n",
        );
        for (path, _) in selected {
            member(path);
            // Paths came from module_path!/stringify! in compiled declarations.
            writeln!(code, "<{target}::{path} as ::ic_db::Entity>::SCHEMA,").unwrap();
        }
        code.push_str("] ).expect(\"generated schema admission failed\");\n");
        writeln!(
            code,
            "assert_eq!(schema.fingerprint(), {:?}, \"host/target schema mismatch\");",
            schema.fingerprint()
        )
        .unwrap();
        code.push_str("schema\n}; }\nSCHEMA.with(::std::clone::Clone::clone)\n}\n");
        Ok(code)
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    nodes: Vec::new(),
    stores: Vec::new(),
    canisters: Vec::new(),
    sealed: false,
    late: false,
});

/// Register a compiler-produced declaration before the host build's main function.
/// # Panics
/// A poisoned host registration lock aborts the build.
#[doc(hidden)]
pub fn __entity<E: crate::Entity>() {
    use crate::Store;
    let mut registry = REGISTRY.lock().expect("schema registration lock poisoned");
    registry.register(E::declaration_path(), E::Store::DECLARATION, E::SCHEMA);
}

#[doc(hidden)]
pub fn __store<S: crate::Store>() {
    use crate::Canister;
    let mut registry = REGISTRY.lock().expect("schema registration lock poisoned");
    registry.collect(|registry| {
        registry.stores.push(StoreDeclaration {
            path: S::DECLARATION,
            canister: S::Canister::DECLARATION,
            key: S::KEY,
        })
    });
}

#[doc(hidden)]
pub fn __canister<C: crate::Canister>() {
    let mut registry = REGISTRY.lock().expect("schema registration lock poisoned");
    registry.collect(|registry| registry.canisters.push(C::DECLARATION));
}

/// Build the complete schema declared under `src/<module>` in its owning crate.
/// Use `build_schema!(model)` in the package-root build script and
/// `ic_db::include_schema!()` in the library to export typed canister rosters and store constructors.
/// The no-argument form retains the local canister schema convention.
/// For a colocated script, `self::schema` selects its sibling module.
#[macro_export]
macro_rules! build_schema {
    () => { $crate::build_schema!(src::schema => main); };
    (=> $function:ident) => { $crate::build_schema!(src::schema => $function); };
    (src::$schema:ident) => { $crate::build_schema!(src::$schema => main); };
    (self::$schema:ident) => { $crate::build_schema!(self::$schema => main); };
    (self::$schema:ident => $function:ident) => {
        mod $schema;
        $crate::build_schema!(@function $schema, $function, "::", false);
    };
    (src::$schema:ident => $function:ident) => {
        $crate::build_schema!(@src $schema, $function, false);
    };
    ($schema:ident) => {
        $crate::build_schema!(@src $schema, main, true);
    };
    (@src $schema:ident, $function:ident, $owned:literal) => {
        mod src { pub mod $schema; }
        pub use self::src::$schema;
        $crate::build_schema!(@function $schema, $function, "::src::", $owned);
    };
    (@function $schema:ident, $function:ident, $prefix:literal, $owned:literal) => {
        fn $function() -> ::std::result::Result<(), ::std::boxed::Box<dyn ::std::error::Error>> {
            $crate::__private::__build_schema(
                ::std::concat!(::std::module_path!(), $prefix, ::std::stringify!($schema)),
                ::std::stringify!($schema), $owned,
            )
        }
    };
}

/// Cross-crate expansion support for `build_schema!`.
/// # Errors
/// Returns graph admission and output errors.
#[doc(hidden)]
pub fn __build_schema(
    source: &str,
    module: &str,
    owned: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = format!("crate::{module}");
    let mut registry = REGISTRY.lock().map_err(|_| Error::Poisoned)?;
    let assembly = if owned {
        registry.owned_assembly(source, &target, module)?
    } else {
        registry.generate(source, &target)?
    };
    write_assembly(&assembly)
}

fn write_assembly(assembly: &str) -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::var_os("OUT_DIR").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Cargo did not set OUT_DIR")
    })?;
    std::fs::write(std::path::PathBuf::from(out).join("schema.rs"), assembly)?;

    Ok(())
}
