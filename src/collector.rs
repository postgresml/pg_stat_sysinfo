use std::thread;
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use parking_lot::{Mutex, MutexGuard};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sysinfo::{CpuExt, DiskExt, SystemExt};
use time::OffsetDateTime;

lazy_static! {
    static ref SINGLETON: Mutex<Collector> = Mutex::new(Collector::new());
}
/**
 Provide singleton collector.
*/
pub fn singleton() -> MutexGuard<'static, Collector> {
    SINGLETON.lock()
}

/**
 The system report as Postgres-friendly types. Note that all memory and disk
 sizes are stored as `f64`, which allows for exact representation of up to
 8192 terabytes.
*/
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Report {
    pub load: Load,
    pub at: time::OffsetDateTime,
    pub cpu_usage: f64,
    pub memory: Memory,
    pub swap: Memory,
    pub volumes: Vec<VolumeInfo>,
}

impl Report {
    pub fn rows(&self) -> Vec<(String, Value, OffsetDateTime, f64)> {
        report_rows(self)
    }
}

fn report_rows(r: &Report) -> Vec<(String, Value, OffsetDateTime, f64)> {
    let ownerize = |v: Vec<(&str, &Value, OffsetDateTime, f64)>| -> Vec<_> {
        v.into_iter()
            .map(|(a, b, c, d)| (String::from(a), b.clone(), c, d))
            .collect()
    };
    let empty = json!({});
    let duration1m = json!({"duration": "1m"});
    let duration5m = json!({"duration": "5m"});
    let duration15m = json!({"duration": "15m"});
    let mut result = ownerize(vec![
        ("load_average", &duration1m, r.at, r.load.min1),
        ("load_average", &duration5m, r.at, r.load.min5),
        ("load_average", &duration15m, r.at, r.load.min15),
        ("cpu_usage", &empty, r.at, r.cpu_usage),
        ("memory_usage", &empty, r.at, r.memory.usage),
        ("memory_size", &empty, r.at, r.memory.size),
        ("memory_available", &empty, r.at, r.memory.available),
        ("swap_usage", &empty, r.at, r.swap.usage),
        ("swap_size", &empty, r.at, r.swap.size),
        ("swap_available", &empty, r.at, r.swap.available),
    ]);

    for vol in &r.volumes {
        let dims = json!({ "fs": vol.name });
        result.append(&mut ownerize(vec![
            ("disk_usage", &dims, r.at, vol.usage),
            ("disk_size", &dims, r.at, vol.size),
            ("disk_available", &dims, r.at, vol.available),
        ]));
    }

    result
}

/**
 The collector manages system caches and reporting.
*/
pub struct Collector {
    client: sysinfo::System,
    last_refresh: Option<Instant>,
}

impl Collector {
    pub fn new() -> Self {
        Collector {
            client: sysinfo::System::new(),
            last_refresh: None,
        }
    }

    pub fn report(&mut self) -> Report {
        if self.last_refresh.is_none() {
            self.cache_initialization();
        }

        self.refresh();

        let at = OffsetDateTime::now_utc();

        let load_average = self.client.load_average();

        let disks = self.client.disks();
        let volumes = disks.iter().map(VolumeInfo::from).collect();

        let memory = Memory::from_total_and_available(
            self.client.total_memory(),
            // Why this stat: free means memory that is not used for anything,
            // whereas available means memory that can be allocated, including
            // by flushing cache or buffers. This seems to be more indicative
            // of the real system state.
            self.client.available_memory(),
        );
        let swap = Memory::from_total_and_available(
            self.client.total_swap(),
            // NB: No `available_swap` statistic.
            self.client.free_swap(),
        );

        let cpu_usage = self.client.global_cpu_info().cpu_usage() as f64;

        Report {
            load: Load {
                min1: load_average.one,
                min5: load_average.five,
                min15: load_average.fifteen,
            },
            at,
            cpu_usage,
            memory,
            swap,
            volumes,
        }
    }

    // Anything that needs to be run before taking the first real measurements.
    pub fn cache_initialization(&mut self) {
        self.discover_new_disks();
        self.client.refresh_cpu();
        self.last_refresh = Some(Instant::now());
    }

    pub fn is_initialized(&self) -> bool {
        self.last_refresh.is_some()
    }

    fn refresh(&mut self) {
        if let Some(dur) = self.cpu_safe_sleep() {
            thread::sleep(dur);
        }
        // All of these seem to be implemented in a way that is relatively
        // fault tolerant. For example, if a disk was removed and can not be
        // refreshed, it simply won't be updated.
        self.client.refresh_cpu();
        self.client.refresh_disks();
        self.client.refresh_memory();

        self.last_refresh = Some(Instant::now());
    }

    fn cpu_safe_sleep(&self) -> Option<Duration> {
        let zero = Duration::new(0, 0);
        let now = Instant::now();
        let passed = self.last_refresh.map(|t| now - t).unwrap_or(zero);

        sysinfo::System::MINIMUM_CPU_UPDATE_INTERVAL.checked_sub(passed)
    }

    // This should be run once in an awhile, or due to device events or
    // something of that nature.
    pub fn discover_new_disks(&mut self) {
        self.client.refresh_disks_list();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VolumeInfo {
    pub name: String,
    pub size: f64,
    pub available: f64,
    pub usage: f64,
}

impl From<&sysinfo::Disk> for VolumeInfo {
    fn from(disk: &sysinfo::Disk) -> Self {
        let name = disk.mount_point().to_string_lossy().into_owned();
        let size = disk.total_space() as f64;
        let available = disk.available_space() as f64;
        let usage = usage_percent(size, available);

        VolumeInfo {
            name,
            size,
            available,
            usage,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Load {
    pub min1: f64,
    pub min5: f64,
    pub min15: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Memory {
    pub size: f64,
    pub available: f64,
    pub usage: f64,
}

impl Memory {
    fn from_total_and_available(size: u64, available: u64) -> Self {
        let size = size as f64;
        let available = available as f64;
        let usage = usage_percent(size, available);

        Memory {
            size,
            available,
            usage,
        }
    }
}

fn usage_percent(size: f64, available: f64) -> f64 {
    100.0
        * if size > 0.0 {
            1.0 - available / size
        } else {
            0.0
        }
}
