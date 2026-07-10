# ADR-0009: Lock target platform to Arch Linux; reject portability as scope

Status: Accepted  
Date: 2026-07-10  
Supersedes: the "portability beyond Arch/systemd/cgroup-v2" item proposed in `docs/journal/2026-07-10-2000-tui-legend-visibility-and-status.md`

## Context

That journal entry listed "portability beyond the Arch/systemd/cgroup-v2 profile" as one of several unaccepted candidate next steps. The project owner has since stated the goal is specifically an AUR package — Arch Linux's own community repository — and does not want the tool to run anywhere else. Continuing to carry portability as an open question after that statement is inaccurate bookkeeping, not caution.

## Decision

Arch Linux, systemd, and cgroup v2 are permanent, accepted assumptions, not temporary conveniences to later generalize. `docs/ARCH_HYPRLAND_PROFILE.md` remains the only validated target. The tool may keep reporting `unsupported` for individual data sources it can't reach (that's the existing typed-field-failure model working as intended, e.g. no systemd manager reachable), but the project will not add distro/init detection, cgroup v1 fallback paths, or non-systemd unit sources to broaden reach beyond Arch.

## Alternatives considered

- **Distro-agnostic detection with graceful degradation** (query systemd if present, fall back otherwise): rejected. Nobody asked for it, and it would mean writing and testing code paths for environments the project owner doesn't use and can't verify.
- **Support other inits (OpenRC, runit, dinit)**: rejected for the same reason; there is no target user on those systems.

## Consequences

- Simplifies the AUR `PKGBUILD`: it can hard-depend on `systemd` rather than treating it as optional.
- Frees `docs/MILESTONES_V2.md` and `README.md` from carrying an open-ended cross-distro goal.
- Any future request to support another distro is new scope requiring its own ADR, not a resumption of deferred work.

## Verification

No code change; this is a scope decision. It is recorded here so `README.md`'s "candidate next steps" list stops listing portability as pending.
