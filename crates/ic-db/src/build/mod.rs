//! Host-only second stage: collect compiler-resolved entities, validate the complete
//! selected schema and emit its typed assembly for the target build.
//!
//! Adapted from IcyDB 6d421e3's constructor registration and graph sealing. No
//! registration state or constructor dependency is included in Wasm builds.

use crate::{EntitySchema, Schema};
use std::{fmt::Write, sync::Mutex};

#[doc(hidden)]
pub use ctor as __ctor;

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
    /// The collected schema violates the ordinary product admission rules.
    Schema(crate::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
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
        let schema = Schema::new(
            &selected
                .iter()
                .map(|(_, schema)| *schema)
                .collect::<Vec<_>>(),
        )
        .map_err(Error::Schema)?;
        let mut code = String::from(
            "// Generated from the validated host schema; do not edit.\n\
            fn compiled() -> ::ic_db::Schema {\n\
            ::std::thread_local! { static SCHEMA: ::ic_db::Schema = {\n\
            let schema = ::ic_db::Schema::new(&[\n",
        );
        for (path, _) in selected {
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

/// Generate the target schema from a Cargo build script.
/// In the package-root `build.rs`, use `ic_db::build_schema!(src::schema);`.
/// To call generation alongside other build steps, use
/// `ic_db::build_schema!(src::schema => build_db_schema);` and call `build_db_schema()?`.
/// Both forms use ordinary module discovery and expose the schema as `crate::schema`
/// for imports shared with the target. A script beside the declarations can use
/// `ic_db::build_schema!(schema);`. Enable IC DB's `build` feature in build-dependencies.
#[macro_export]
macro_rules! build_schema {
    (src::$schema:ident) => {
        $crate::build_schema!(src::$schema => main);
    };
    (src::$schema:ident => $function:ident) => {
        mod src {
            pub mod $schema;
        }
        pub use self::src::$schema;
        $crate::build_schema!(@function $function, "::src::", $schema);
    };
    ($schema:ident) => {
        mod $schema;
        $crate::build_schema!(@function main, "::", $schema);
    };
    (@function $function:ident, $prefix:literal, $schema:ident) => {
        fn $function() -> ::std::result::Result<(), ::std::boxed::Box<dyn ::std::error::Error>> {
            $crate::build::write_schema(
                ::std::concat!(::std::module_path!(), $prefix, ::std::stringify!($schema)),
                ::std::concat!("crate::", ::std::stringify!($schema)),
            )
        }
    };
}

/// Generate the selected module and write the target assembly to Cargo's `OUT_DIR/schema.rs`.
/// Cargo's default package change tracking also covers nested schema source files.
/// # Errors
/// Returns schema generation errors, a missing output directory, or a file write error.
pub fn write_schema(
    source_module: &str,
    target_module: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let assembly = generate(source_module, target_module)?;
    let out = std::env::var_os("OUT_DIR").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Cargo did not set OUT_DIR")
    })?;
    std::fs::write(std::path::PathBuf::from(out).join("schema.rs"), assembly)?;
    Ok(())
}

#[cfg(test)]
mod tests;
