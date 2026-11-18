// ==============================================================================
// Program: recording.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Process event recording and serialization module for What's Running.
//          Safely writes captured lifecycle events to disk as JSON Lines (JSONL)
//          with secure file permissions (0600) and strict maximum byte bounds
//          to prevent runaway disk usage during profiling runs.
// ==============================================================================

use crate::events::Event;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Serializes and writes lifecycle events to disk within size constraints and secure permissions.
pub fn write_events(path: &Path, events: &[Event], max_bytes: u64) -> io::Result<(usize, u64)> {
    // Step 1: Open file with restricted owner-only read/write permissions (0600)
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;

    // Step 2: Write metadata header preamble
    let header = "{\"record_type\":\"whats-running-events\",\"schema_version\":1,\"commands_included\":false}\n";
    if header.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "maximum recording size is too small",
        ));
    }
    file.write_all(header.as_bytes())?;

    // Step 3: Stream and write individual event lines while respecting byte threshold
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

    // Step 4: Flush filesystem buffers
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
