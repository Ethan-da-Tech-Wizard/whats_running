# ADR-0001: Documentation is append-only historical evidence

Status: Accepted  
Date: 2026-07-10

## Context

The project owner wants extensive brainstorming and product documentation that is never silently overwritten or deleted.

## Decision

Use the rules in `docs/DOCUMENTATION_POLICY.md`. Accepted records remain readable; corrections are appended and semantic changes create superseding records.

## Consequences

The repository grows and may contain obsolete ideas. Indexes and explicit statuses are therefore essential. Sensitive data may still require redaction because safety outranks retention.

## Verification

Later CI will compare immutable release manifests and reject unacknowledged mutations/deletions.

