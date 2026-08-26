# D5 — per-request server cell-scale profile

**Status:** approved for Part 6 application.

## Finding

The bare `1000+ Cells` NFR omits the memory model needed to interpret it. Current defaults remain
`MAX_CELLS = 64`, a 16 MiB quota, preallocated stacks, and full per-spawn ELF copies. Spec 19
already records the correct direction, while A1 and A3 now provide real RAM discovery and memory
measurement.

The actor-future alternative does not replace per-request isolation: futures in one cell share a
heap, quota, and capability set. The server profile is therefore a legitimate separate goal.

## Recommended ruling [FINAL]

**Accept recommendation A: retain 1000 simultaneous isolated cells as the per-request server
profile goal, with explicit prerequisites and staged measurements.**

1. The current large-app profile and 64-cell default remain unchanged.
2. Before raising table limits, measure N=64/128/256/512 for per-spawn committed memory, spawn
   latency, and isolation behavior.
3. The 1000-cell qualification requires shared immutable `.text`/`.rodata` frames after W^X,
   demand-paged stacks, profile-specific quotas, and dynamically sized cell/VA tables.
4. Queue this implementation behind the Midori WIP gate; this ruling authorizes no runtime or ABI
   change.
