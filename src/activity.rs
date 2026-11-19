// ==============================================================================
// Program: activity.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Linux background system activity and resource inventory collector.
//          Inspects socket tables (/proc/net/tcp, udp, unix), mounts (/proc/self/mountinfo),
//          control group hierarchies (/sys/fs/cgroup), and systemd service/timer units.
//          Correlates open network socket inodes back to owning process PIDs.
// ==============================================================================

// How it fits the app: main.rs builds one process `Snapshot` (procfs.rs), then calls `collect` here
// to add the "background activity" view (sockets, mounts, cgroups, systemd units). check.rs and the
// TUI/GUI render the resulting `Inventory`. Every source is read-only (/proc, /sys, `systemctl list-*`).
// Failure model: each field is a `Field<T>` so a denied or missing source degrades to a status
// instead of aborting the whole report. Known limit: owner PIDs for sockets are only visible for
// processes the current user may inspect.
use crate::procfs::{Field, Snapshot};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
// Command is used only to run `systemctl` (read-only list queries); no shell is involved, so
// arguments are never subject to shell interpretation.
use std::process::Command;

/// Represents an individual network or Unix-domain socket entry.
#[derive(Debug)]
pub struct SocketRecord {
    /// One of "tcp", "tcp6", "udp", "udp6", "unix"; static because only these literals are ever used.
    pub protocol: &'static str,
    /// Raw /proc/net `ADDRHEX:PORTHEX` text (decoded for display in check.rs); "-" for Unix sockets.
    pub local: String,
    pub remote: String,
    /// TCP: hex state code from /proc/net/tcp (0A = LISTEN). Unix: the St column.
    pub state: String,
    /// Socket inode; the key used to find which process owns the socket (see socket_owners).
    pub inode: Option<u64>,
    /// Filesystem or abstract path of a Unix socket, when it has one.
    pub path: Option<String>,
    /// PIDs holding an fd to this socket; empty if none are visible to the current user.
    pub owners: Vec<u32>,
}

/// Represents an active filesystem mount point.
#[derive(Debug)]
pub struct MountRecord {
    /// Mount target with octal escapes decoded (see `unescape`).
    pub mount_point: String,
    pub fs_type: String,
    pub source: String,
}

/// Comprehensive snapshot of system resources, open sockets, mounts, and units.
#[derive(Debug)]
pub struct Inventory {
    pub sockets: Field<Vec<SocketRecord>>,
    pub mounts: Field<Vec<MountRecord>>,
    /// `Field` carries either a value or the reason it is missing (permissions, unsupported kernel, ...).
    pub cgroup_count: Field<usize>,
    pub cgroups: Field<Vec<String>>,
    /// Unit names derived from the process snapshot itself (no systemctl needed).
    pub units: Vec<String>,
    /// The four systemd_* fields below shell out to `systemctl`; they are Unsupported when systemd is absent.
    pub system_units: Field<Vec<String>>,
    pub user_units: Field<Vec<String>>,
    pub system_timers: Field<Vec<String>>,
    pub user_timers: Field<Vec<String>>,
}

/// Collects system inventory data matching the given process snapshot.
pub fn collect(snapshot: &Snapshot) -> Inventory {
    let mut units: Vec<String> = snapshot
        .processes
        .iter()
        .filter_map(|p| p.systemd_unit.value().cloned())
        .collect();
    // Sort + dedup yields each systemd unit once, in stable order.
    units.sort();
    units.dedup();

    // Everything below is a read-only observation; no unit or socket is ever modified.
    Inventory {
        sockets: collect_sockets(snapshot),
        // Field::map applies the parser only if the read succeeded; otherwise the error status passes through.
        mounts: read_bytes("/proc/self/mountinfo").map(|v| parse_mounts(&v)),
        cgroup_count: count_cgroups(Path::new("/sys/fs/cgroup")),
        cgroups: list_cgroups(Path::new("/sys/fs/cgroup")),
        units,
        system_units: systemd_lines(&[
            "list-units",
            "--all",
            "--type=service",
            "--type=scope",
            "--state=running",
            // Flags keep systemctl output machine-friendly: no header/footer (--no-legend), no pager, plain
            // (non-tree) rows. "--all" includes inactive units; the --state filter then limits services to running ones.
            "--no-legend",
            "--no-pager",
            "--plain",
        ]),
        user_units: systemd_lines(&[
            "--user",
            "list-units",
            "--all",
            "--type=service",
            "--type=scope",
            "--state=running",
            "--no-legend",
            "--no-pager",
            "--plain",
        ]),
        system_timers: systemd_lines(&[
            "list-timers",
            "--all",
            "--no-legend",
            "--no-pager",
            "--plain",
        ]),
        user_timers: systemd_lines(&[
            "--user",
            "list-timers",
            "--all",
            "--no-legend",
            "--no-pager",
            "--plain",
        ]),
    }
}

