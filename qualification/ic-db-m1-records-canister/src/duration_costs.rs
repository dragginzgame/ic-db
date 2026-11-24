//! Installed measurement of grouped elapsed-time arithmetic.

use crate::{open, runtime, schema::TimeRow};
use ic_db::design::prelude::*;

/// Partition observations by their zone.
#[record(fields(field(name = "zone", value(item(prim = "Bool")))))]
struct Zone {}

/// Preserve absent measurements and millisecond units.
#[record(fields(field(name = "delay", value(opt, item(prim = "Duration")))))]
struct Delay {}

// measured_duration_summary
// Measure opening the records store and completing a grouped Duration summary.
// guard  : validates the request frame for method 245.
// limits : 64 KiB request, 64 KiB reply.
#[ic_platform_runtime::update(
    runtime = ic_platform_runtime,
    name = "measured_duration_summary",
    guard = crate::transport::admit::<245>,
    decode = crate::transport::decode_measured::<_, 245>,
    encode = crate::transport::encode::<_, 245>,
    max_bytes = 65_536,
    on_error = crate::transport::endpoint_error
)]
fn measured_duration_summary(
    (run_summary, counter): crate::transport::Measured<bool>,
) -> (u64, u64, u64, u32, u32, u64, u64) {
    if !run_summary {
        return (0, 0, 0, 0, 0, 0, 0);
    }
    let start = counter();
    let host = runtime();
    let db = open(&host);
    let opened = counter();
    let summary = db
        .query::<TimeRow>()
        .group_summary::<Zone, Delay, Delay>()
        .unwrap();
    let finished = counter();
    let [group] = summary.value.as_slice() else {
        panic!("expected one seeded zone")
    };
    (
        group.count,
        group.sum.as_ref().unwrap().delay.unwrap().as_millis(),
        group.average.as_ref().unwrap().delay.unwrap().as_millis(),
        summary.examined as u32,
        summary.fetched as u32,
        opened - start,
        finished - opened,
    )
}
