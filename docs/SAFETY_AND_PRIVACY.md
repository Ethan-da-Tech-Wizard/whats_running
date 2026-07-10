# Safety, privacy, and trust

Status: Proposed

## Threat model

Inputs are hostile or unreliable: process names can contain terminal control characters; command lines can contain secrets; procfs entries vanish; counters can be malformed in fixtures or surprising on future kernels; privileged processes deny fields.

Potential harm includes leaking secrets through exports/screenshots, misleading users with false completeness, consuming enough resources to worsen the investigated problem, enabling unsafe process termination, or expanding privilege unnecessarily.

## Required controls

- Escape terminal control codes and untrusted text.
- Default exports omit/redact command lines and never include environments.
- Use restrictive file creation permissions.
- No telemetry or network access in MVP.
- Run unprivileged; explain optional elevated visibility without prompting for passwords.
- Read only documented system interfaces; never write `/proc` or `/sys`.
- Bound polling frequency, memory history, per-field string size, and rendering work.
- Display monitoring overhead and sample overruns.
- Never infer “malicious” from a process name or resource use.

## Completeness statement

“All processes” means all process directory entries successfully enumerated in the procfs/PID namespace visible to the observer during a non-atomic sampling window. Some entries may disappear before reading. Permissions, `hidepid`, namespaces, kernel compromise, firmware, and processes whose lifetime falls between polls can limit visibility.

This statement must be accessible from the UI and exported metadata.

## Privilege policy

Root is not a product requirement. Elevated execution may expose additional paths/commands/accounting, but increases consequences of parser or terminal bugs. The tool must never install setuid, silently invoke `sudo`, or imply that root defeats kernel-level deception.

## Safety review checklist

- Fuzz parsers and terminal escaping.
- Test secret-bearing command lines and filenames.
- Audit dependencies and release artifacts.
- Verify zero network syscalls in representative MVP operation.
- Test hostile process names and huge argument lists.
- Test resource exhaustion with large process/thread counts.
- Document kernel/namespace observation boundary.
- Establish vulnerability reporting before public release.

