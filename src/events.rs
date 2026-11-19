// ==============================================================================
// Program: events.rs
// Programmer: Ethan-da-Tech-Wizard
// Last Modified: November 18, 2026
// Purpose: Linux Netlink proc-connector lifecycle event monitor.
//          Subscribes to kernel process lifecycle notifications via Netlink
//          sockets (AF_NETLINK, NETLINK_CONNECTOR) to capture real-time process
//          fork, exec, and exit events without polling. Formats events as JSON
//          and detects sequence number gaps per CPU core.
// ==============================================================================

// No libc crate: the handful of C functions needed are declared by hand below, keeping the
// binary dependency-free. Constants are the Linux values from <sys/socket.h> and <linux/netlink.h>.
use std::collections::HashMap;
use std::io;
use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Address family for kernel<->userspace Netlink sockets.
const AF_NETLINK: c_int = 16;
const SOCK_DGRAM: c_int = 2;
/// Netlink protocol that carries "connector" messages, the transport used by the proc connector.
const NETLINK_CONNECTOR: c_int = 11;
const SOL_SOCKET: c_int = 1;
/// Receive timeout option, so recv() returns instead of blocking forever when no events arrive.
const SO_RCVTIMEO: c_int = 20;

// Raw libc bindings. All calls are made inside `unsafe` blocks below; return values (< 0 = error,
// with details in errno via io::Error::last_os_error) are checked at each call site.
unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, address: *const SockAddrNl, length: u32) -> c_int;
    fn send(fd: c_int, buffer: *const c_void, length: usize, flags: c_int) -> isize;
    fn recv(fd: c_int, buffer: *mut c_void, length: usize, flags: c_int) -> isize;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, length: u32)
    -> c_int;
    fn close(fd: c_int) -> c_int;
    fn getpid() -> c_int;
}

/// C-compatible layout, required because the kernel reads these structs directly.
#[repr(C)]
struct SockAddrNl {
    /// struct sockaddr_nl: family must be AF_NETLINK; `pad` is reserved and must be zero.
    family: u16,
    pad: u16,
    /// Port id: we use our own PID so the kernel can address replies to this socket.
    pid: u32,
    /// Multicast group bitmask; bit 1 (value 1) is the connector proc-events group (CN_IDX_PROC).
    groups: u32,
}

#[repr(C)]
struct TimeVal {
    /// struct timeval with 64-bit fields, matching the layout on 64-bit Linux. This struct would be wrong on 32-bit targets.
    seconds: i64,
    microseconds: i64,
}

/// Represents the classification and payload of a captured process lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// PIDs here are the kernel's per-thread ids (the `pid` fields of the proc event, not tgid).
    Fork {
        parent: u32,
        child: u32,
    },
    Exec {
        pid: u32,
    },
    Exit {
        pid: u32,
        code: u32,
        signal: u32,
    },
    /// Any event type this tool does not decode (uid/gid changes, sid, ptrace, comm, coredump...); keeps the raw code.
    Other(u32),
}

/// Normalized event record carrying kernel telemetry and monotonic sequences.
#[derive(Debug, Clone)]
pub struct Event {
    /// Per-CPU counter assigned by the kernel; used to detect dropped messages (see `sequence_gaps`).
    pub sequence: u32,
    /// Kernel timestamp in nanoseconds (monotonic clock; not wall-clock time).
    pub timestamp_ns: u64,
    /// Wall-clock time at which user space parsed the message, as Unix epoch milliseconds.
    pub received_unix_ms: u128,
    /// CPU that generated the event; sequence numbers are only continuous within one CPU.
    pub cpu: u32,
    pub kind: Kind,
}

impl std::fmt::Display for Event {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "sequence={} received_ms={} kernel_ns={} cpu={} kind={:?}",
            self.sequence, self.received_unix_ms, self.timestamp_ns, self.cpu, self.kind
        )
    }
}

impl Event {
    /// Serializes the event record into a structured JSON string.
    pub fn json(&self) -> String {
        // Hand-rolled JSON: all values are integers, so no string escaping is needed and no serde dependency.
        let kind = match self.kind {
            Kind::Fork { parent, child } => {
                format!("\"fork\",\"parent\":{parent},\"child\":{child}")
            }
            Kind::Exec { pid } => format!("\"exec\",\"pid\":{pid}"),
            Kind::Exit { pid, code, signal } => {
                format!("\"exit\",\"pid\":{pid},\"code\":{code},\"signal\":{signal}")
            }
            Kind::Other(value) => format!("\"other\",\"value\":{value}"),
        };
        format!(
            "{{\"schema_version\":1,\"sequence\":{},\"received_unix_ms\":{},\"kernel_timestamp_ns\":{},\"cpu\":{},\"kind\":{kind}}}",
            self.sequence, self.received_unix_ms, self.timestamp_ns, self.cpu
        )
    }
}

