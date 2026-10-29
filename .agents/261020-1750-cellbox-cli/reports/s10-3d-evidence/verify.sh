#!/usr/bin/env bash
set -euo pipefail
cd /home/dmin/cellos
W=/tmp/cellbox-s10-3d-resume
export CARGO_TARGET_DIR="$W/host-target"
cargo test -p driver-nvme -p app-init -p service-platform -p app-cellbox -p service-vfs --target x86_64-unknown-linux-gnu > "$W/logs/host-tests.log" 2>&1
printf 'PASS: host behavioral tests\n'
python3 -B scripts/test_phase10_canonical_drive.py > "$W/logs/harness-tests.log" 2>&1
printf 'PASS: harness tests\n'
python3 "$W/lanes/run_lane.py" regression-p04-terminal /tmp/cellbox-phase04- CELLBOX_PHASE04_WORK -- scripts/qemu-phase04-terminal-test.sh
python3 "$W/lanes/run_lane.py" regression-p02-kernel /tmp/cellbox-phase02- CELLBOX_PHASE02_WORK PHASE02_KERNEL_TESTS=1 -- scripts/qemu-phase02-session-test.sh
python3 -B "$W/build-controlled.py" > "$W/logs/controlled.log" 2>&1
printf 'PASS: controlled scan-before-driver proof\n'
bash "$W/build-candidate.sh"
printf 'PASS: rebuilt fleet candidate\n'
python3 -B "$W/battery.py"
