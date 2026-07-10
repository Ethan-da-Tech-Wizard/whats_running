# Initial brainstorm

Date: 2026-07-10  
Status: Historical record

## Raw intent distilled

The user wants relief from uncertainty about background activity, heat, and slowness. They want every process shown, including the observer, first on Arch Linux and eventually on Windows/macOS. They favor practical KISS engineering and unusually deep, additive documentation.

## Early product personality

The application should feel less like a cockpit full of unexplained gauges and more like a glass engine cover: exact machinery remains visible, but labels connect it to the human question. It must resist two temptations—hiding complexity and pretending complexity is certainty.

## Brainstorm inventory (not commitments)

- Complete process table, process tree, and “changed recently” view.
- Observer-effect badge showing own CPU/RAM and collection latency.
- Visibility report: namespace, permissions, denied fields, sample interval, vanished processes.
- Sustained-heat ranking using a rolling window rather than a twitchy instant.
- Process story: executable → package → parent → systemd unit/cgroup → resource trend.
- Snapshot export for asking for help without leaking secrets.
- “Why might this be hot?” explanations that distinguish correlation from causation.
- Optional advanced event tracing later, never necessary for the honest polling MVP.

## Biggest conceptual discovery

“Show everything” is a trust requirement, not merely a UI checkbox. The implementable promise is: enumerate everything visible inside a precisely stated OS boundary, never intentionally exclude the tool, and report all known gaps. Absolute visibility cannot be guaranteed from user space.

## Immediate next questions

What should the default view optimize for: reassurance, heat diagnosis, or exhaustive inspection? How much command-line detail is safe by default? Should threads be initially collapsed under processes? These should be resolved through research rather than taste alone.

