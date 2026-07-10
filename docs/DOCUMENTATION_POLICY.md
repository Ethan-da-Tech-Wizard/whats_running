# Documentation policy

Status: Accepted  
Effective: 2026-07-10

## The append-only promise

The intent is that project reasoning never disappears. Once a dated journal entry, decision record, released requirements baseline, incident record, or safety review is accepted, it must not be deleted or silently rewritten.

Git history alone is not the policy. Git is backup evidence; the readable repository must also preserve the chain of thought and decisions.

## Document classes

1. **Living indexes** (`README.md` files) may be extended and reorganized. Existing links and historical facts should remain when still meaningful.
2. **Proposals** may be edited while explicitly Proposed. Acceptance freezes their semantic baseline.
3. **Immutable records** include accepted ADRs, journal entries, releases, incidents, research results, and scope locks. Correct them only through an appended correction or superseding document.
4. **Generated artifacts** may be regenerated, but their source and generation method must be retained.

## How to change an accepted idea

1. Create a new dated record.
2. State what it supersedes and why.
3. Append a `Superseded by ...` note to the old record; do not erase its original body.
4. Update indexes to point to both the current record and history.

## Corrections

Append a section in this form:

```text
## Correction — YYYY-MM-DD
The earlier statement ... was incorrect/incomplete.
Correct statement: ...
Reason/evidence: ...
```

Never conceal credentials, personal data, or dangerous material merely to satisfy append-only history. If such content is committed, remove it safely, document that a redaction occurred, and rotate affected secrets. Privacy and safety outrank historical completeness.

## Enforcement plan

- Use dated, unique filenames for journal entries and ADRs.
- Add CI later to reject deletion or modification of paths declared immutable in a release manifest.
- Sign tagged requirement baselines where practical.
- Require a reason in every superseding record.
- Keep raw research captures separate from conclusions.

