// ==============================================================================
// Program: recording.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Process event recording and serialization module for What's Running.
//          Safely writes captured lifecycle events to disk as JSON Lines (JSONL)
//          with secure file permissions (0600) and strict maximum byte bounds
//          to prevent runaway disk usage during profiling runs.
// ==============================================================================

// How it fits the app: main.rs collects `Event`s from events.rs (the Netlink proc-connector reader)
// and, when the user asks to record, hands them to `write_events` here. Nothing else reads the file
// back inside this crate; it is meant for offline analysis (one JSON object per line, header first).
// Inputs: destination path, event slice, byte cap. Output: `(events_written, bytes_written)`.
// Known limits: Unix only (file modes); a cap that is too small yields an error, not a partial header.
use crate::events::Event;
use std::fs::OpenOptions;
use std::io::{self, Write};
// OpenOptionsExt provides `.mode()`, which is Unix-only; this module therefore does not build on Windows.
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Serializes and writes lifecycle events to disk within size constraints and secure permissions.
///
/// # Arguments
/// * `path` - destination file; created or truncated, never appended to.
/// * `events` - events in capture order; only a prefix is written if `max_bytes` is reached.
/// * `max_bytes` - hard cap on the file size in bytes, header included.
///
/// # Returns
/// `(count, bytes)`: events actually written and the final file size. `count < events.len()`
/// means the cap truncated the recording.
///
/// # Errors
/// I/O errors from open/write/sync, or `InvalidInput` if `max_bytes` cannot even hold the header.
pub fn write_events(path: &Path, events: &[Event], max_bytes: u64) -> io::Result<(usize, u64)> {
    let mut file = OpenOptions::new()
        // create + truncate: every recording starts from an empty file, never appends to an old one.
        .create(true)
        .truncate(true)
        .write(true)
        // Mode only applies when the file is newly created (and is subject to the umask); an existing
        // file keeps its previous permissions. Owner-only because events describe local process activity.
        .mode(0o600)
        .open(path)?;

    // First JSONL line identifies the format. `commands_included:false` records that command lines
    // are deliberately never written, so recordings are safe(r) to share. Bump schema_version on any format change.
    let header = "{\"record_type\":\"whats-running-events\",\"schema_version\":1,\"commands_included\":false}\n";
    // The header alone must fit, otherwise the "bounded" promise could not be kept at all.
    if header.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "maximum recording size is too small",
        ));
    }
    file.write_all(header.as_bytes())?;

    // `bytes` tracks the running file size so the cap check below never needs to stat the file.
    let mut bytes = header.len() as u64;
    let mut count = 0;
    for event in events {
        // One JSON object per line (JSONL) so partial files are still parseable line by line.
        let line = format!("{}\n", event.json());
        // Stop before exceeding the cap instead of truncating a line mid-record; later events are dropped.
        if bytes + line.len() as u64 > max_bytes {
            break;
        }
        file.write_all(line.as_bytes())?;
        bytes += line.len() as u64;
        count += 1;
    }

    // Force data to disk so a crash right after returning cannot lose a recording the caller was told succeeded.
    file.sync_all()?;
    // Returns (events written, total bytes including header) so callers can report any truncation.
    Ok((count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, Kind};
    /// Pins down the two safety promises of recordings: the file never exceeds the byte cap, and it
    /// is readable only by its owner (0600). Scenario: two identical events but room for one.
    /// Cap of 180 bytes fits the header plus about one event, so the second (identical) event must be dropped.
    #[test]
    fn recording_is_bounded_and_private() {
        // PID in the name avoids collisions between concurrent test runs.
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
