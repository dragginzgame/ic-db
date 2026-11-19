//! Host-only second stage: collect compiler-resolved entities, validate the complete
//! selected schema and emit its typed assembly for the target build.
//!
//! Adapted from IcyDB 6d421e3's constructor registration and graph sealing. No
//! registration state or constructor dependency is included in Wasm builds.

#[cfg(test)]
mod tests;

use crate::{EntitySchema, Schema};
use std::{fmt::Write, sync::Mutex};

#[doc(hidden)]
pub use ctor as __ctor;

/// Emit the linker constructor for compiled host schema builds.
/// Editor analysis only needs the schema types. Expanding the linker machinery
/// there attributes ctor's inactive UEFI branch to the authored entity.
#[doc(hidden)]
#[cfg(not(rust_analyzer))]
#[macro_export]
macro_rules! __register_schema {
    ($entity:ident) => {
        const _: () = {
            $crate::build::__ctor::declarative::ctor! {
                #[ctor(unsafe, anonymous)]
                fn register() {
                    $crate::build::__register(
                        <$entity as $crate::Entity>::declaration_path(),
                        <$entity as $crate::Entity>::SCHEMA,
                    );
                }
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
    /// The collected schema violates the ordinary product admission rules.
    Schema(crate::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReference(message) => f.write_str(message),
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

#[derive(Default)]
struct Registry {
    nodes: Vec<(&'static str, &'static EntitySchema)>,
    sealed: bool,
    late: bool,
}
impl Registry {
    fn register(&mut self, path: &'static str, schema: &'static EntitySchema) {
        if self.sealed {
            self.late = true;
        } else {
            self.nodes.push((path, schema));
        }
    }

    fn generate(&mut self, source: &str, target: &str) -> Result<String, Error> {
        self.assembly(source, target, false, |_| {})
    }

    fn assembly(
        &mut self,
        source: &str,
        target: &str,
        public: bool,
        mut member: impl FnMut(&str),
    ) -> Result<String, Error> {
        if self.late {
            return Err(Error::LateRegistration);
        }
        if source.is_empty() || target.is_empty() {
            return Err(Error::InvalidModule);
        }
        self.nodes.sort_by_key(|(path, _)| *path);
        if let Some(pair) = self.nodes.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(Error::DuplicatePath(pair[0].0));
        }
        let prefix = format!("{source}::");
        let mut selected: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|(path, schema)| path.strip_prefix(&prefix).map(|suffix| (suffix, *schema)))
            .collect();
        if selected.is_empty() {
            return Err(Error::EmptyModule);
        }
        selected.sort_by_key(|(_, schema)| schema.name);
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
        let visibility = if public { "pub " } else { "" };
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
        self.sealed = true;

        Ok(code)
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    nodes: Vec::new(),
    sealed: false,
    late: false,
});

/// Register a compiler-produced declaration before the host build's main function.
/// # Panics
/// A poisoned host registration lock aborts the build.
#[doc(hidden)]
pub fn __register(path: &'static str, schema: &'static EntitySchema) {
    REGISTRY
        .lock()
        .expect("schema registration lock poisoned")
        .register(path, schema);
}

/// Generate `fn compiled() -> ic_db::Schema` for entities beneath a source module.
///
/// Use the same schema module in the host build script and target. For example,
/// map `build_script_build::schema` to `crate::schema`, write the returned Rust to
/// `OUT_DIR`, then include it in the target. Rust resolves types; this function
/// only remaps the known module root. Supply a trusted Rust target path from the
/// build script; the target compiler checks it. Runtime admission checks the host fingerprint.
/// The generated function prepares immutable executable metadata once per thread
/// and returns a clone. It never retains database admission or revision values.
/// Constructors must have completed; success seals registration for this process.
/// # Errors
/// Refuses duplicates, late registration, empty modules or invalid schemas,
/// including references whose targets were not collected in the selected module.
pub fn generate(source_module: &str, target_module: &str) -> Result<String, Error> {
    REGISTRY
        .lock()
        .map_err(|_| Error::Poisoned)?
        .generate(source_module, target_module)
}

/// Build the complete schema declared under `src/<module>` in its owning crate.
/// Use `build_schema!(model)` in the package-root build script and include
/// `OUT_DIR/schema.rs` in the library to export `compiled()`.
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
            $crate::build::__build_schema(
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
    if !owned {
        return write_assembly(&generate(source, &target)?);
    }

    let prefix = format!("::{module}::");
    let mut members = std::collections::BTreeMap::<String, Vec<String>>::new();
    let assembly =
        REGISTRY
            .lock()
            .map_err(|_| Error::Poisoned)?
            .assembly(source, &target, true, |path| {
                let name = path.rsplit("::").next().unwrap_or(path);
                members
                    .entry(name.to_owned())
                    .or_default()
                    .push(format!("{prefix}{path}"));
            })?;
    write_assembly(&assembly)?;
    println!("cargo::rustc-env=IC_DB_SCHEMA_ROOT={prefix}");
    for (name, paths) in members {
        println!(
            "cargo::rustc-env=IC_DB_SCHEMA_MEMBER_{name}={}",
            paths.join("|")
        );
    }

    Ok(())
}

fn write_assembly(assembly: &str) -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::var_os("OUT_DIR").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Cargo did not set OUT_DIR")
    })?;
    std::fs::write(std::path::PathBuf::from(out).join("schema.rs"), assembly)?;

    Ok(())
}