/// RAII wrapper ensuring socket descriptor closure on drop.
struct Socket(c_int);
/// Guarantees the fd is closed on every return path, including the early `?`/Err returns in capture().
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe {
            close(self.0);
        }
    }
}

/// Results container for an event capture session.
pub struct Capture {
    pub events: Vec<Event>,
    /// Total count of events the kernel numbered but we never saw (buffer overflow / lost messages).
    pub sequence_gaps: u64,
}

/// Binds to the kernel proc connector and captures lifecycle events up to a given limit.
pub fn capture(limit: usize, timeout: Duration) -> io::Result<Capture> {
    // Opening this socket usually needs CAP_NET_ADMIN (root); failures are translated by capability_error().
    let fd = unsafe { socket(AF_NETLINK, SOCK_DGRAM, NETLINK_CONNECTOR) };
    if fd < 0 {
        return Err(capability_error(io::Error::last_os_error()));
    }
    let socket = Socket(fd);

    let address = SockAddrNl {
        family: AF_NETLINK as u16,
        pad: 0,
        pid: unsafe { getpid() } as u32,
        groups: 1,
    };
    // Binding with groups=1 joins the proc-event multicast group, but the kernel only starts sending
    // after the subscribe message below.
    if unsafe { bind(fd, &address, size_of::<SockAddrNl>() as u32) } < 0 {
        return Err(capability_error(io::Error::last_os_error()));
    }

    // Without a timeout recv() would block forever on a quiet system; with it the capture window ends cleanly.
    let tv = TimeVal {
        seconds: timeout.as_secs() as i64,
        microseconds: timeout.subsec_micros() as i64,
    };
    if unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            SO_RCVTIMEO,
            &tv as *const _ as *const c_void,
            size_of::<TimeVal>() as u32,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }

    // Without this PROC_CN_MCAST_LISTEN request the kernel does not emit process events.
    let subscribe = subscription_message(unsafe { getpid() } as u32, true);
    if unsafe { send(fd, subscribe.as_ptr() as *const c_void, subscribe.len(), 0) } < 0 {
        return Err(capability_error(io::Error::last_os_error()));
    }

    let mut events = Vec::new();
    // Last sequence seen per CPU. A jump larger than +1 means messages were lost.
    let mut last_sequence: HashMap<u32, u32> = HashMap::new();
    let mut sequence_gaps = 0_u64;
    let mut buffer = [0_u8; 4096];
    while events.len() < limit {
        let read = unsafe { recv(fd, buffer.as_mut_ptr() as *mut c_void, buffer.len(), 0) };
        if read < 0 {
            let error = io::Error::last_os_error();
            // A receive timeout surfaces as WouldBlock/TimedOut: treat it as "capture window finished", not an error.
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) {
                break;
            }
            return Err(error);
        }
        if let Some(event) = parse(&buffer[..read as usize]) {
            if let Some(last) = last_sequence.get(&event.cpu) {
                // saturating_add avoids overflow at u32::MAX; wrap-around itself is not handled.
                let expected = last.saturating_add(1);
                if event.sequence > expected {
                    sequence_gaps += (event.sequence - expected) as u64;
                }
            }
            last_sequence.insert(event.cpu, event.sequence);
            events.push(event)
        }
    }

    // Best-effort polite unsubscribe; the result is ignored because closing the socket ends the
    // subscription anyway. subscription_message encodes `false` as PROC_CN_MCAST_IGNORE (2).
    let unsubscribe = subscription_message(unsafe { getpid() } as u32, false);
    let _ = unsafe {
        send(
            socket.0,
            unsubscribe.as_ptr() as *const c_void,
            unsubscribe.len(),
            0,
        )
    };

    Ok(Capture {
        events,
        sequence_gaps,
    })
}

/// Translates permission errors into user-friendly capability advice.
fn capability_error(error: io::Error) -> io::Error {
    // errno 1 is EPERM, which some paths report without mapping to PermissionDenied.
    if error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1) {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process events denied; run explicitly with suitable capability/elevation",
        )
    } else {
        error
    }
}