/// Collects open TCP, UDP, and Unix domain sockets, mapping them to owner PIDs.
fn collect_sockets(snapshot: &Snapshot) -> Field<Vec<SocketRecord>> {
    let mut all = Vec::new();
    // Table-driven so IPv4/IPv6 and TCP/UDP share one parsing routine.
    for (protocol, path) in [
        ("tcp", "/proc/net/tcp"),
        ("tcp6", "/proc/net/tcp6"),
        ("udp", "/proc/net/udp"),
        ("udp6", "/proc/net/udp6"),
    ] {
        match read_bytes(path) {
            Field::Value(v) => all.extend(parse_inet(protocol, &v)),
            // A failed read (permissions, missing file) aborts the whole socket list: a partial inventory
            // would be misleading. `failure` converts the error status to the target type.
            other => return failure(other),
        }
    }
    match read_bytes("/proc/net/unix") {
        Field::Value(v) => all.extend(parse_unix(&v)),
        other => return failure(other),
    }
    // Done last, once: walking every process's fd table is the expensive part.
    let owners = socket_owners(snapshot);
    for socket in &mut all {
        if let Some(inode) = socket.inode {
            socket.owners = owners.get(&inode).cloned().unwrap_or_default();
        }
    }
    Field::Value(all)
}

/// Reads file bytes with typed error categorization.
fn read_bytes(path: &str) -> Field<Vec<u8>> {
    match fs::read(path) {
        Ok(v) => Field::Value(v),
        Err(e) => match e.kind() {
            std::io::ErrorKind::PermissionDenied => Field::PermissionDenied,
            // A missing /proc file means this kernel/OS doesn't offer it, not a transient fault.
            std::io::ErrorKind::NotFound => Field::Unsupported,
            _ => Field::IoError,
        },
    }
}

/// Maps typed error fields across distinct payload types.
fn failure<T, U>(field: Field<T>) -> Field<U> {
    match field {
        // Unreachable in practice (callers only pass non-Value fields); mapped to ParseError defensively.
        Field::Value(_) => Field::ParseError,
        Field::NotCollected => Field::NotCollected,
        Field::PermissionDenied => Field::PermissionDenied,
        Field::Vanished => Field::Vanished,
        Field::Unsupported => Field::Unsupported,
        Field::ParseError => Field::ParseError,
        Field::IoError => Field::IoError,
    }
}

/// Parses /proc/net/tcp and /proc/net/udp line records into SocketRecords.
fn parse_inet(protocol: &'static str, bytes: &[u8]) -> Vec<SocketRecord> {
    String::from_utf8_lossy(bytes)
        .lines()
        // First line of /proc/net/* is a column header.
        .skip(1)
        .filter_map(|line| {
            // Columns (tcp/udp): 0 sl, 1 local, 2 remote, 3 state, 4 tx:rx queues, 5 timer, 6 retrans, 7 uid,
            // 8 timeout, 9 inode. Missing columns make filter_map drop the line instead of panicking.
            let p: Vec<_> = line.split_whitespace().collect();
            Some(SocketRecord {
                protocol,
                local: p.get(1)?.to_string(),
                remote: p.get(2)?.to_string(),
                state: p.get(3)?.to_string(),
                inode: p.get(9).and_then(|v| v.parse().ok()),
                path: None,
                owners: Vec::new(),
            })
        })
        .collect()
}

/// Parses /proc/net/unix socket tables into SocketRecords.
fn parse_unix(bytes: &[u8]) -> Vec<SocketRecord> {
    String::from_utf8_lossy(bytes)
        .lines()
        .skip(1)
        .map(|line| {
            let p: Vec<_> = line.split_whitespace().collect();
            SocketRecord {
                protocol: "unix",
                local: "-".into(),
                remote: "-".into(),
                // Columns (/proc/net/unix): Num RefCount Protocol Flags Type St Inode [Path]. Unnamed sockets have no 8th column.
                state: p.get(5).unwrap_or(&"?").to_string(),
                inode: p.get(6).and_then(|v| v.parse().ok()),
                path: p.get(7).map(|v| v.to_string()),
                owners: Vec::new(),
            }
        })
        .collect()
}

