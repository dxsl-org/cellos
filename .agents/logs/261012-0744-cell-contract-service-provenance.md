# 2026-10-12 — P01 source-rebuilt service image

## What happened
Rebuilt selected init/shell/VFS/config/platform/NET/input/x86serial/e1000/bench/probe into isolated target, devsigned11executables through F1/F5, packed freshFAT16/policy/init, built kernel and newISO. No source/ABI edits, oldartifact overwrite, lockrelease or commit.

## Decisions
- Initial signing refused missing driver-serial executable; library-only package corrected to driver-serial-x86 and signed through samegate, no bypass.
- Recorded3174input hashes are equality at observed snapshots captured during initial asyncbuild/afterrun, not atomicprebuild or exhaustive externalprovenance.
- Independent10binary ramdisk readbacks+policy/init equality and init/ramdisk→kernel→ISO containment close selected packaging chain. Included is not necessarily running.

## Evidence
- Cellbuildsuccess with4NETwarnings; kernelbuild/ISOsuccess. Fullcommand/digestchain service-x86-provenance.json.
- ActualnewISO shellfree twice and8bootwatermarks; knownVMMfailure remains explicit. No overall qualificationPASS or marginal/highwatercapacity claim.
- Currentdevelopment Tier2enabled oneCPU differs from historicaldisabledimage; no newcontract/remote/production gateapproval.
-25docs/286links/0missingpaths;10pendingphases/acyclic;38locks/adoptionrows;50ACowners;3negativecontrols.

## Next steps
Finite incremental/retained/highwater/SMP/CPU/profile measurements and exactcandidate closure/checkpoint1 proposals remain P01. No shell→probe grantinvented to force unauthorized measurement.
