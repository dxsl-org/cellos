# Phase 01 — ECAM Range and Scanner

Status: completed

## Change

- Admit a valid segment-0 MCFG allocation even when `bus_start != 0`.
- Compute checked inclusive bus count and ECAM byte length.
- Pass the range into x86 paging; map only the admitted window.
- Claim the same bounded window in Platform Cell.
- Scan every admitted bus using `(bus - bus_start) << 20` offsets.
- Register canonical requester IDs with `bus << 8 | dev << 3 | function`.
- Enable bus mastering using the same admitted range and relative offset.

## Acceptance

- Unit tests cover zero/nonzero starts, endpoints, reversed range, and overflow-safe address sizing.
- Existing bus-0 BAR probe behavior remains unchanged.
- Out-of-range BDFs cannot reach ECAM MMIO.

## Evidence

- Platform and kernel host suites passed 9/9 and 100/100; the `driver-nvme`
  cross-check passed.
- A fresh cells → kernel → ISO build passed.
- Strict q35 multibus passed 2/2 and registered the NVMe endpoint at `01:00.0`.
- The raw MCFG base remains bus-0-relative and is normalized by `bus_start`;
  the frozen syscall ABI is unchanged.
