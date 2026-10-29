#!/usr/bin/env bash
# S10-3 stream A: import-mode serial lanes on the fleet candidate, then each runner once in its
# default (non-import) mode. Records go to lanes/summary.jsonl via run_lane.py.
set -eu
L=/tmp/cellbox-s10-3d-resume/lanes
C=/tmp/cellbox-s10-3d-resume/cand/candidate.json
R="python3 $L/run_lane.py"
$R L1-p02-session    /tmp/cellbox-phase02- CELLBOX_PHASE02_WORK -- scripts/qemu-phase02-session-test.sh --candidate "$C"
$R L1-p02-kernel     /tmp/cellbox-phase02- CELLBOX_PHASE02_WORK PHASE02_KERNEL_TESTS=1 -- scripts/qemu-phase02-session-test.sh --candidate "$C"
$R L2-p03-pipeline   /tmp/cellbox-phase03- CELLBOX_PHASE03_WORK -- scripts/qemu-phase03-pipeline-test.sh --candidate "$C"
$R L3-p04-terminal   /tmp/cellbox-phase04- CELLBOX_PHASE04_WORK -- scripts/qemu-phase04-terminal-test.sh --candidate "$C"
$R L5-p09-nofaults   /tmp/cellbox-phase09- CELLBOX_PHASE09_WORK -- scripts/qemu-phase09-ssh-test.sh --no-faults --candidate "$C"
$R L5f-p09-faults    /tmp/cellbox-phase09- CELLBOX_PHASE09_WORK -- scripts/qemu-phase09-ssh-test.sh
# Default (non-import) behaviour unchanged (S10-3.8). The P04 terminal default run is the S10-3a
# regression run; GUI default runs after the GUI battery (stream B).
$R default-p02-session  /tmp/cellbox-phase02- CELLBOX_PHASE02_WORK -- scripts/qemu-phase02-session-test.sh
$R default-p03-pipeline /tmp/cellbox-phase03- CELLBOX_PHASE03_WORK -- scripts/qemu-phase03-pipeline-test.sh
$R default-p09-nofaults /tmp/cellbox-phase09- CELLBOX_PHASE09_WORK -- scripts/qemu-phase09-ssh-test.sh --no-faults
echo "stream A done"
