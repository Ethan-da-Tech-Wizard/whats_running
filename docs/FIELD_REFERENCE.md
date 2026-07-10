# Linux field reference

Status: Verified for M1 on 2026-07-10

| Output | Linux source | Meaning and caveat |
|---|---|---|
| PID | numeric `/proc` directory | ID in the observer's visible PID namespace; reusable |
| PPID | `/proc/PID/stat` field 4 | Parent at sample time; parent may already have exited |
| USER/UID | first value of `/proc/PID/status` `Uid:` | Numeric real UID; name lookup intentionally deferred |
| STATE | `/proc/PID/stat` field 3 | One-letter kernel state; transient |
| NAME | parentheses-delimited command in `/proc/PID/stat` | Kernel task name, not necessarily executable filename |
| COMMAND | NUL-separated `/proc/PID/cmdline` | Hidden by default; empty for many kernel threads; can contain secrets |
| executable | `/proc/PID/exe` symlink | May be denied, vanished, or point to a deleted executable |
| start_ticks | `/proc/PID/stat` field 22 | Clock ticks since boot; paired with PID for stable identity |
| CPU ticks | `/proc/PID/stat` fields 14–15 | Lifetime user/system counters; M1 does not mislabel them as rates |
| RSS KiB | `/proc/PID/status` `VmRSS:` | Convenient resident estimate with kernel accounting caveats |
| observer PID | process's own PID | Must correspond to a listed record in a successful ordinary snapshot |
| duration | monotonic timer around collection | Observer cost for enumeration/reading, excluding rendering |
| CPU% | delta of process user+system ticks divided by aggregate CPU tick delta, scaled by logical CPUs | Requires a prior sample; 100% means roughly one fully occupied logical CPU |
| read/write bytes | `/proc/PID/io` `read_bytes:`/`write_bytes:` | Storage-layer counters, not all application reads/writes or cache activity |
| read/write bytes per second | delta of storage byte counter divided by wall-clock sample interval | Requires a prior sample; unknown if either counter is unavailable or decreases |
| memory totals | `/proc/meminfo` | Used display is total minus available; swap used is total minus free |

## Typed field statuses

- `value`: successfully read and represented.
- `permission_denied`: kernel/filesystem denied access.
- `vanished`: entry disappeared during the non-atomic snapshot.
- `unsupported`: operation/interface is unsupported.
- `parse_error`: bytes were readable but not understood.
- `io_error`: another read error occurred.
- `not_collected`: privacy or option deliberately prevented collection/export.

Schema-v3 numeric fields carry the same typed status as strings: value, permission denied, vanished, unsupported, parse error, I/O error, or not collected. Derived rates retain their separate warm-up status because they depend on two samples.
