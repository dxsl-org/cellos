#!/usr/bin/env bash
# Approved S10-3c verification, all writable outputs outside the repository.
set -euo pipefail
cd /home/dmin/cellos
W=/tmp/cellbox-s10-3c-resume
export CARGO_TARGET_DIR="$W/host-target"
cargo test -p driver-nvme -p app-init -p app-cellbox -p service-vfs --target x86_64-unknown-linux-gnu > "$W/logs/host-tests.log" 2>&1
printf 'PASS: affected host crates\n'
python3 -m unittest discover -s scripts -p test_phase10_canonical_drive.py > "$W/logs/harness-tests.log" 2>&1
printf 'PASS: harness behavior tests\n'
python3 "$W/lanes/run_lane.py" regression-p04-terminal /tmp/cellbox-phase04- CELLBOX_PHASE04_WORK -- scripts/qemu-phase04-terminal-test.sh
python3 "$W/lanes/run_lane.py" regression-p02-kernel /tmp/cellbox-phase02- CELLBOX_PHASE02_WORK PHASE02_KERNEL_TESTS=1 -- scripts/qemu-phase02-session-test.sh
printf 'PASS: default P04 terminal and P02 kernel regressions\n'
bash "$W/build-candidate.sh"
printf 'PASS: rebuilt fleet candidate\n'