/// Parses /proc/self/mountinfo mount points and filesystem types.
fn parse_mounts(bytes: &[u8]) -> Vec<MountRecord> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let p: Vec<_> = line.split_whitespace().collect();
            // mountinfo has a variable number of optional fields before a lone `-`; the fs type and source
            // follow that separator. Field 4 (index 4) is the mount point.
            let dash = p.iter().position(|v| *v == "-")?;
            Some(MountRecord {
                mount_point: unescape(p.get(4)?),
                fs_type: p.get(dash + 1)?.to_string(),
                source: p.get(dash + 2).unwrap_or(&"?").to_string(),
            })
        })
        .collect()
}

/// Unescapes octal sequences from mountinfo string entries.
/// mountinfo escapes space/tab/newline/backslash as octal (\040, \011, \012, \134). The backslash
/// case is replaced last so it cannot create new escape sequences from earlier replacements.
fn unescape(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

/// Counts total active control groups under a cgroup root directory.
fn count_cgroups(root: &Path) -> Field<usize> {
    // Iterative depth-first walk with an explicit stack avoids recursion-depth limits on deep cgroup trees.
    let mut stack = vec![root.to_path_buf()];
    // Guards against visiting a directory twice (e.g. via unusual mounts/symlinks).
    let mut seen = HashSet::new();
    let mut count = 0;
    while let Some(path) = stack.pop() {
        let entries = match fs::read_dir(&path) {
            Ok(v) => v,
            Err(e) => {
                return match e.kind() {
                    std::io::ErrorKind::PermissionDenied => Field::PermissionDenied,
                    _ => Field::IoError,
                };
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `insert` returns false for already-seen paths, so this both de-duplicates and counts.
            if path.is_dir() && seen.insert(path.clone()) {
                count += 1;
                stack.push(path)
            }
        }
    }
    Field::Value(count)
}

/// Lists all control group hierarchy paths relative to cgroup root.
fn list_cgroups(root: &Path) -> Field<Vec<String>> {
    let mut stack = vec![root.to_path_buf()];
    let mut values = Vec::new();
    while let Some(path) = stack.pop() {
        let entries = match fs::read_dir(&path) {
            Ok(value) => value,
            Err(error) => {
                return match error.kind() {
                    std::io::ErrorKind::PermissionDenied => Field::PermissionDenied,
                    _ => Field::IoError,
                };
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                values.push(
                    // Store paths relative to the cgroup root so output doesn't repeat /sys/fs/cgroup.
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                );
                stack.push(path);
            }
        }
    }
    // The DFS order is arbitrary, so sort for deterministic output.
    values.sort();
    Field::Value(values)
}

/// Executes systemctl with arguments and returns lines of stdout.
fn systemd_lines(args: &[&str]) -> Field<Vec<String>> {
    match Command::new("systemctl").args(args).output() {
        Ok(o) if o.status.success() => Field::Value(
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_owned)
                .collect(),
        ),
        // systemctl ran but failed (e.g. no user session bus): reported as Unsupported rather than as an error.
        Ok(_) => Field::Unsupported,
        // systemctl not installed (non-systemd system).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Field::Unsupported,
        Err(_) => Field::IoError,
    }
}

/// Scans process /proc/<pid>/fd symlinks to map socket inodes to owner PIDs.
fn socket_owners(snapshot: &Snapshot) -> HashMap<u64, Vec<u32>> {
    let mut result: HashMap<u64, Vec<u32>> = HashMap::new();
    for process in &snapshot.processes {
        // Reading another user's /proc/<pid>/fd needs privileges; those processes are silently skipped,
        // which is why some sockets show "unknown owner".
        let Ok(entries) = fs::read_dir(Path::new("/proc").join(process.pid.to_string()).join("fd"))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            // Socket fds are symlinks whose target text looks like `socket:[12345]`; the number is the inode.
            let target = target.to_string_lossy();
            if let Some(inode) = target
                .strip_prefix("socket:[")
                .and_then(|v| v.strip_suffix(']'))
                .and_then(|v| v.parse().ok())
            {
                result.entry(inode).or_default().push(process.pid);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The `\040` in the fixture is an escaped space, so the decoded mount point is "/hello world".
    #[test]
    fn parses_mountinfo() {
        let v = parse_mounts(b"1 2 0:1 / /hello\\040world rw - ext4 /dev/x rw\n");
        assert_eq!(v[0].mount_point, "/hello world");
        assert_eq!(v[0].fs_type, "ext4")
    }
    #[test]
    fn parses_socket() {
        let v = parse_inet("tcp", b"sl local_address rem_address st tx rx tr tm retr uid timeout inode\n0: 0100007F:0016 00000000:0000 0A 0 0 0 0 0 123\n");
        assert_eq!(v[0].state, "0A");
        assert_eq!(v[0].inode, Some(123));
    }
}
