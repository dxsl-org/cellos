#!/usr/bin/env bash
# S10-3 stream B: L4 GUI 6/6 rule (S10-3.5) on the desktop candidate in import mode: five click
# launches then one autoload, consecutive; any failure resets the count (no best-of-N), up to a
# bounded number of attempts. Then one default-mode GUI run. Records via run_lane.py.
set -eu
L=/tmp/cellbox-s10-3c-resume/lanes
C=/tmp/cellbox-s10-3c-resume/cand/candidate.json
R="python3 $L/run_lane.py"
streak=0; attempt=0
while (( streak < 6 && attempt < 12 )); do
    attempt=$((attempt + 1))
    if (( streak < 5 )); then mode=click; else mode=autoload; fi
    if $R "L4-gui-attempt$attempt-$mode" /tmp/cellbox-phase04-gui- CELLBOX_PHASE04_WORK GUI_LAUNCH=$mode -- \
        scripts/qemu-phase04-gui-test.sh --candidate "$C"; then
        streak=$((streak + 1))
    else
        streak=0
    fi
    echo "L4 attempt $attempt ($mode): streak $streak" | tee -a "$L/l4-streak.txt"
done
echo "L4 final streak $streak after $attempt attempts" | tee -a "$L/l4-streak.txt"
$R default-p04-gui /tmp/cellbox-phase04-gui- CELLBOX_PHASE04_WORK GUI_LAUNCH=click -- scripts/qemu-phase04-gui-test.sh
echo "stream B done"
