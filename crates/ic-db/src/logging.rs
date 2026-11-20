//! Bounded operational failure logs for standalone databases and host adapters.

pub use ic_logging::{Entry, Level};
use ic_logging::{EntryInput, Filter, Limits, Log, PageRequest};
use std::{
    cell::{OnceCell, RefCell},
    num::NonZeroUsize,
};

type Sink = fn(Level, &'static str, &str);

thread_local! {
    static SINK: OnceCell<Sink> = const { OnceCell::new() };
    static LOCAL: RefCell<Log> = const { RefCell::new(Log::new(Limits {
        max_entries: NonZeroUsize::new(128).unwrap(),
        max_label_bytes: 32,
        max_message_bytes: 160,
        max_text_bytes: 8192,
    })) };
}

/// Logging has already selected a destination for this incarnation.
#[derive(Debug, Eq, PartialEq)]
pub struct AlreadyInitialized;

impl std::fmt::Display for AlreadyInitialized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IC DB logging already initialized")
    }
}
impl std::error::Error for AlreadyInitialized {}

/// Forward diagnostics to a host recorder, such as `canic::logging::record`.
/// Install once during initialization. The sink runs synchronously and must
/// return normally without calling back into the database.
/// # Errors
/// Refuses if a host sink was already installed or a standalone entry was recorded.
pub fn initialize_with_logger(
    sink: fn(Level, &'static str, &str),
) -> Result<(), AlreadyInitialized> {
    SINK.with(|slot| slot.set(sink).map_err(|_| AlreadyInitialized))
}

fn local_record(level: Level, source: &'static str, message: &str) {
    LOCAL.with_borrow_mut(|log| {
        let _ = log.append(EntryInput {
            created_at_secs: crate::clock::time_ns() / 1_000_000_000,
            level,
            source,
            topic: None,
            message,
        });
    });
}

/// Up to eight standalone entries in newest-first order.
pub struct Page {
    /// Retained diagnostic entries.
    pub entries: Vec<Entry>,
    /// Exclusive cursor for the next older page.
    pub next_before: Option<u64>,
}

/// Read the standalone recorder. A host sink owns its own read interface.
#[must_use]
pub fn page(before: Option<u64>) -> Page {
    LOCAL.with_borrow(|log| {
        const PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(8).unwrap();
        let page = log.page(
            Filter::default(),
            PageRequest {
                before,
                max_results: PAGE_SIZE,
                max_scanned: PAGE_SIZE,
            },
        );
        Page {
            entries: page.entries.into_iter().cloned().collect(),
            next_before: page.next_before,
        }
    })
}

pub(crate) fn observe<T>(
    source: &'static str,
    run: impl FnOnce() -> Result<T, crate::Error>,
) -> Result<T, crate::Error> {
    run().inspect_err(|error| {
        use crate::Error;
        let reason = match error {
            Error::BorrowConflict => "session borrow conflict",
            Error::Maintenance => "maintenance active",
            Error::InvalidTransition => "invalid maintenance transition",
            Error::InvalidJob => "invalid aggregate job",
            Error::JobCapacity => "aggregate job capacity exhausted",
            Error::ArithmeticOverflow => "arithmetic overflow",
            Error::InvalidQuery => "invalid query",
            Error::MultipleMatches => "multiple rows matched",
            Error::InvalidCursor => "invalid cursor",
            Error::RowTooLarge => "row exceeds output allowance",
            Error::MissingOperationTime => "missing operation time",
            Error::ManagedField => "managed field assignment",
            Error::TimestampRegression => "timestamp regression",
            Error::MissingField(_) => "missing required field",
            Error::InvalidSchema => "invalid schema",
            Error::InvalidOperand => "invalid operand",
            Error::UnknownEntity => "unknown entity",
            Error::IncompatibleSchema => "incompatible stored schema",
            Error::Corrupt => "inconsistent stored data",
            Error::Exists => "primary key already exists",
            Error::Missing => "primary key missing",
            Error::DuplicateTarget => "duplicate batch target",
            Error::UniqueViolation => "unique constraint violation",
            Error::ReferenceViolation => "reference constraint violation",
            Error::Limit => "resource limit exceeded",
        };
        let level = if matches!(error, Error::Corrupt | Error::IncompatibleSchema) {
            Level::Error
        } else {
            Level::Warn
        };
        SINK.with(|slot| slot.get_or_init(|| local_record)(level, source, reason));
    })
}
