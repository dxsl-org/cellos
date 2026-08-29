#!/usr/bin/env bash

# Phase-06 probe schema retained for the ARM runner. These markers remain
# non-qualifying where that runner reports its existing environment block.
TIER3_HOSTILE_CORPUS=(
  "bounds|[HOSTILE_PROBE] BOUNDS_TEST_NOT_APPLICABLE|1|not_applicable"
  "descriptor|[HOSTILE_PROBE] DESC_TEST_NOT_APPLICABLE|2|not_applicable"
  "backend|[HOSTILE_PROBE] BACKEND_TEST_NOT_APPLICABLE|3|not_applicable"
  "budget|[HOSTILE_PROBE] BUDGET_TEST_STARTED|5|hostile_input_not_asserted"
  "reset|[HOSTILE_PROBE] RESET_TEST_STARTED|4|hostile_input_not_asserted"
)

# Ordered x86 guest stimuli and their host-authored production-path outcomes.
# START/DONE lines only delimit an interval; they are never acceptance evidence.
X86_VIRTIO_HOSTILE_CORPUS=(
  "invalid-queue-select|[hv-virtio-host] reject queue-select|transport"
  "queue-size-zero|[hv-virtio-host] reject queue-ready|queue"
  "queue-size-non-power-two|[hv-virtio-host] reject queue-ready|queue"
  "queue-size-oversize|[hv-virtio-host] reject queue-ready|queue"
  "descriptor-zero|[hv-virtio-host] reject queue-ready|queue"
  "descriptor-misaligned|[hv-virtio-host] reject queue-ready|queue"
  "avail-zero|[hv-virtio-host] reject queue-ready|queue"
  "avail-misaligned|[hv-virtio-host] reject queue-ready|queue"
  "used-zero|[hv-virtio-host] reject queue-ready|queue"
  "used-misaligned|[hv-virtio-host] reject queue-ready|queue"
  "descriptor-span-overflow|[hv-virtio-host] reject queue-ready|queue"
  "avail-span-overflow|[hv-virtio-host] reject queue-ready|queue"
  "used-span-overflow|[hv-virtio-host] reject queue-ready|queue"
  "notify-before-driver-ok|[hv-virtio-host] reject queue-notify-before-driver-ok|transport"
  "notify-invalid-config|[hv-virtio-host] reject queue-notify-invalid|transport"
  "reset-clears-state|[hv-virtio-host] reset|reset"
  "pending-index-delta|[hv-virtio-host] reject pending-delta|descriptor"
  "descriptor-head-oob|[hv-virtio-host] reject descriptor-chain|descriptor"
  "descriptor-next-oob|[hv-virtio-host] reject descriptor-chain|descriptor"
  "descriptor-payload-overflow|[hv-virtio-host] reject descriptor-chain|descriptor"
  "backend-unsupported-opcode|[hv-blk-host] request failed type=255 sector=0 buffers=2 status=2|backend"
  "net-recovery-sentinel|[hv-virtio-host] net-tx-complete|net"
)

X86_VIRTIO_HOSTILE_BLOCKED=(
  "backend-disconnect|fault-injectable persistent VFS backend plus a host-authored disconnect/reconnect observation"
  "net-backend-disconnect|fault-injectable Net Cell backend plus host-authored drop/recovery observations"
  "arm64-execution|an ARM TCG environment that reaches the guest probe without the existing synchronous fault"
)
