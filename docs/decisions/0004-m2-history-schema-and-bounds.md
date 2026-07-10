# ADR-0004: Finite schema-v2 history with a 120-sample hard bound

Status: Accepted  
Date: 2026-07-10

## Context

M2 needs rates and recent history without turning the program into a daemon or unbounded recorder.

## Decision

Collect a user-requested finite number of samples, reject values above 120, retain them only in memory, and export one schema-v2 JSON object containing a `samples` array. Derive rates only between matching `(PID, start_ticks)` identities.

## Consequences

Behavior is predictable and resource-bounded. Capturing longer periods requires repeated invocations or a later explicitly designed persistence feature. The first sample has unavailable rates by design.

## Verification

See `docs/verification/M2-2026-07-10.md`.