/// Generates proc connector subscription packet buffer.
fn subscription_message(pid: u32, listen: bool) -> [u8; 40] {
    // Message layout (offsets in bytes): nlmsghdr 0..16 {len@0, type@4, flags@6, seq@8, pid@12},
    // then cn_msg 16..36 {idx@16, val@20, seq@24, ack@28, len@32, flags@34}, then a 4-byte payload @36
    // (the PROC_CN_MCAST_* op). Total 40 bytes.
    let mut b = [0_u8; 40];
    put32(&mut b, 0, 40);
    // nlmsg_type 3 = NLMSG_DONE, which connector messages use.
    put16(&mut b, 4, 3);
    put32(&mut b, 12, pid);
    // cn_msg id: idx = 1 (CN_IDX_PROC) ...|
    put32(&mut b, 16, 1);
    // ... val = 1 (CN_VAL_PROC): addresses the process-events connector.
    put32(&mut b, 20, 1);
    // cn_msg.len = size of the payload that follows (one u32).
    put16(&mut b, 32, 4);
    // Payload: PROC_CN_MCAST_LISTEN = 1, PROC_CN_MCAST_IGNORE = 2 (linux/cn_proc.h). Unsubscribe
    // used to send 0, which the kernel does not recognise as a valid operation.
    put32(&mut b, 36, if listen { 1 } else { 2 });
    b
}

/// Parses raw Netlink connector packet into an Event record.
fn parse(b: &[u8]) -> Option<Event> {
    // Smallest message we can decode: 16 (nlmsghdr) + 20 (cn_msg) + 16 (proc_event header: what, cpu, timestamp)
    // = 52 bytes, i.e. up to the start of event_data.
    if b.len() < 52 {
        return None;
    }
    // cn_msg.seq (offset 24): the per-CPU sequence number.
    let sequence = get32(b, 24)?;
    // proc_event.what (offset 36): event type code.
    let what = get32(b, 36)?;
    // what == 0 (PROC_EVENT_NONE) is the acknowledgement for our subscribe request, not a real event.
    if what == 0 {
        return None;
    }
    let cpu = get32(b, 40)?;
    let timestamp_ns = get64(b, 44)?;
    // Start of the event-specific data union inside proc_event.
    let u = 52;
    let kind = match what {
        // Event codes: 1 = PROC_EVENT_FORK, 2 = PROC_EVENT_EXEC, 0x80000000 = PROC_EVENT_EXIT.
        1 => Kind::Fork {
            parent: get32(b, u)?,
            // fork data = parent_pid@0, parent_tgid@4, child_pid@8, child_tgid@12; we read the pids.
            child: get32(b, u + 8)?,
        },
        2 => Kind::Exec { pid: get32(b, u)? },
        0x8000_0000 => Kind::Exit {
            pid: get32(b, u)?,
            // exit data = pid@0, tgid@4, exit_code@8, exit_signal@12.
            code: get32(b, u + 8)?,
            signal: get32(b, u + 12)?,
        },
        other => Kind::Other(other),
    };
    Some(Event {
        sequence,
        timestamp_ns,
        received_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        cpu,
        kind,
    })
}

/// Netlink uses host (native) byte order, hence to_ne_bytes/from_ne_bytes rather than big-endian.
/// The getters return None on short buffers so truncated packets are skipped, never panic.
fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_ne_bytes())
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_ne_bytes())
}

fn get32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn get64(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins down the wire layout: a hand-built exec event must decode to the right kind, so a
    /// wrong offset in parse() (which would silently misreport PIDs) fails here.
    /// Builds a minimal 68-byte packet by hand: what=2 (exec) @36, cpu=3 @40, timestamp=99 @44, pid=42 @52.
    #[test]
    fn parses_exec() {
        let mut b = [0_u8; 68];
        put32(&mut b, 36, 2);
        put32(&mut b, 40, 3);
        b[44..52].copy_from_slice(&99_u64.to_ne_bytes());
        put32(&mut b, 52, 42);
        assert_eq!(parse(&b).unwrap().kind, Kind::Exec { pid: 42 });
    }
    /// The subscribe packet must be exactly 40 bytes with the LISTEN op (1); a wrong length or op
    /// means no events ever arrive, and nothing else in the test suite would notice.
    #[test]
    fn subscription_is_well_formed() {
        let b = subscription_message(7, true);
        assert_eq!(get32(&b, 0), Some(40));
        assert_eq!(get32(&b, 36), Some(1));
    }

    /// Regression: unsubscribing must send PROC_CN_MCAST_IGNORE (2), not 0, which the kernel rejects.
    #[test]
    fn unsubscribe_sends_ignore_op() {
        let b = subscription_message(7, false);
        assert_eq!(get32(&b, 36), Some(2));
    }
}
