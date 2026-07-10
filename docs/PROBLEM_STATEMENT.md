# Problem statement

Status: Proposed

## Human problem

When a computer becomes hot, loud, slow, or battery-hungry, the user sees symptoms but not a coherent explanation. Existing tools often expose tables designed for experts, omit contextual relationships, update too quickly, differ by privilege level, or imply more certainty than the operating system provides. That uncertainty can become anxiety: “Is something hidden? Is this normal? Why is it doing that?”

## Product problem

Build a local observer that turns volatile operating-system process data into a transparent snapshot and understandable narrative while remaining low-overhead, auditable, race-tolerant, and explicit about its blind spots.

## Problem statement template to validate

For **[user]**, who experiences **[symptom/anxiety]**, existing **[tools/workflow]** fail because **[observed deficiency]**. We will provide **[capability]**, measured by **[outcome]**, while respecting **[constraints and non-goals]**.

## Evidence required before freezing the statement

- Interview or observe at least five target users using `ps`, `top`, `htop`, and system monitors.
- Catalogue the questions they ask first and where vocabulary breaks down.
- Measure baseline overhead and discoverability in comparable tools.
- Test whether process ancestry, resource attribution, and plain-language explanations reduce time-to-answer.
- Separate “security reassurance” from “performance diagnosis”; they overlap but are not identical promises.

## Candidate jobs to be done

- When my fans spin up, show what is consuming CPU and whether that use persists.
- When the system feels sluggish, show pressure on CPU, memory, swap, and storage.
- When I see an unfamiliar name, show origin, executable path, user, parent, package provenance when knowable, and uncertainty.
- When activity is transient, preserve a bounded local history so I can inspect what just happened.
- When access is denied, tell me what is missing and whether elevation would reveal more.

