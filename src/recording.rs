use crate::events::Event;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub fn write_events(path: &Path, events: &[Event], max_bytes: u64) -> io::Result<(usize, u64)> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    let header = "{\"record_type\":\"whats-running-events\",\"schema_version\":1,\"commands_included\":false}\n";
    if header.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "maximum recording size is too small",
        ));
    }
    file.write_all(header.as_bytes())?;
    let mut bytes = header.len() as u64;
    let mut count = 0;
    for event in events {
        let line = format!("{}\n", event.json());
        if bytes + line.len() as u64 > max_bytes {
            break;
        }
        file.write_all(line.as_bytes())?;
        bytes += line.len() as u64;
        count += 1;
    }
    file.sync_all()?;
    Ok((count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, Kind};
    #[test]
    fn recording_is_bounded_and_private() {
        let path = std::env::temp_dir().join(format!("wr-record-{}", std::process::id()));
        let event = Event {
            sequence: 0,
            timestamp_ns: 1,
            received_unix_ms: 2,
            cpu: 3,
            kind: Kind::Exec { pid: 4 },
        };
        let (_, bytes) = write_events(&path, &[event.clone(), event], 180).unwrap();
        assert!(bytes <= 180);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_file(path).unwrap();
    }
}
