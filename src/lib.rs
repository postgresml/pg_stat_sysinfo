// Something about the #[pg_extern] TableIterator functions is confusing to
// Clippy.
#![allow(clippy::useless_conversion)]

use pgrx::prelude::*;
use pgrx::*;
use serde_json::Value;
use time::OffsetDateTime;

mod cache_worker;
mod collector;
mod crate_info;
mod init;
mod settings;
mod shmem_ring_buffer;

pgrx::pg_module_magic!();

#[pg_extern(stable)]
fn pg_stat_sysinfo_collect() -> TableIterator<
    'static,
    (
        name!(metric, String),
        name!(dimensions, JsonB),
        name!(at, TimestampWithTimeZone),
        name!(value, f64),
    ),
> {
    let mut instance = collector::singleton();

    if !instance.is_initialized() {
        notice!("Initializing system information caches.");
        instance.cache_initialization();
    }

    let report = instance.report().rows();

    maprows(report.into_iter())
}

#[pg_extern(stable)]
fn pg_stat_sysinfo_cache_summary(
) -> TableIterator<'static, (name!(bytes_used, i64), name!(items, i64))> {
    let info = cache_worker::cache_info();
    let translated = (info.bytes_used as i64, info.items as i64);

    TableIterator::new(vec![translated].into_iter())
}

#[pg_extern(stable)]
fn pg_stat_sysinfo_cached() -> TableIterator<
    'static,
    (
        name!(metric, String),
        name!(dimensions, JsonB),
        name!(at, TimestampWithTimeZone),
        name!(value, f64),
    ),
> {
    let reports = cache_worker::reports();
    let rows = reports.iter().flat_map(|report| report.rows());
    #[allow(clippy::needless_collect)]
    let v: Vec<_> = rows.collect();

    maprows(v.into_iter())
}

extension_sql!(
    r#"
    CREATE VIEW pg_stat_sysinfo AS
         SELECT * FROM pg_stat_sysinfo_cached()
       ORDER BY at DESC;
    "#,
    name = "create_view",
    requires = [pg_stat_sysinfo_cached]
);

fn maprows<'a, I: Iterator<Item = (String, Value, OffsetDateTime, f64)> + 'a>(
    iter: I,
) -> TableIterator<
    'a,
    (
        name!(metric, String),
        name!(dimensions, JsonB),
        name!(at, TimestampWithTimeZone),
        name!(value, f64),
    ),
> {
    let translated = iter.filter_map(|(metric, dimensions, at, value)| {
        match TimestampWithTimeZone::try_from(at) {
            Ok(tstz) => Some((metric, JsonB(dimensions), tstz, value)),
            Err(_err) => {
                warning!("Failed to translate timestamp: {:?}", at);
                None
            }
        }
    });

    TableIterator::new(translated)
}
