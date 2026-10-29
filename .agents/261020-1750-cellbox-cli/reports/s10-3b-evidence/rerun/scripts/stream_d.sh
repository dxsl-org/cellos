#!/usr/bin/env bash
# S10-3 stream D (after S10-3b): stream C ended with a reset in each profile (headless run 2: driver
# idle detection, desktop run 3: the Docker sshd fixture did not start). The 6-consecutive rule has no
# best-of-N: any failure resets the count. Continue each profile from its current streak (read from
# l0-runs.txt by code) until six consecutive passes, bounded at 12 runs per profile in total.
set -u
L=/tmp/cellbox-s10-3b-tsbn/lanes
C=/tmp/cellbox-s10-3b-tsbn/cand/candidate.json
R="python3 $L/run_lane.py"
for profile in headless desktop; do
    read -r runs streak < <(awk -v p="$profile" '$2 == p { n++; s = ($NF == "0") ? s + 1 : 0 } END { print n + 0, s + 0 }' "$L/l0-runs.txt")
    while (( streak < 6 && runs < 12 )); do
        runs=$((runs + 1))
        $R "L0-$profile-run$runs" /tmp/cellbox-phase10- P10_WORK -- \
            scripts/qemu-phase10-canonical-test.sh --candidate "$C" --profile "$profile" --work "{work}"
        rc=$?
        if (( rc == 0 )); then streak=$((streak + 1)); else streak=0; fi
        echo "L0 $profile run $runs: exit $rc" | tee -a "$L/l0-runs.txt"
    done
    echo "L0 $profile final streak $streak after $runs runs" | tee -a "$L/l0-streak.txt"
done
echo "stream D done"
