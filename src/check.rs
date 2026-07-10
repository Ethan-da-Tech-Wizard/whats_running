use crate::activity::Inventory;
use crate::procfs::{Field, Snapshot};
use crate::sanitize;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

pub fn report(snapshot: &Snapshot, inventory: &Inventory) {
    let mut names: Vec<(String, u32)> = snapshot
        .processes
        .iter()
        .map(|p| (p.name.value().cloned().unwrap_or_else(|| "?".into()), p.pid))
        .collect();
    names.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then(a.1.cmp(&b.1))
    });

    println!("What's Running? — quick check\n");
    println!(
        "RUNNING PROCESSES ({} total; scan for anything unexpected)",
        names.len()
    );
    for (name, pid) in &names {
        println!("  {:<28} pid {}", sanitize(name), pid);
    }

    println!("\nLISTENING PORTS (network services other programs could reach)");
    print_listening_ports(snapshot, inventory);

    println!("\nFor the full interactive inspector: whats-running --tui");
}

fn print_listening_ports(snapshot: &Snapshot, inventory: &Inventory) {
    let pid_names: HashMap<u32, String> = snapshot
        .processes
        .iter()
        .map(|p| (p.pid, p.name.value().cloned().unwrap_or_else(|| "?".into())))
        .collect();
    let sockets = match &inventory.sockets {
        Field::Value(sockets) => sockets,
        other => {
            println!("  <{}>", other.status());
            return;
        }
    };
    let mut listening: Vec<_> = sockets
        .iter()
        .filter(|s| matches!(s.protocol, "tcp" | "tcp6" | "udp" | "udp6"))
        .filter(|s| s.protocol.starts_with("udp") || s.state == "0A")
        .collect();
    if listening.is_empty() {
        println!("  none found");
        return;
    }
    listening.sort_by(|a, b| a.local.cmp(&b.local));
    for socket in listening {
        let address = decode_local(&socket.local, socket.protocol);
        let owner = if socket.owners.is_empty() {
            "unknown owner".to_string()
        } else {
            socket
                .owners
                .iter()
                .map(|pid| {
                    format!(
                        "{} (pid {pid})",
                        sanitize(pid_names.get(pid).map(String::as_str).unwrap_or("?"))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("  {:<5} {:<22} {}", socket.protocol, address, owner);
    }
}

fn decode_local(local: &str, protocol: &str) -> String {
    let Some((addr_hex, port_hex)) = local.split_once(':') else {
        return local.to_string();
    };
    let Ok(port) = u16::from_str_radix(port_hex, 16) else {
        return local.to_string();
    };
    let decoded = if protocol.ends_with('6') {
        decode_v6(addr_hex).map(|ip| format!("[{ip}]:{port}"))
    } else {
        decode_v4(addr_hex).map(|ip| format!("{ip}:{port}"))
    };
    decoded.unwrap_or_else(|| local.to_string())
}

fn decode_v4(hex: &str) -> Option<Ipv4Addr> {
    let bytes = hex_bytes(hex)?;
    let [a, b, c, d] = bytes[..].try_into().ok()?;
    Some(Ipv4Addr::new(d, c, b, a))
}

fn decode_v6(hex: &str) -> Option<Ipv6Addr> {
    let bytes = hex_bytes(hex)?;
    let bytes: [u8; 16] = bytes[..].try_into().ok()?;
    let mut reordered = [0u8; 16];
    for word in 0..4 {
        for byte in 0..4 {
            reordered[word * 4 + byte] = bytes[word * 4 + (3 - byte)];
        }
    }
    Some(Ipv6Addr::from(reordered))
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_ipv4_loopback_and_port() {
        assert_eq!(decode_local("0100007F:0016", "tcp"), "127.0.0.1:22");
    }
    #[test]
    fn decodes_ipv4_wildcard() {
        assert_eq!(decode_local("00000000:1F90", "tcp"), "0.0.0.0:8080");
    }
    #[test]
    fn decodes_ipv6_loopback() {
        assert_eq!(
            decode_local("00000000000000000000000001000000:0050", "tcp6"),
            "[::1]:80"
        );
    }
    #[test]
    fn falls_back_on_malformed_input() {
        assert_eq!(decode_local("not-an-address", "tcp"), "not-an-address");
    }
}
