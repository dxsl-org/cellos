#!/usr/bin/env bash
# Shared cell-signing helper for the image-assembly scripts. Source it, then call
# `sign_cells <binary>...`, which runs the F1/F5 check and signs only if it passes.
#
# WHY every image needs this: a cell with no `__ViCell_sig` section is DENIED by
# loader::spawn_gated under the `signing-required` feature, and the guest never
# reaches a shell. The symptom misdirects — init prints "cell not found — skipping"
# and the only mention of signatures is a kernel-side `[loader] DENY` line. Before
# this helper, gen_disk.ps1 was the sole lane that signed, so `signing-required`
# could not be enabled in CI at all: four of five image lanes would have died.
#
# Requires `$PYTHON_BIN` (the caller's probed interpreter) to be set already.
# Idempotent: sign-cell.py strips a stale __ViCell_sig before adding the new one, so
# re-running over an already-signed target/ directory is safe.

# Resolve a cross objcopy for the target ELF architecture. A host objcopy exits 1
# with "Unable to recognise the architecture of the input file", so this cannot fall
# back to plain `objcopy`. Honors a pre-set $OBJCOPY; otherwise probes the CI package
# name first (gcc-riscv64-unknown-elf on Ubuntu), then the local xpack name.
#
# $1: optional space-separated candidate list, for callers targeting another arch.
resolve_objcopy() {
    local candidates="${1:-riscv64-unknown-elf-objcopy riscv-none-elf-objcopy}"
    if [[ -n "${OBJCOPY:-}" ]]; then
        export OBJCOPY
        return 0
    fi
    local cand
    for cand in $candidates; do
        if command -v "$cand" >/dev/null 2>&1; then
            OBJCOPY="$cand"
            export OBJCOPY
            return 0
        fi
    done
    echo "FAIL: no cross objcopy found (tried: $candidates)" >&2
    return 1
}

# Check F1/F5, then sign each argument in place and re-verify — one `cellos-sign`
# invocation, because the signature now attests "built by a pipeline that enforced
# F1", not merely "these bytes are ours" (Spec 18 §2.1). Splitting the check from
# the signing would make the attestation a claim nobody checks, so this helper has
# no sign-only path: if the check fails, nothing is signed and the image build stops.
#
# F5 is mandatory here and needs no flag: `cellos-sign --sign` forces strict mode, so
# a build host that cannot confirm the pinned toolchain refuses to sign rather than
# printing SKIP and signing anyway. A caller cannot weaken that by forgetting an option.
#
# Verifying after the embed is also not redundant: objcopy can exit 0 having written
# a section the kernel's payload rules reject — the ELF header is deliberately
# excluded from the signed bytes, so a layout change invalidates the signature
# without failing the embed.
sign_cells() {
    [[ $# -gt 0 ]] || return 0
    resolve_objcopy || return 1
    local bin
    for bin in "$@"; do
        if [[ ! -f "$bin" ]]; then
            echo "FAIL: cannot sign missing binary: $bin" >&2
            return 1
        fi
    done
    "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "$OBJCOPY" --sign "$@"
}
