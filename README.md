# `pg_stat_sysinfo`

Collects system statistics.

```sql
----
CREATE EXTENSION pg_stat_sysinfo;
CREATE EXTENSION
----
SELECT * FROM pg_stat_sysinfo_collect();
      metric      |   dimensions |              at              |       value
------------------+--------------+------------------------------+--------------------
 load_average     | duration:1m  | 2023-01-17 20:40:24.74495+00 |       4.3427734375
 load_average     | duration:5m  | 2023-01-17 20:40:24.74495+00 |        2.740234375
 load_average     | duration:15m | 2023-01-17 20:40:24.74495+00 |           2.390625
 cpu_usage        |              | 2023-01-17 20:40:24.74495+00 |   0.12653848528862
 memory_usage     |              | 2023-01-17 20:40:24.74495+00 | 10.022946522725185
 memory_size      |              | 2023-01-17 20:40:24.74495+00 |         7966543872
 memory_available |              | 2023-01-17 20:40:24.74495+00 |         7168061440
 swap_usage       |              | 2023-01-17 20:40:24.74495+00 |                  0
 swap_size        |              | 2023-01-17 20:40:24.74495+00 |                  0
 swap_available   |              | 2023-01-17 20:40:24.74495+00 |                  0
 disk_usage       | fs:/         | 2023-01-17 20:40:24.74495+00 |  48.68292833372914
 disk_size        | fs:/         | 2023-01-17 20:40:24.74495+00 |        66404147200
 disk_available   | fs:/         | 2023-01-17 20:40:24.74495+00 |        34076663808
 disk_usage       | fs:/boot/efi | 2023-01-17 20:40:24.74495+00 |  4.986992082951202
 disk_size        | fs:/boot/efi | 2023-01-17 20:40:24.74495+00 |          109422592
 disk_available   | fs:/boot/efi | 2023-01-17 20:40:24.74495+00 |          103965696
(16 rows)

```

## Enabling Caching Collector

Add the extension library to `shared_preload_libraries` and set the collection
interval:

```python
shared_preload_libraries = 'pg_stat_sysinfo.so'
pg_stat_sysinfo.interval = '1s'   # Accepts any time format Postgres recognizes
```

The cache is stored in Postgres shared memory. Up to 1280 KiB is cached -- over
an hour, in most cases, at 1 query per second.

```sql
----
CREATE EXTENSION pg_stat_sysinfo;
CREATE EXTENSION
----
SELECT DISTINCT min(at) AS oldest,
       max(at) - min(at) AS during
  FROM pg_stat_sysinfo;
            oldest             |     during
-------------------------------+-----------------
 2023-01-17 20:04:46.220977+00 | 00:55:55.908972
(1 row)

----
SELECT DISTINCT dimensions FROM pg_stat_sysinfo;
   dimensions
----------------

 duration:1m
 duration:5m
 duration:15m
 disk:/
 disk:/boot/efi
(6 rows)

```

Basic cache statistics are available:

```sql
----
SELECT * FROM pg_stat_sysinfo_cache_summary();
 bytes_used | items
------------+-------
     563159 |  3587
(1 row)

```

NVIDIA GPU statistics are available if the `nvidia-smi` command is available

```sql
----
SELECT device_id, device_name, total_memory_mb, used_memory_mb, temperature_c from pg_gpu_info();
 device_id | device_name | total_memory_mb | used_memory_mb | temperature_c 
-----------+-------------+-----------------+----------------+---------------
         0 | Tesla T4    |     16106.12736 |   12277.972992 |            59
         1 | Tesla T4    |     16106.12736 |   13227.982848 |            58
         2 | Tesla T4    |     16106.12736 |    2043.871232 |            56
         3 | Tesla T4    |     16106.12736 |    9325.182976 |            53
(4 rows)
```
## Configuration Changes

The `pg_stat_sysinfo.interval` can be updated by changing `postgres.conf` and
sending `SIGHUP` to the Postgres server process. The cache worker will use the
new interval from that point forward.

If a long enough time has passed between server startup and a `SIGHUP`, or
between one `SIGHUP` and another, the cache worker will refresh the disk
metadata. This will allow it to pick up any disks that have been added to or
removed from the system.
