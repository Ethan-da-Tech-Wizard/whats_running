use crate::procfs::{Field, Snapshot};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct SocketRecord {
    pub protocol: &'static str,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub inode: Option<u64>,
    pub path: Option<String>,
    pub owners: Vec<u32>,
}
#[derive(Debug)]
pub struct MountRecord {
    pub mount_point: String,
    pub fs_type: String,
    pub source: String,
}
#[derive(Debug)]
pub struct Inventory {
    pub sockets: Field<Vec<SocketRecord>>,
    pub mounts: Field<Vec<MountRecord>>,
    pub cgroup_count: Field<usize>,
    pub cgroups: Field<Vec<String>>,
    pub units: Vec<String>,
    pub system_units: Field<Vec<String>>,
    pub user_units: Field<Vec<String>>,
    pub system_timers: Field<Vec<String>>,
    pub user_timers: Field<Vec<String>>,
}

pub fn collect(snapshot: &Snapshot) -> Inventory {
    let mut units: Vec<String> = snapshot
        .processes
        .iter()
        .filter_map(|p| p.systemd_unit.value().cloned())
        .collect();
    units.sort();
    units.dedup();
    Inventory {
        sockets: collect_sockets(snapshot),
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
fn collect_sockets(snapshot: &Snapshot) -> Field<Vec<SocketRecord>> {
    let mut all = Vec::new();
    for (protocol, path) in [
        ("tcp", "/proc/net/tcp"),
        ("tcp6", "/proc/net/tcp6"),
        ("udp", "/proc/net/udp"),
        ("udp6", "/proc/net/udp6"),
    ] {
        match read_bytes(path) {
            Field::Value(v) => all.extend(parse_inet(protocol, &v)),
            other => return failure(other),
        }
    }
    match read_bytes("/proc/net/unix") {
        Field::Value(v) => all.extend(parse_unix(&v)),
        other => return failure(other),
    }
    let owners = socket_owners(snapshot);
    for socket in &mut all {
        if let Some(inode) = socket.inode {
            socket.owners = owners.get(&inode).cloned().unwrap_or_default();
        }
    }
    Field::Value(all)
}
fn read_bytes(path: &str) -> Field<Vec<u8>> {
    match fs::read(path) {
        Ok(v) => Field::Value(v),
        Err(e) => match e.kind() {
            std::io::ErrorKind::PermissionDenied => Field::PermissionDenied,
            std::io::ErrorKind::NotFound => Field::Unsupported,
            _ => Field::IoError,
        },
    }
}
fn failure<T, U>(field: Field<T>) -> Field<U> {
    match field {
        Field::Value(_) => Field::ParseError,
        Field::NotCollected => Field::NotCollected,
        Field::PermissionDenied => Field::PermissionDenied,
        Field::Vanished => Field::Vanished,
        Field::Unsupported => Field::Unsupported,
        Field::ParseError => Field::ParseError,
        Field::IoError => Field::IoError,
    }
}
fn parse_inet(protocol: &'static str, bytes: &[u8]) -> Vec<SocketRecord> {
    String::from_utf8_lossy(bytes)
        .lines()
        .skip(1)
        .filter_map(|line| {
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
                state: p.get(5).unwrap_or(&"?").to_string(),
                inode: p.get(6).and_then(|v| v.parse().ok()),
                path: p.get(7).map(|v| v.to_string()),
                owners: Vec::new(),
            }
        })
        .collect()
}
fn parse_mounts(bytes: &[u8]) -> Vec<MountRecord> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let p: Vec<_> = line.split_whitespace().collect();
            let dash = p.iter().position(|v| *v == "-")?;
            Some(MountRecord {
                mount_point: unescape(p.get(4)?),
                fs_type: p.get(dash + 1)?.to_string(),
                source: p.get(dash + 2).unwrap_or(&"?").to_string(),
            })
        })
        .collect()
}
fn unescape(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}
fn count_cgroups(root: &Path) -> Field<usize> {
    let mut stack = vec![root.to_path_buf()];
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
            if path.is_dir() && seen.insert(path.clone()) {
                count += 1;
                stack.push(path)
            }
        }
    }
    Field::Value(count)
}
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
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                );
                stack.push(path);
            }
        }
    }
    values.sort();
    Field::Value(values)
}
fn systemd_lines(args: &[&str]) -> Field<Vec<String>> {
    match Command::new("systemctl").args(args).output() {
        Ok(o) if o.status.success() => Field::Value(
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_owned)
                .collect(),
        ),
        Ok(_) => Field::Unsupported,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Field::Unsupported,
        Err(_) => Field::IoError,
    }
}
fn socket_owners(snapshot: &Snapshot) -> HashMap<u64, Vec<u32>> {
    let mut result: HashMap<u64, Vec<u32>> = HashMap::new();
    for process in &snapshot.processes {
        let Ok(entries) = fs::read_dir(Path::new("/proc").join(process.pid.to_string()).join("fd"))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
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
    #[test]
    fn parses_mountinfo() {
        let v = parse_mounts(b"1 2 0:1 / /hello\\040world rw - ext4 /dev/x rw\n");
        assert_eq!(v[0].mount_point, "/hello world");
        assert_eq!(v[0].fs_type, "ext4")
    }
    #[test]
    fn parses_socket() {
        let v=parse_inet("tcp",b"sl local_address rem_address st tx rx tr tm retr uid timeout inode\n0: 0100007F:0016 00000000:0000 0A 0 0 0 0 0 123\n");
        assert_eq!(v[0].state, "0A");
        assert_eq!(v[0].inode, Some(123));
    }
}
