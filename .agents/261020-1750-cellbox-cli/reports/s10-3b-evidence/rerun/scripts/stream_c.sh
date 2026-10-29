#!/usr/bin/env bash
# S10-3 stream C (after S10-3b): L0 canonical-surface runs on the rebuilt fleet candidate, six
# consecutive runs per profile (every run is recorded; no best-of-N). Records via run_lane.py.
set -u
L=/tmp/cellbox-s10-3b-tsbn/lanes
C=/tmp/cellbox-s10-3b-tsbn/cand/candidate.json
R="python3 $L/run_lane.py"
for profile in headless desktop; do
    for n in 1 2 3 4 5 6; do
        $R "L0-$profile-run$n" /tmp/cellbox-phase10- P10_WORK -- \
            scripts/qemu-phase10-canonical-test.sh --candidate "$C" --profile "$profile" --work "{work}"
        echo "L0 $profile run $n: exit $?" | tee -a "$L/l0-runs.txt"
    done
done
echo "stream C done"
