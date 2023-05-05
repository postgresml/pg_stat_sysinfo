use std::panic::catch_unwind;
use std::time::{Duration, Instant};

use pgrx::bgworkers::*;
use pgrx::*;

use crate::collector::*;
use crate::crate_info::*;
use crate::settings;
use crate::shmem_ring_buffer::*;

static CACHE: PgLwLock<ShmemRingBuffer<Report>> = PgLwLock::new();

unsafe impl PGRXSharedMemory for ShmemRingBuffer<Report> {}

pub fn start() {
    pg_shmem_init!(CACHE);

    if settings::read_or_default().interval.is_some() {
        BackgroundWorkerBuilder::new("Cache Worker")
            .set_function("cache_worker")
            .set_library("pg_stat_sysinfo")
            // We don't run any queries but, without SPI, the worker segfaults
            // on startup.
            .enable_spi_access()
            .load();
    }
}

pub fn reports() -> Vec<Report> {
    // It can happen that the lock is not intialized, if the library is not
    // loaded with shared_preload_libraries. That leads to a `panic!(...)`.
    //
    // It is in general not recommended to catch `panic!(...)` but other
    // methods are not ready to hand. Setting an static atomic `ENABLED`
    // variable and then switching off of that does not work because the
    // client backend sees what the variable looks like before the background
    // worker backend starts up and sets it to true. The only way they can
    // share memory is with something in Postgres shared memory, but such
    // values must be locked; and then we have the same problem.
    catch_unwind(|| CACHE.share().read()).unwrap_or_default()
}

pub fn cache_info() -> BufferSummary {
    catch_unwind(|| CACHE.share().stats()).unwrap_or_default()
}

#[pg_guard]
#[no_mangle]
pub extern "C" fn cache_worker() {
    let flags = SignalWakeFlags::SIGHUP | SignalWakeFlags::SIGTERM;

    BackgroundWorker::attach_signal_handlers(flags);

    let name = BackgroundWorker::get_name();
    let interval = settings::read_or_default().interval;
    let mut last_config = Instant::now();

    let mut state: WorkerState = WorkerState::default();
    if let Some(interval) = interval {
        if state.enable(interval) {
            log!(
                "{}: Initialising {} with interval: {:?}",
                CRATE,
                name,
                interval
            );
        }
    }

    let mut remaining_time = if state.enabled {
        state.remaining_time()
    } else {
        Duration::MAX
    };

    while BackgroundWorker::wait_latch(Some(remaining_time)) {
        if BackgroundWorker::sighup_received() {
            match settings::read_or_default().interval {
                Some(interval) => {
                    if state.enable(interval) {
                        log!(
                            "{}: Configuring {} with interval: {:?}",
                            CRATE,
                            name,
                            interval
                        );
                    }
                }
                None => {
                    if state.disable() {
                        log!("{}: Disabling {}.", CRATE, name,);
                    }
                }
            }
            let dur = Instant::now().saturating_duration_since(last_config);
            if dur >= DISK_CACHE_HIATUS {
                log!(
                    "{}: Reloading cache of disk metadata in {} after: {:?}",
                    CRATE,
                    name,
                    dur
                );
                refresh_collector_disk_listing();
            }
            last_config = Instant::now();
        }

        if !state.enabled {
            remaining_time = Duration::MAX;
            continue;
        }

        if state.remaining_time() == Duration::ZERO {
            debug1!(
                "{}: Writing to cache in {} after: {:?}",
                CRATE,
                name,
                Instant::now().saturating_duration_since(state.last_run)
            );
            write_new_report_to_cache();
            state.last_run = Instant::now();
        }

        remaining_time = state.remaining_time();
    }

    if !state.enabled {
        log!("{}: Shutting down {}", CRATE, name);
    }
}

fn write_new_report_to_cache() {
    let report = singleton().report();
    CACHE.exclusive().write(report).expect("Full cache?");
}

const DISK_CACHE_HIATUS: Duration = Duration::from_secs(100);

fn refresh_collector_disk_listing() {
    singleton().discover_new_disks();
}

#[derive(Clone, Debug, PartialEq)]
struct WorkerState {
    interval: Duration,
    last_run: Instant,
    enabled: bool,
}

impl WorkerState {
    fn enable(&mut self, interval: Duration) -> bool {
        let mut changed = false;
        if !self.enabled {
            self.enabled = true;
            self.last_run = Instant::now();
            changed = true;
        }
        if self.interval != interval {
            self.interval = interval;
            changed = true;
        }
        changed
    }

    fn disable(&mut self) -> bool {
        let mut changed = false;
        if self.enabled {
            self.enabled = false;
            changed = true;
        }
        changed
    }

    fn remaining_time(&self) -> Duration {
        let passed = Instant::now().saturating_duration_since(self.last_run);
        self.interval.saturating_sub(passed)
    }
}

impl Default for WorkerState {
    fn default() -> Self {
        WorkerState {
            interval: Duration::default(),
            last_run: Instant::now(),
            enabled: false,
        }
    }
}
