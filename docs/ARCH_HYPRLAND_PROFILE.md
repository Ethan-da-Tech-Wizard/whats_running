# Arch Linux + Hyprland reference profile

Status: Verified reference environment  
Date: 2026-07-10

The primary desktop is Arch Linux with Hyprland on Wayland. Validation observed `XDG_CURRENT_DESKTOP=Hyprland`, `XDG_SESSION_TYPE=wayland`, and installed `Hyprland`/`hyprctl` commands.

## Design consequences

- Process truth still comes from kernel procfs; Hyprland is not asked for the process list.
- The terminal output avoids assumptions about a specific terminal emulator and sanitizes control characters.
- The application remains usable from a TTY, SSH session, launcher, or Hyprland terminal binding.
- Future GUI work must be Wayland-native and must not require XWayland.
- A later Arch package may include an optional desktop entry; no autostart or background daemon will be installed silently.
- Hyprland IPC is optional future enrichment for mapping visible windows to processes. It cannot replace procfs and must never cause non-windowed processes to disappear.
- systemd user scopes and cgroup v2 are more reliable future ownership/context sources than compositor window lists.

## Suggested Hyprland binding after local installation

```text
bind = SUPER, R, exec, $terminal -e whats-running --samples 30 --interval-ms 1000
```

This is an example only because terminal variables and preferred keys differ between configurations. The program does not edit Hyprland configuration.

