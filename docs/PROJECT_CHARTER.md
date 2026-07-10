# Project charter

Status: Proposed  
Working name: **What's Running?** (codename: **Glassbox**)

## Vision

Give an ordinary computer owner a trustworthy, comprehensible, real-time view of the work their machine is performing, beginning with Arch Linux and eventually supporting Linux, Windows, and macOS.

## User promise

“I will show every process I can actually observe, including myself; tell you when the operating system prevents or races me; and help connect resource use to heat, noise, battery drain, and sluggishness.”

## Important honesty boundary

No user-space program can literally guarantee that it sees everything. Rootkits, kernel/firmware activity, containers or namespaces, permission boundaries, very short-lived processes, and kernel accounting limitations can make activity invisible. The product must say **complete relative to a defined observation boundary**, never “omniscient.”

## Primary users

- Arch Linux users who want reassurance and understandable diagnostics.
- Power users investigating CPU, RAM, disk I/O, process churn, or thermal symptoms.
- Future: support technicians and users on other desktop operating systems.

## Success signals

- A user can identify the current top resource consumers in under 30 seconds.
- Every observed PID can be traced to an executable, owner, state, and parent when the OS exposes them.
- The tool is visibly present in its own process list.
- Missing/denied/raced data has a reason rather than a blank or fabricated value.
- Monitoring overhead is measured and displayed.
- The core works without root; elevated operation is optional and explained.

## Anti-goals

- Being an antivirus, rootkit detector, firewall, package manager, or universal security verdict engine.
- Claiming that an unfamiliar process is malicious.
- Uploading process data by default.
- Killing, renicing, or suspending processes in v1.
- Replacing `top`, `htop`, `ps`, or professional tracing tools for every expert workflow.

