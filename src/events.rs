//! Linux proc-connector lifecycle events. This is intentionally small and
//! isolated: failure never degrades into pretend event coverage.
use std::collections::HashMap;
use std::io;
use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const AF_NETLINK: c_int = 16;
const SOCK_DGRAM: c_int = 2;
const NETLINK_CONNECTOR: c_int = 11;
const SOL_SOCKET: c_int = 1;
const SO_RCVTIMEO: c_int = 20;

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

#[repr(C)]
struct SockAddrNl {
    family: u16,
    pad: u16,
    pid: u32,
    groups: u32,
}
#[repr(C)]
struct TimeVal {
    seconds: i64,
    microseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Fork { parent: u32, child: u32 },
    Exec { pid: u32 },
    Exit { pid: u32, code: u32, signal: u32 },
    Other(u32),
}
#[derive(Debug, Clone)]
pub struct Event {
    pub sequence: u32,
    pub timestamp_ns: u64,
    pub received_unix_ms: u128,
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
    pub fn json(&self) -> String {
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

struct Socket(c_int);
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe {
            close(self.0);
        }
    }
}

pub struct Capture {
    pub events: Vec<Event>,
    pub sequence_gaps: u64,
}
pub fn capture(limit: usize, timeout: Duration) -> io::Result<Capture> {
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
    if unsafe { bind(fd, &address, size_of::<SockAddrNl>() as u32) } < 0 {
        return Err(capability_error(io::Error::last_os_error()));
    }
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
    let subscribe = subscription_message(unsafe { getpid() } as u32, true);
    if unsafe { send(fd, subscribe.as_ptr() as *const c_void, subscribe.len(), 0) } < 0 {
        return Err(capability_error(io::Error::last_os_error()));
    }
    let mut events = Vec::new();
    let mut last_sequence: HashMap<u32, u32> = HashMap::new();
    let mut sequence_gaps = 0_u64;
    let mut buffer = [0_u8; 4096];
    while events.len() < limit {
        let read = unsafe { recv(fd, buffer.as_mut_ptr() as *mut c_void, buffer.len(), 0) };
        if read < 0 {
            let error = io::Error::last_os_error();
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
                let expected = last.saturating_add(1);
                if event.sequence > expected {
                    sequence_gaps += (event.sequence - expected) as u64;
                }
            }
            last_sequence.insert(event.cpu, event.sequence);
            events.push(event)
        }
    }
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

fn capability_error(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1) {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process events denied; run explicitly with suitable capability/elevation",
        )
    } else {
        error
    }
}
fn subscription_message(pid: u32, listen: bool) -> [u8; 40] {
    let mut b = [0_u8; 40];
    put32(&mut b, 0, 40);
    put16(&mut b, 4, 3);
    put32(&mut b, 12, pid);
    put32(&mut b, 16, 1);
    put32(&mut b, 20, 1);
    put16(&mut b, 32, 4);
    put32(&mut b, 36, u32::from(listen));
    b
}
fn parse(b: &[u8]) -> Option<Event> {
    if b.len() < 52 {
        return None;
    }
    let sequence = get32(b, 24)?;
    let what = get32(b, 36)?;
    if what == 0 {
        return None;
    }
    let cpu = get32(b, 40)?;
    let timestamp_ns = get64(b, 44)?;
    let u = 52;
    let kind = match what {
        1 => Kind::Fork {
            parent: get32(b, u)?,
            child: get32(b, u + 8)?,
        },
        2 => Kind::Exec { pid: get32(b, u)? },
        0x8000_0000 => Kind::Exit {
            pid: get32(b, u)?,
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
    #[test]
    fn parses_exec() {
        let mut b = [0_u8; 68];
        put32(&mut b, 36, 2);
        put32(&mut b, 40, 3);
        b[44..52].copy_from_slice(&99_u64.to_ne_bytes());
        put32(&mut b, 52, 42);
        assert_eq!(parse(&b).unwrap().kind, Kind::Exec { pid: 42 });
    }
    #[test]
    fn subscription_is_well_formed() {
        let b = subscription_message(7, true);
        assert_eq!(get32(&b, 0), Some(40));
        assert_eq!(get32(&b, 36), Some(1));
    }
}
