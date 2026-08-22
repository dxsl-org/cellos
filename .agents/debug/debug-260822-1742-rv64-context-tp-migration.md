# RV64 saved-Context `tp` migration A/B

**Hypothesis result:** FALSIFIED for the exact unpinned two-hart init fault.

**Confidence:** CONFIRMED (2 signals: two fresh `-smp 2` boots + raw-switch boundary capture)

## Question

Whether a Context saved on a source hart restores a stale hart-local `tp` on a destination hart, causing the observed unpinned init/VFS corruption.

## Temporary diagnostic

Only test-hook builds were changed for the run. At each RV64 raw-switch boundary, the temporary assembly hook received:

1. the `tp` just loaded from the incoming `Context`,
2. the known destination hart kernel `tp`, and
3. the incoming Context address.

The control left the loaded Context `tp` unchanged. The treatment would immediately write the destination `tp` before the incoming scheduler-completion callback, but only after a real mismatch. The hook emits a first-boundary sample and a detailed `TP-MIGRATION` record when it observes a mismatch.

## Fresh-image A/B evidence

Both images used the same freshly rebuilt and signed embedded init/kernel filesystem and QEMU `virt -m 256M -smp 2` runner. The runner required a boundary sample and the exact historical init fault for each side, and rejected a registry/VFS healthy terminal in either result.

| Mode | First boundary sample | Boundary mismatch before init fault | Endpoint |
|---|---|---|---|
| control | `source-hart=Some(0) destination-hart=Some(0) context=0x807b4cd8 context-tp=0x8070da90 destination-tp=0x8070da90 mismatch=false` | none (`TP-MIGRATION` absent) | exact init NX fault |
| treatment | `source-hart=Some(0) destination-hart=Some(0) context=0x807b4cd8 context-tp=0x8070da90 destination-tp=0x8070da90 mismatch=false` | none (`TP-MIGRATION` absent; no rebind was warranted) | same exact init NX fault |

Both boots reached:

```text
Cellos: Kernel exception: scause=12 sepc=0x817cfd88 stval=0x817cfd88
```

Neither reached `Init: service registry verified.` or `[vfs-test] ALL TESTS PASSED` before that fault. The treatment cannot repair a stale saved `tp` when the capture proves no differing Context/destination `tp` was restored before the signature; it reproduced the same endpoint.

## Command and result

```sh
cargo test --manifest-path tests/integration/Cargo.toml \
  --target x86_64-unknown-linux-gnu \
  --test tp-migration-diagnostic -- \
  --exact compare_saved_context_tp_control_and_treatment --nocapture
```

```text
CONTROL_TP_BOUNDARY=... mismatch=false
TREATMENT_TP_BOUNDARY=... mismatch=false
CAUSAL_VERDICT=saved-context-tp-mismatch-falsified-before-init-fault
test result: ok. 1 passed; 0 failed
```

## Conclusion

The stale source-hart `Context.tp` mechanism is not causal for the exact pre-registry init fault under this shared fresh two-hart boot. This does not explain the separate pinned-init VFS `stval=0x58` fault; that remains a distinct investigation. No production fix is justified by this experiment.

## Baseline disposition

Destination-`tp` rebinding is deferred from the RV64 retirement-causality baseline. The baseline retains per-hart boot contexts, secondary SIE enablement, selected/executing ownership pins, completion-epoch publication, task-to-idle attribution clearing, and the pre-selection SIE/full-`sstatus` switch ABI. It excludes destination-`tp` binding and deferred requeue until each has an independent control/treatment proof tied to a specific fault signature.

## Cleanup

The temporary kernel features, context-switch argument/hook, assembly call, runner test, integration manifest stanza, and `/tmp` diagnostic images were removed after the recorded run. This report is the sole retained diagnostic artifact.
