# ADR-0005: Dependency-free Arch terminal interface

Status: Accepted  
Date: 2026-07-10

Use ANSI alternate-screen output, byte-key input, and GNU `stty` raw-mode/dimension management. Reuse the collector/rate model; add no terminal crate, async runtime, daemon, or Hyprland IPC.

This preserves a small dependency-free binary but ties M3 TUI handling to the Arch/Linux reference environment. See `docs/verification/M3-2026-07-10.md`.

